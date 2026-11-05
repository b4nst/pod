//! The firmware's look on the desktop: the LCD's four levels of ink on the
//! backlit panel, Orbitron (the firmware's "Deck") for chrome, JetBrains Mono
//! for data. Square corners, 1px rules, inverse video for what is selected.

use iced::font::{Family, Weight};
use iced::widget::{button, checkbox, container, pick_list, scrollable, text};
use iced::{Background, Border, Color, Font, Shadow, Theme, color, theme};

pub const PAPER: Color = color!(0xdfd8ff);
pub const FAINT: Color = color!(0x9590aa);
pub const MID: Color = color!(0x4b4855);
pub const INK: Color = color!(0x000000);
/// The dot grid: just visible on the panel.
pub const DOT: Color = color!(0xb9b2d8);
/// The panel a step darker, for wells.
pub const SHADE: Color = color!(0xd2caf4);

pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/Orbitron-Medium.ttf"),
    include_bytes!("../../assets/fonts/Orbitron-Bold.ttf"),
];

pub const MONO: Font = Font::with_name("JetBrains Mono");
pub const MONO_BOLD: Font = Font { weight: Weight::Bold, ..MONO };
pub const DECK: Font = Font { family: Family::Name("Orbitron"), weight: Weight::Medium, ..Font::DEFAULT };
pub const DECK_BOLD: Font = Font { weight: Weight::Bold, ..DECK };

pub fn theme() -> Theme {
    Theme::custom(
        "pod",
        theme::Palette { background: PAPER, text: INK, primary: INK, success: INK, warning: MID, danger: INK },
    )
}

fn rule(c: Color) -> Border {
    Border { color: c, width: 1.0, radius: 0.0.into() }
}

/// Outlined key; inverse when hovered or pressed.
pub fn key(_: &Theme, status: button::Status) -> button::Style {
    let (bg, fg, edge) = match status {
        button::Status::Active => (None, INK, INK),
        button::Status::Hovered | button::Status::Pressed => (Some(INK), PAPER, INK),
        button::Status::Disabled => (None, FAINT, FAINT),
    };
    button::Style {
        background: bg.map(Background::Color),
        text_color: fg,
        border: rule(edge),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// The action that matters on a screen: filled ink.
pub fn primary(t: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Active => {
            button::Style { background: Some(INK.into()), text_color: PAPER, border: rule(INK), ..key(t, status) }
        }
        button::Status::Hovered | button::Status::Pressed => {
            button::Style { background: Some(MID.into()), text_color: PAPER, border: rule(INK), ..key(t, status) }
        }
        button::Status::Disabled => key(t, status),
    }
}

/// A tape deck's piano key: latched down when it is the current screen.
pub fn deck_key(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |t, status| {
        if active {
            button::Style { background: Some(INK.into()), text_color: PAPER, border: rule(INK), ..key(t, status) }
        } else {
            key(t, status)
        }
    }
}

pub fn panel(_: &Theme) -> container::Style {
    container::Style { border: rule(INK), background: Some(PAPER.into()), ..container::Style::default() }
}

pub fn well(_: &Theme) -> container::Style {
    container::Style { border: rule(FAINT), background: Some(SHADE.into()), ..container::Style::default() }
}

/// Inverted strip, as the firmware's title bar.
pub fn bar(_: &Theme) -> container::Style {
    container::Style { background: Some(INK.into()), text_color: Some(PAPER), ..container::Style::default() }
}

/// Status chip in the title bar.
pub fn chip(_: &Theme) -> container::Style {
    container::Style { background: Some(PAPER.into()), text_color: Some(INK), ..container::Style::default() }
}

pub fn notice(_: &Theme) -> container::Style {
    container::Style {
        border: Border { width: 1.0, color: INK, radius: 0.0.into() },
        background: Some(SHADE.into()),
        ..container::Style::default()
    }
}

pub fn mid(_: &Theme) -> text::Style {
    text::Style { color: Some(MID) }
}

pub fn check(_: &Theme, status: checkbox::Status) -> checkbox::Style {
    let (checked, disabled) = match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Hovered { is_checked } => (is_checked, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, true),
    };
    let edge = if disabled { FAINT } else { INK };
    checkbox::Style {
        background: if checked { edge.into() } else { PAPER.into() },
        icon_color: PAPER,
        border: rule(edge),
        text_color: Some(if disabled { FAINT } else { INK }),
    }
}

pub fn picker(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: INK,
        placeholder_color: MID,
        handle_color: INK,
        background: if matches!(status, pick_list::Status::Hovered) { SHADE.into() } else { PAPER.into() },
        border: rule(INK),
    }
}

pub fn scroller(t: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut s = scrollable::default(t, status);
    s.vertical_rail.scroller.background = MID.into();
    s.vertical_rail.scroller.border = Border::default();
    s.vertical_rail.background = None;
    s
}
