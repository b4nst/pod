//! Canvas drawings in the firmware's HUD wireframe style: the cassette from
//! the player screen (`firmware/tools/iso.py`), the iPod from the USB screen,
//! segmented meters and the hatched rule under the title bar.

use std::f32::consts::TAU;

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::theme::{DOT, FAINT, INK, MID, PAPER};

fn line(c: Color, w: f32) -> Stroke<'static> {
    Stroke::default().with_color(c).with_width(w)
}

fn dashed(c: Color, w: f32, segs: &'static [f32]) -> Stroke<'static> {
    Stroke { line_dash: LineDash { segments: segs, offset: 0 }, ..line(c, w) }
}

// ---------------------------------------------------------------- cassette

// cassette space in mm: x along the length, y from the back edge to the
// tape opening, z up; same constants as iso.py
const LEN: f32 = 100.0;
const DEP: f32 = 64.0;
const HGT: f32 = 12.0;
const HUB_L: f32 = 30.0;
const HUB_R: f32 = 70.0;
const HUB_Y: f32 = 25.0;
const WIN: (f32, f32, f32, f32, f32) = (24.0, 76.0, 16.0, 34.0, 7.0);
const PACK_MIN: f32 = 6.0;
const PACK_MAX: f32 = 18.0;
const EX: (f32, f32) = (0.92, 0.22);
const EY: (f32, f32) = (-0.36, 0.52);
const EZ: f32 = 0.75;

/// The 3D wireframe cassette. `tape` moves the tape from the left reel to
/// the right one (0..1), `angle` turns the hubs (radians).
pub struct Cassette {
    pub tape: f32,
    pub angle: f32,
    pub active: bool,
}

struct Proj {
    ox: f32,
    oy: f32,
    s: f32,
}

impl Proj {
    fn raw(x: f32, y: f32, z: f32) -> (f32, f32) {
        (x * EX.0 + y * EY.0, x * EX.1 + y * EY.1 - z * EZ)
    }

    /// Scale and centre the cassette in `size`.
    fn fit(size: Size) -> Proj {
        let mut lo = (f32::MAX, f32::MAX);
        let mut hi = (f32::MIN, f32::MIN);
        for x in [0.0, LEN] {
            for y in [0.0, DEP] {
                for z in [0.0, HGT] {
                    let (u, v) = Self::raw(x, y, z);
                    lo = (lo.0.min(u), lo.1.min(v));
                    hi = (hi.0.max(u), hi.1.max(v));
                }
            }
        }
        let pad = 8.0;
        let s = ((size.width - 2.0 * pad) / (hi.0 - lo.0)).min((size.height - 2.0 * pad) / (hi.1 - lo.1));
        Proj {
            s,
            ox: (size.width - s * (hi.0 - lo.0)) / 2.0 - s * lo.0,
            oy: (size.height - s * (hi.1 - lo.1)) / 2.0 - s * lo.1,
        }
    }

    fn p(&self, x: f32, y: f32, z: f32) -> Point {
        let (u, v) = Self::raw(x, y, z);
        Point::new(self.ox + self.s * u, self.oy + self.s * v)
    }

    fn poly(&self, pts: &[(f32, f32, f32)], closed: bool) -> Path {
        Path::new(|b| {
            b.move_to(self.p(pts[0].0, pts[0].1, pts[0].2));
            for &(x, y, z) in &pts[1..] {
                b.line_to(self.p(x, y, z));
            }
            if closed {
                b.close();
            }
        })
    }

    /// A circle on the top face, as the projected ellipse; segments outside
    /// `clip` are left out.
    fn circle(&self, cx: f32, cy: f32, r: f32, clip: Option<fn(f32, f32) -> bool>) -> Path {
        let n = 72;
        Path::new(|b| {
            let mut pen = false;
            for i in 0..=n {
                let a = TAU * i as f32 / n as f32;
                let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
                if clip.is_some_and(|f| !f(x, y)) {
                    pen = false;
                    continue;
                }
                let pt = self.p(x, y, HGT);
                if pen {
                    b.line_to(pt)
                } else {
                    b.move_to(pt)
                }
                pen = true;
            }
        })
    }
}

fn in_window(x: f32, y: f32) -> bool {
    let (x0, x1, y0, y1, r) = WIN;
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    (x - cx).hypot(y - cy) <= r
}

fn window_outline() -> Vec<(f32, f32, f32)> {
    let (x0, x1, y0, y1, r) = WIN;
    let mut pts = Vec::new();
    for (cx, cy, a0) in
        [(x1 - r, y0 + r, -90.0f32), (x1 - r, y1 - r, 0.0), (x0 + r, y1 - r, 90.0), (x0 + r, y0 + r, 180.0)]
    {
        for i in 0..=10 {
            let a = (a0 + 9.0 * i as f32).to_radians();
            pts.push((cx + r * a.cos(), cy + r * a.sin(), HGT));
        }
    }
    pts
}

impl<M> canvas::Program<M> for Cassette {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let p = Proj::fit(bounds.size());
        let w = (p.s * 0.35).clamp(1.0, 2.0);
        let ink = line(INK, w);

        // hidden edges, dotted
        for (a, b) in [((0., 0., 0.), (LEN, 0., 0.)), ((0., 0., 0.), (0., DEP, 0.)), ((0., 0., 0.), (0., 0., HGT))] {
            f.stroke(&p.poly(&[a, b], false), dashed(MID, 1.0, &[2.0, 3.0]));
        }

        // reels: dotted tape packs seen through the window, spoked hubs
        let tape = self.tape.clamp(0.0, 1.0);
        for (hx, pack, dir) in [(HUB_L, 1.0 - tape, 1.0), (HUB_R, tape, 1.0f32)] {
            let r = PACK_MIN + (PACK_MAX - PACK_MIN) * pack;
            f.stroke(&p.circle(hx, HUB_Y, r, Some(in_window)), dashed(INK, w, &[1.5, 2.5]));
            f.stroke(&p.circle(hx, HUB_Y, 5.5, None), ink);
            for i in 0..3 {
                let a = dir * self.angle + i as f32 * TAU / 3.0;
                let (c, s) = (a.cos(), a.sin());
                f.stroke(
                    &p.poly(&[(hx + 2.0 * c, HUB_Y + 2.0 * s, HGT), (hx + 5.5 * c, HUB_Y + 5.5 * s, HGT)], false),
                    ink,
                );
            }
        }
        f.stroke(&p.poly(&window_outline(), true), ink);

        // label, bottom ridge, tape head openings
        let h = HGT;
        f.stroke(&p.poly(&[(5., 3., h), (95., 3., h), (95., 44., h), (5., 44., h)], true), ink);
        f.stroke(&p.poly(&[(18., DEP, h), (25., 51., h), (75., 51., h), (82., DEP, h)], false), ink);
        for x0 in [30.0, 46.0, 62.0] {
            f.stroke(&p.poly(&[(x0, DEP, 3.), (x0 + 8., DEP, 3.), (x0 + 8., DEP, 8.), (x0, DEP, 8.)], true), ink);
        }
        // label stripes, as on a real C60
        for (i, y) in [8.0, 11.0].into_iter().enumerate() {
            let c = if self.active && i == 0 { INK } else { MID };
            f.stroke(&p.poly(&[(9., y, h), (91., y, h)], false), line(c, w));
        }

        // visible box edges
        f.stroke(&p.poly(&[(0., 0., h), (LEN, 0., h), (LEN, DEP, h), (0., DEP, h)], true), ink);
        for (a, b) in [
            ((0., DEP, h), (0., DEP, 0.)),
            ((LEN, DEP, h), (LEN, DEP, 0.)),
            ((LEN, 0., h), (LEN, 0., 0.)),
            ((0., DEP, 0.), (LEN, DEP, 0.)),
            ((LEN, 0., 0.), (LEN, DEP, 0.)),
        ] {
            f.stroke(&p.poly(&[a, b], false), ink);
        }

        crosshairs(&mut f, bounds.size());
        vec![f.into_geometry()]
    }
}

/// `+` marks in the corners, as on the player screen.
fn crosshairs(f: &mut Frame, s: Size) {
    let r = 5.0;
    for (x, y) in [(r + 1.0, s.height - r - 1.0), (s.width - r - 1.0, s.height - r - 1.0)] {
        f.stroke(&Path::line(Point::new(x - r, y), Point::new(x + r, y)), line(INK, 1.0));
        f.stroke(&Path::line(Point::new(x, y - r), Point::new(x, y + r)), line(INK, 1.0));
    }
}

// ---------------------------------------------------------------- iPod

/// The iPod mini from the USB screen: body, screen, click wheel.
pub struct Ipod {
    pub present: bool,
    pub blink: bool,
}

impl<M> canvas::Program<M> for Ipod {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        // 22x34 in the firmware drawing
        let k = (bounds.height / 34.0).min(bounds.width / 22.0);
        let o = Vector::new((bounds.width - 22.0 * k) / 2.0, (bounds.height - 34.0 * k) / 2.0);
        let at = |x: f32, y: f32| Point::new(o.x + x * k, o.y + y * k);
        let body = if self.present { line(INK, 2.0) } else { dashed(FAINT, 1.5, &[4.0, 4.0]) };
        let thin = if self.present { line(MID, 1.0) } else { dashed(FAINT, 1.0, &[3.0, 3.0]) };

        f.stroke(&Path::rounded_rectangle(at(0.5, 0.5), Size::new(21.0 * k, 33.0 * k), (3.0 * k).into()), body);
        f.stroke(&Path::rectangle(at(4.0, 4.0), Size::new(14.0 * k, 11.0 * k)), thin);
        f.stroke(&Path::circle(at(11.0, 24.0), 6.0 * k), body);
        f.fill(&Path::circle(at(11.0, 24.0), 2.0 * k), if self.present { MID } else { FAINT });
        if self.present {
            // the Δ splash on its screen
            let c = at(11.0, 9.5);
            let r = 3.2 * k;
            let tri = Path::new(|b| {
                b.move_to(Point::new(c.x, c.y - r));
                b.line_to(Point::new(c.x + r * 0.95, c.y + r * 0.7));
                b.line_to(Point::new(c.x - r * 0.95, c.y + r * 0.7));
                b.close();
            });
            f.stroke(&tri, line(if self.blink { INK } else { MID }, 1.5));
        }
        vec![f.into_geometry()]
    }
}

// ---------------------------------------------------------------- bits

/// Segmented bar meter: ink up to `value`, faint after.
pub struct Meter {
    pub value: f32,
}

impl<M> canvas::Program<M> for Meter {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (seg, gap) = (4.0, 2.0);
        let n = ((bounds.width + gap) / (seg + gap)).floor().max(1.0) as usize;
        let lit = (self.value.clamp(0.0, 1.0) * n as f32).round() as usize;
        for i in 0..n {
            let c = if i < lit { INK } else { FAINT };
            f.fill_rectangle(Point::new(i as f32 * (seg + gap), 0.0), Size::new(seg, bounds.height), c);
        }
        vec![f.into_geometry()]
    }
}

/// Diagonal hatching, the rule under the firmware's title bar.
pub struct Hatch;

impl<M> canvas::Program<M> for Hatch {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let h = bounds.height;
        let mut x = -h;
        while x < bounds.width {
            f.stroke(&Path::line(Point::new(x, h), Point::new(x + h, 0.0)), line(INK, 1.0));
            x += 4.0;
        }
        vec![f.into_geometry()]
    }
}

/// The Δ mark.
pub struct Delta {
    pub color: Color,
}

impl Default for Delta {
    fn default() -> Self {
        Delta { color: PAPER }
    }
}

impl<M> canvas::Program<M> for Delta {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let sw = (h / 7.0).max(2.0);
        let tri = Path::new(|b| {
            b.move_to(Point::new(w / 2.0, sw));
            b.line_to(Point::new(w - sw, h - sw / 1.5));
            b.line_to(Point::new(sw, h - sw / 1.5));
            b.close();
        });
        f.stroke(&tri, Stroke { line_join: canvas::LineJoin::Miter, ..line(self.color, sw) });
        vec![f.into_geometry()]
    }
}

/// Dot grid background, as behind the EQ plot and the USB screen.
pub struct Dots;

impl<M> canvas::Program<M> for Dots {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let step = 16.0;
        let mut y = step / 2.0;
        while y < bounds.height {
            let mut x = step / 2.0;
            while x < bounds.width {
                f.fill_rectangle(Point::new(x, y), Size::new(1.0, 1.0), DOT);
                x += step;
            }
            y += step;
        }
        vec![f.into_geometry()]
    }
}
