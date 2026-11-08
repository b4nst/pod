use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use futures::channel::{mpsc, oneshot};
use iced::widget::{button, canvas, checkbox, column, container, pick_list, row, scrollable, space, stack, text};
use iced::{Center, Element, Fill, Subscription, Task, Theme, time, window};

use pod_core::device::{self, Device};
use pod_core::flash::{self, Action, Report};
use pod_core::history::{self, Imported};
use pod_core::ipod::Bootloader;
use pod_core::release::{self, Release};
use pod_core::sync::{self, Mode, Plan};
use pod_core::{Error, Model, Progress, human_bytes, upgrade};

use crate::deck::{Cassette, Delta, Dots, Hatch, Ipod, Meter};
use crate::theme::{self, DECK, DECK_BOLD, MONO, MONO_BOLD};

pub fn run() -> iced::Result {
    let mut app = iced::application(App::boot, App::update, App::view)
        .title("pod link")
        .theme(|_: &App| theme::theme())
        .subscription(App::subscription)
        .default_font(MONO)
        .window_size((1000.0, 680.0))
        .antialiasing(true);
    for f in theme::FONTS {
        app = app.font(f);
    }
    app.run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Flash,
    Upgrade,
    Sync,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Flash => "FLASH",
            Tab::Upgrade => "UPGRADE",
            Tab::Sync => "SYNC",
        }
    }
}

#[derive(Debug, Clone)]
enum Remote {
    Loading,
    Ready(Option<Release>),
    Failed,
}

struct Job {
    label: &'static str,
    progress: Progress,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
enum Done {
    Flashed(Report),
    Upgraded(upgrade::Info),
    Planned(Plan, Result<Imported, String>),
    Synced { copied: usize, deleted: usize },
    Ejected,
}

#[derive(Debug, Clone)]
enum Event {
    Progress(Progress),
    Done(Result<Done, String>),
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Scanned(Vec<Device>),
    Select(String),
    Tab(Tab),
    Release(Result<Option<Release>, String>),
    Model(Model),
    UseRelease { firmware: bool },
    PickLocal { firmware: bool },
    Picked { firmware: bool, path: Option<PathBuf> },
    KeepSettings(bool),
    RemoveRockbox(bool),
    PickFolder,
    FolderPicked(Option<PathBuf>),
    SyncMode(Mode),
    Flash,
    Unflash,
    Upgrade,
    Scan,
    Sync,
    Eject,
    Cancel,
    Worker(Event),
    Frame(Instant),
}

struct App {
    devices: Vec<Device>,
    selected: Option<PathBuf>,
    scanning: bool,
    tab: Tab,
    release: Remote,
    model: Option<Model>,
    boot_local: Option<PathBuf>,
    fw_local: Option<PathBuf>,
    keep_settings: bool,
    remove_rockbox: bool,
    folder: Option<PathBuf>,
    mode: Mode,
    plan: Option<Plan>,
    report: Option<Report>,
    job: Option<Job>,
    status: (bool, String),
    angle: f32,
    last_frame: Option<Instant>,
}

/// Run blocking work off the UI thread.
fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    to: impl Fn(T) -> Message + Send + 'static,
) -> Task<Message> {
    let (tx, rx) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    Task::perform(rx, move |r| to(r.expect("worker thread died")))
}

impl App {
    fn boot() -> (App, Task<Message>) {
        let app = App {
            devices: Vec::new(),
            selected: None,
            scanning: true,
            tab: Tab::Flash,
            release: Remote::Loading,
            model: None,
            boot_local: None,
            fw_local: None,
            keep_settings: true,
            remove_rockbox: true,
            folder: None,
            mode: Mode::Update,
            plan: None,
            report: None,
            job: None,
            status: (false, "READY".into()),
            angle: 0.0,
            last_frame: None,
        };
        let tasks = Task::batch([
            blocking(device::scan, Message::Scanned),
            blocking(|| release::latest().map_err(|e| e.to_string()), Message::Release),
        ]);
        (app, tasks)
    }

    fn device(&self) -> Option<&Device> {
        let sel = self.selected.as_ref()?;
        self.devices.iter().find(|d| &d.mount == sel)
    }

    fn current_model(&self) -> Option<Model> {
        self.model.or_else(|| self.device().and_then(|d| d.model))
    }

    fn latest(&self) -> Option<&Release> {
        match &self.release {
            Remote::Ready(Some(r)) => Some(r),
            _ => None,
        }
    }

    fn say(&mut self, error: bool, msg: impl Into<String>) {
        self.status = (error, msg.into());
    }

    /// Run `f` on a worker thread, reporting progress to the footer.
    fn start(
        &mut self,
        label: &'static str,
        f: impl FnOnce(&mut dyn FnMut(Progress), &AtomicBool) -> Result<Done, Error> + Send + 'static,
    ) -> Task<Message> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(Job { label, progress: Progress::default(), cancel: cancel.clone() });
        self.say(false, format!("{label}..."));
        let (tx, rx) = mpsc::unbounded();
        std::thread::spawn(move || {
            let mut last = Instant::now() - Duration::from_secs(1);
            let ptx = tx.clone();
            let mut report = |p: Progress| {
                if last.elapsed() >= Duration::from_millis(33) || p.done == p.total {
                    last = Instant::now();
                    let _ = ptx.unbounded_send(Event::Progress(p));
                }
            };
            let r = f(&mut report, &cancel).map_err(|e| e.to_string());
            let _ = tx.unbounded_send(Event::Done(r));
        });
        Task::run(rx, Message::Worker)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                if self.scanning || self.job.is_some() {
                    return Task::none();
                }
                self.scanning = true;
                blocking(device::scan, Message::Scanned)
            }
            Message::Scanned(devices) => {
                self.scanning = false;
                let had = self.device().map(|d| d.mount.clone());
                if devices.iter().all(|d| Some(&d.mount) != had.as_ref()) {
                    self.selected = devices.first().map(|d| d.mount.clone());
                    self.model = None;
                    self.plan = None;
                    self.report = None;
                }
                self.devices = devices;
                Task::none()
            }
            Message::Select(label) => {
                self.selected = self.devices.iter().find(|d| d.label == label).map(|d| d.mount.clone());
                self.model = None;
                self.plan = None;
                Task::none()
            }
            Message::Tab(t) => {
                self.tab = t;
                Task::none()
            }
            Message::Release(r) => {
                self.release = match r {
                    Ok(r) => Remote::Ready(r),
                    Err(e) => {
                        self.say(true, format!("release check failed: {e}"));
                        Remote::Failed
                    }
                };
                Task::none()
            }
            Message::Model(m) => {
                self.model = Some(m);
                Task::none()
            }
            Message::UseRelease { firmware } => {
                *self.local_mut(firmware) = None;
                Task::none()
            }
            Message::PickLocal { firmware } => {
                let (what, ext) = if firmware { ("pod firmware", "zip") } else { ("pod bootloader", "ipod") };
                Task::perform(rfd::AsyncFileDialog::new().add_filter(what, &[ext]).pick_file(), move |f| {
                    Message::Picked { firmware, path: f.map(|f| f.path().to_path_buf()) }
                })
            }
            Message::Picked { firmware, path } => {
                if let Some(p) = path {
                    *self.local_mut(firmware) = Some(p);
                }
                Task::none()
            }
            Message::KeepSettings(v) => {
                self.keep_settings = v;
                Task::none()
            }
            Message::RemoveRockbox(v) => {
                self.remove_rockbox = v;
                Task::none()
            }
            Message::PickFolder => Task::perform(rfd::AsyncFileDialog::new().pick_folder(), |f| {
                Message::FolderPicked(f.map(|f| f.path().to_path_buf()))
            }),
            Message::FolderPicked(p) => {
                if p.is_some() {
                    self.folder = p;
                    self.plan = None;
                }
                Task::none()
            }
            Message::SyncMode(m) => {
                self.mode = m;
                self.plan = None;
                Task::none()
            }
            Message::Flash | Message::Unflash => self.flash(matches!(message, Message::Flash)),
            Message::Upgrade => self.upgrade(),
            Message::Scan => {
                let (Some(d), Some(src)) = (self.device(), self.folder.clone()) else { return Task::none() };
                let (mount, mode) = (d.mount.clone(), self.mode);
                self.plan = None;
                self.start("SCANNING", move |_, cancel| {
                    // a history problem is reported, but doesn't stop the scan
                    let base = history::stores_dir();
                    let imported = history::import(&mount, &base).map_err(|e| e.to_string());
                    Ok(Done::Planned(sync::plan(&src, &mount, mode, cancel)?, imported))
                })
            }
            Message::Sync => {
                let (Some(d), Some(plan)) = (self.device(), self.plan.clone()) else { return Task::none() };
                if let Err(e) = plan.check_space(d.free) {
                    self.say(true, e.to_string());
                    return Task::none();
                }
                let mount = d.mount.clone();
                self.start("SYNCING", move |progress, cancel| {
                    sync::apply(&plan, &mount, progress, cancel)?;
                    Ok(Done::Synced { copied: plan.copies.len(), deleted: plan.deletes.len() })
                })
            }
            Message::Eject => {
                let Some(d) = self.device().cloned() else { return Task::none() };
                self.start("EJECTING", move |_, _| {
                    d.eject()?;
                    Ok(Done::Ejected)
                })
            }
            Message::Cancel => {
                if let Some(j) = &self.job {
                    j.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Task::none()
            }
            Message::Worker(Event::Progress(p)) => {
                if let Some(j) = &mut self.job {
                    j.progress = p;
                }
                Task::none()
            }
            Message::Worker(Event::Done(r)) => {
                self.job = None;
                self.last_frame = None;
                match r {
                    Ok(done) => self.finished(done),
                    Err(e) => self.say(true, e),
                }
                // the volume may have been remounted or changed
                self.scanning = true;
                blocking(device::scan, Message::Scanned)
            }
            Message::Frame(now) => {
                if let Some(last) = self.last_frame {
                    self.angle += (now - last).as_secs_f32() * 2.4;
                }
                self.last_frame = Some(now);
                Task::none()
            }
        }
    }

    fn local_mut(&mut self, firmware: bool) -> &mut Option<PathBuf> {
        if firmware { &mut self.fw_local } else { &mut self.boot_local }
    }

    fn finished(&mut self, done: Done) {
        match done {
            Done::Flashed(r) => {
                self.say(false, format!("FLASHED: {} -> {}", r.before, r.after));
                self.report = Some(r);
            }
            Done::Upgraded(info) => self.say(false, format!("INSTALLED {}. EJECT AND REBOOT THE IPOD", info.version)),
            Done::Planned(p, imported) => {
                let scan = if p.is_empty() { "LIBRARY UP TO DATE" } else { "SCAN DONE" };
                match imported {
                    Ok(i) if i.plays + i.tracks > 0 => {
                        self.say(false, format!("{scan}. {} PLAYS, {} TRACK STATS BACKED UP", i.plays, i.tracks))
                    }
                    Ok(_) => self.say(false, scan),
                    Err(e) => self.say(true, format!("{scan}. HISTORY NOT SAVED: {e}")),
                }
                self.plan = Some(p);
            }
            Done::Synced { copied, deleted } => {
                self.plan = None;
                self.say(false, format!("SYNCED: {copied} COPIED, {deleted} DELETED. POD RESCANS AT BOOT"));
            }
            Done::Ejected => self.say(false, "EJECTED. SAFE TO UNPLUG"),
        }
    }

    fn flash(&mut self, install: bool) -> Task<Message> {
        let Some(d) = self.device() else { return Task::none() };
        let Some(disk) = d.disk.clone() else {
            self.say(true, "the iPod's disk device is unknown");
            return Task::none();
        };
        if !install {
            return self
                .start("REMOVING", move |_, _| Ok(Done::Flashed(flash::run_elevated(&disk, &Action::Uninstall)?)));
        }
        let Some(model) = self.current_model() else {
            self.say(true, "pick the iPod model first");
            return Task::none();
        };
        let local = self.boot_local.clone();
        let rel = self.latest().cloned();
        self.start("FLASHING", move |progress, cancel| {
            let path = match local {
                Some(p) => p,
                None => {
                    let rel = rel.ok_or_else(|| Error::Other("no release to download from".into()))?;
                    release::fetch(&rel, &release::bootloader_asset(model), progress, cancel)?
                }
            };
            // check the file before asking for the password
            let boot = Bootloader::parse(&std::fs::read(&path)?)?;
            if boot.model != model {
                return Err(Error::WrongModel { ipod: model, file: boot.model });
            }
            Ok(Done::Flashed(flash::run_elevated(&disk, &Action::Install(path))?))
        })
    }

    fn upgrade(&mut self) -> Task<Message> {
        let Some(d) = self.device() else { return Task::none() };
        let mount = d.mount.clone();
        let model = self.current_model();
        let local = self.fw_local.clone();
        let rel = self.latest().cloned();
        let opts = upgrade::Options { keep_settings: self.keep_settings, remove_rockbox: self.remove_rockbox };
        self.start("INSTALLING", move |progress, cancel| {
            let zip = match local {
                Some(p) => p,
                None => {
                    let rel = rel.ok_or_else(|| Error::Other("no release to download from".into()))?;
                    let model = model.ok_or_else(|| Error::Other("pick the iPod model first".into()))?;
                    release::fetch(&rel, &release::firmware_asset(model), progress, cancel)?
                }
            };
            Ok(Done::Upgraded(upgrade::install(&zip, &mount, model, opts, progress, cancel)?))
        })
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.job.is_some() {
            window::frames().map(Message::Frame)
        } else {
            time::every(Duration::from_secs(2)).map(|_| Message::Tick)
        }
    }

    // ------------------------------------------------------------ view

    fn view(&self) -> Element<'_, Message> {
        let body = row![
            container(self.device_panel()).style(theme::panel).width(270).height(Fill),
            container(column![self.keys(), self.deck(), self.screen()].spacing(0))
                .style(theme::panel)
                .width(Fill)
                .height(Fill),
        ]
        .spacing(12)
        .padding(12)
        .height(Fill);
        column![self.header(), canvas(Hatch).width(Fill).height(6), body, self.footer()].into()
    }

    fn header(&self) -> Element<'_, Message> {
        let chip = match (self.job.as_ref(), self.device()) {
            (Some(j), _) => j.label,
            (None, Some(_)) => "LINKED",
            (None, None) => "NO DEVICE",
        };
        container(
            row![
                canvas(Delta::default()).width(24).height(22),
                text("POD LINK").font(DECK_BOLD).size(20),
                text("bΔnst ⧉ design").font(MONO).size(12).color(theme::FAINT),
                space::horizontal(),
                container(text(chip).font(DECK_BOLD).size(11)).style(theme::chip).padding([3, 8]),
            ]
            .spacing(12)
            .align_y(Center),
        )
        .style(theme::bar)
        .padding([10, 14])
        .width(Fill)
        .into()
    }

    fn device_panel(&self) -> Element<'_, Message> {
        let blink = self.job.is_some() && (self.angle * 2.0) as i32 % 2 == 0;
        let mut col = column![
            heading("DEVICE"),
            canvas(Ipod { present: self.device().is_some(), blink: !blink }).width(Fill).height(130),
        ]
        .spacing(10)
        .padding(14);

        let Some(d) = self.device() else {
            return col
                .push(text("NO DEVICE").font(DECK_BOLD).size(16))
                .push(
                    text(
                        "Plug the iPod in. With pod installed it shows DATA LINK and mounts by itself. \
                         With the Apple firmware, put it in disk mode: hold MENU+SELECT to reset, then \
                         SELECT+PLAY at the Apple logo.",
                    )
                    .size(12)
                    .style(theme::mid),
                )
                .into();
        };

        if self.devices.len() > 1 {
            let labels: Vec<String> = self.devices.iter().map(|d| d.label.clone()).collect();
            col = col.push(pick_list(labels, Some(d.label.clone()), Message::Select).style(theme::picker).width(Fill));
        }
        let used = if d.total > 0 { 1.0 - d.free as f32 / d.total as f32 } else { 0.0 };
        col = col
            .push(readout("VOLUME", d.label.clone()))
            .push(readout("MODEL", d.model.map_or("unknown".into(), |m| m.to_string())))
            .push(readout("FORMAT", if d.is_fat() { "FAT32".into() } else { d.filesystem.to_uppercase() }))
            .push(readout("DISK", d.disk.clone().unwrap_or_else(|| "?".into())))
            .push(readout("FIRMWARE", d.firmware.clone().unwrap_or_else(|| "Apple".into())))
            .push(readout("FREE", format!("{} of {}", human_bytes(d.free), human_bytes(d.total))))
            .push(canvas(Meter { value: used }).width(Fill).height(8));
        if d.rockbox {
            col = col.push(text("Stock Rockbox found in /.rockbox").size(11).style(theme::mid));
        }
        col.push(space::vertical())
            .push(
                button(text("EJECT").font(DECK).size(12))
                    .style(theme::key)
                    .padding([6, 12])
                    .width(Fill)
                    .on_press_maybe(self.job.is_none().then_some(Message::Eject)),
            )
            .into()
    }

    fn keys(&self) -> Element<'_, Message> {
        let key = |t: Tab| {
            button(container(text(t.label()).font(DECK_BOLD).size(13)).center_x(Fill))
                .style(theme::deck_key(self.tab == t))
                .padding([10, 0])
                .width(Fill)
                .on_press_maybe(self.job.is_none().then_some(Message::Tab(t)))
        };
        row![key(Tab::Flash), key(Tab::Upgrade), key(Tab::Sync)].into()
    }

    fn deck(&self) -> Element<'_, Message> {
        let (tape, pos, xfer, item) = match &self.job {
            Some(j) if j.progress.total > 0 => {
                let f = j.progress.done as f32 / j.progress.total as f32;
                (
                    f,
                    format!("{:>3}%", (f * 100.0) as u32),
                    format!("{}/{}", human_bytes(j.progress.done), human_bytes(j.progress.total)),
                    j.progress.item.clone(),
                )
            }
            Some(j) => (0.5, "---".into(), "--".into(), j.label.to_string()),
            None => (0.0, "---".into(), "--".into(), String::new()),
        };
        let side = match self.tab {
            Tab::Flash => "A",
            Tab::Upgrade => "B",
            Tab::Sync => "C",
        };
        let hud = column![
            container(text(format!("SIDE {side}")).font(DECK_BOLD).size(11)).style(theme::bar).padding([2, 6]),
            column![label("POS"), text(pos).font(MONO_BOLD).size(22)],
            column![label("XFER"), text(xfer).size(12)],
        ]
        .spacing(10)
        .width(150);
        let cassette = canvas(Cassette { tape, angle: self.angle, active: self.job.is_some() }).width(Fill).height(190);
        column![
            row![cassette, hud].spacing(8).padding([8, 14]).align_y(Center),
            container(text(item).size(11).style(theme::mid)).padding([0, 14]).height(16),
        ]
        .into()
    }

    fn screen(&self) -> Element<'_, Message> {
        let content = match self.tab {
            Tab::Flash => self.flash_screen(),
            Tab::Upgrade => self.upgrade_screen(),
            Tab::Sync => self.sync_screen(),
        };
        stack![
            canvas(Dots).width(Fill).height(Fill),
            scrollable(container(content).padding([12, 16])).style(theme::scroller).height(Fill),
        ]
        .into()
    }

    /// A problem that blocks the current screen, if any.
    fn blocker(&self) -> Option<Element<'_, Message>> {
        let d = self.device()?;
        (!d.is_fat()).then(|| notice(Error::MacFormatted.to_string()))
    }

    fn source_row(&self, firmware: bool) -> Element<'_, Message> {
        let local = if firmware { &self.fw_local } else { &self.boot_local };
        let release = match &self.release {
            Remote::Loading => "CHECKING...".to_string(),
            Remote::Ready(Some(r)) => format!("RELEASE {}", r.tag_name),
            Remote::Ready(None) => "NO RELEASE YET".into(),
            Remote::Failed => "OFFLINE".into(),
        };
        let mut r = row![
            label("SOURCE").width(80),
            button(text(release).font(DECK).size(11))
                .style(theme::deck_key(local.is_none()))
                .padding([5, 10])
                .on_press(Message::UseRelease { firmware }),
            button(text("LOCAL FILE...").font(DECK).size(11))
                .style(theme::deck_key(local.is_some()))
                .padding([5, 10])
                .on_press(Message::PickLocal { firmware }),
        ]
        .spacing(8)
        .align_y(Center);
        if let Some(p) = local {
            r = r.push(
                text(p.file_name().unwrap_or_default().to_string_lossy().into_owned()).size(11).style(theme::mid),
            );
        }
        r.into()
    }

    fn model_row(&self) -> Element<'_, Message> {
        row![
            label("MODEL").width(80),
            pick_list(Model::ALL, self.current_model(), Message::Model)
                .placeholder("pick the model")
                .style(theme::picker)
                .text_size(13),
        ]
        .spacing(8)
        .align_y(Center)
        .into()
    }

    fn can_download(&self, local: &Option<PathBuf>) -> bool {
        local.is_some() || self.latest().is_some()
    }

    fn flash_screen(&self) -> Element<'_, Message> {
        let mut col = column![
            heading("BOOTLOADER"),
            text(
                "Writes the pod bootloader in front of the Apple firmware, in the iPod's firmware \
                 partition. The Apple firmware stays: hold MENU at power on to boot it. Once per iPod; \
                 your password is asked for raw disk access.",
            )
            .size(12)
            .style(theme::mid),
            self.source_row(false),
            self.model_row(),
        ]
        .spacing(12);
        let d = self.device();
        if let Some(b) = self.blocker() {
            col = col.push(b);
        } else if d.is_some_and(|d| !d.has_pod()) {
            col = col.push(notice("Install the firmware (UPGRADE) first: the bootloader starts /.pod/pod.ipod."));
        }
        let ok = d.is_some_and(|d| d.is_fat() && d.disk.is_some()) && self.job.is_none();
        // without /.pod/pod.ipod the bootloader has nothing to start
        let go = ok
            && d.is_some_and(Device::has_pod)
            && self.current_model().is_some()
            && self.can_download(&self.boot_local);
        col = col.push(
            row![
                button(text("FLASH BOOTLOADER").font(DECK_BOLD).size(13))
                    .style(theme::primary)
                    .padding([10, 18])
                    .on_press_maybe(go.then_some(Message::Flash)),
                button(text("REMOVE").font(DECK).size(12))
                    .style(theme::key)
                    .padding([10, 14])
                    .on_press_maybe(ok.then_some(Message::Unflash)),
            ]
            .spacing(10),
        );
        if let Some(r) = &self.report {
            col = col.push(
                container(column![
                    readout("MODEL", r.model.clone()),
                    readout("BEFORE", r.before.clone()),
                    readout("AFTER", r.after.clone()),
                ])
                .style(theme::well)
                .padding(10),
            );
        }
        col.into()
    }

    fn upgrade_screen(&self) -> Element<'_, Message> {
        let d = self.device();
        let available = match (&self.fw_local, self.latest()) {
            (Some(p), _) => upgrade::inspect(p).map(|i| i.version).unwrap_or_else(|e| e.to_string()),
            (None, Some(r)) => r.version(),
            (None, None) => "--".into(),
        };
        let mut col = column![
            heading("FIRMWARE"),
            text(
                "Copies pod into /.pod on the iPod. Music and the library database stay. Then eject \
                 and reboot the iPod.",
            )
            .size(12)
            .style(theme::mid),
            row![
                readout("INSTALLED", d.and_then(|d| d.firmware.clone()).unwrap_or_else(|| "none".into())),
                readout("AVAILABLE", available),
            ]
            .spacing(24),
            self.source_row(true),
            self.model_row(),
            checkbox(self.keep_settings)
                .label("Keep my settings (config.cfg)")
                .on_toggle(Message::KeepSettings)
                .style(theme::check)
                .size(14)
                .text_size(13),
        ]
        .spacing(12);
        if d.is_some_and(|d| d.rockbox) {
            col = col.push(
                checkbox(self.remove_rockbox)
                    .label("Remove stock Rockbox (/.rockbox)")
                    .on_toggle(Message::RemoveRockbox)
                    .style(theme::check)
                    .size(14)
                    .text_size(13),
            );
        }
        if let Some(b) = self.blocker() {
            col = col.push(b);
        }
        let model_ok = self.fw_local.is_some() || self.current_model().is_some();
        let go = d.is_some_and(Device::is_fat) && self.job.is_none() && model_ok && self.can_download(&self.fw_local);
        col.push(
            button(text("INSTALL FIRMWARE").font(DECK_BOLD).size(13))
                .style(theme::primary)
                .padding([10, 18])
                .on_press_maybe(go.then_some(Message::Upgrade)),
        )
        .into()
    }

    fn sync_screen(&self) -> Element<'_, Message> {
        let folder = self.folder.as_ref().map_or("none".into(), |p| p.display().to_string());
        let mode_key = |m: Mode, l: &'static str| {
            button(text(l).font(DECK).size(11))
                .style(theme::deck_key(self.mode == m))
                .padding([5, 10])
                .on_press(Message::SyncMode(m))
        };
        let mut col = column![
            heading("LIBRARY"),
            text("Copies music from a folder into /Music. pod adds it to the library at the next boot.")
                .size(12)
                .style(theme::mid),
            row![
                label("FOLDER").width(80),
                button(text("CHOOSE...").font(DECK).size(11))
                    .style(theme::key)
                    .padding([5, 10])
                    .on_press(Message::PickFolder),
                text(folder).size(12),
            ]
            .spacing(8)
            .align_y(Center),
            row![
                label("MODE").width(80),
                mode_key(Mode::Update, "ADD + UPDATE"),
                mode_key(Mode::Mirror, "MIRROR"),
                text(match self.mode {
                    Mode::Update => "nothing is deleted",
                    Mode::Mirror => "music not in the folder is deleted",
                })
                .size(11)
                .style(theme::mid),
            ]
            .spacing(8)
            .align_y(Center),
        ]
        .spacing(12);
        if let Some(b) = self.blocker() {
            col = col.push(b);
        }
        let idle = self.job.is_none() && self.device().is_some();
        let has_plan = self.plan.as_ref().is_some_and(|p| !p.is_empty());
        if let Some(p) = &self.plan {
            let growth = p.growth();
            let free = self.device().map_or(0, |d| d.free);
            let after = (free as i64 - growth).max(0) as u64;
            col = col.push(
                container(
                    row![
                        readout("COPY", format!("{} ({})", p.copies.len(), human_bytes(p.copy_bytes()))),
                        readout("DELETE", p.deletes.len().to_string()),
                        readout("SAME", p.unchanged.to_string()),
                        readout("FREE AFTER", human_bytes(after)),
                    ]
                    .spacing(24),
                )
                .style(theme::well)
                .padding(10),
            );
        }
        col.push(
            row![
                button(text("SCAN").font(DECK).size(12))
                    .style(theme::key)
                    .padding([10, 14])
                    .on_press_maybe((idle && self.folder.is_some()).then_some(Message::Scan)),
                button(text("SYNC").font(DECK_BOLD).size(13))
                    .style(theme::primary)
                    .padding([10, 18])
                    .on_press_maybe((idle && has_plan).then_some(Message::Sync)),
            ]
            .spacing(10),
        )
        .into()
    }

    fn footer(&self) -> Element<'_, Message> {
        let (err, msg) = &self.status;
        let value = self
            .job
            .as_ref()
            .map_or(0.0, |j| if j.progress.total > 0 { j.progress.done as f32 / j.progress.total as f32 } else { 0.0 });
        let mut r = row![
            text(if *err { "ERR" } else { "SYS" }).font(DECK_BOLD).size(11),
            text(msg.to_uppercase()).size(12),
            space::horizontal(),
            canvas(Meter { value }).width(220).height(8),
        ]
        .spacing(10)
        .align_y(Center);
        if self.job.is_some() {
            r = r.push(
                button(text("CANCEL").font(DECK).size(11)).style(theme::key).padding([3, 8]).on_press(Message::Cancel),
            );
        }
        container(r).style(if *err { theme::bar } else { theme::panel }).padding([8, 14]).width(Fill).into()
    }
}

fn heading(s: &str) -> iced::widget::Text<'_, Theme> {
    text(s).font(DECK_BOLD).size(14)
}

fn label(s: &str) -> iced::widget::Text<'_, Theme> {
    text(s).font(DECK).size(10).style(theme::mid)
}

fn readout<'a>(l: &'a str, v: String) -> Element<'a, Message> {
    column![label(l), text(v).size(13)].spacing(2).into()
}

fn notice(s: impl Into<String>) -> Element<'static, Message> {
    container(text(s.into()).size(12)).style(theme::notice).padding(10).width(Fill).into()
}
