"""A compact cassette drawn in axonometric projection, HUD wireframe style.

Cassette space is in millimetres: x along the length (0..100), y from the
back edge to the tape opening (0..64), z up (0..12). The top face is shaded
per pixel through the inverse projection, so reels, packs and spokes land in
perspective; edges are then drawn as 1px lines on top.
"""
import math

from canvas import PAPER, FAINT, MID, INK

LEN, DEP, HGT = 100.0, 64.0, 12.0
HUB_L, HUB_R, HUB_Y = 30.0, 70.0, 25.0
WIN = (24.0, 76.0, 16.0, 34.0, 7.0)   # x0, x1, y0, y1, corner radius
PACK_MIN, PACK_MAX = 6.0, 18.0

EX = (0.92, 0.22)    # +x on screen: right, slightly down
EY = (-0.36, 0.52)   # +y on screen: towards the viewer, left and down
EZ = 0.75            # height is foreshortened too: seen from well above


class Cassette:
    def __init__(self, ox, oy, scale):
        self.ox, self.oy, self.s = ox, oy, scale
        det = EX[0] * EY[1] - EY[0] * EX[1]
        self.inv = (EY[1] / det, -EY[0] / det, -EX[1] / det, EX[0] / det)

    def p(self, x, y, z=HGT):
        s = self.s
        return (self.ox + s * (x * EX[0] + y * EY[0]),
                self.oy + s * (x * EX[1] + y * EY[1] - z * EZ))

    def unproject(self, u, v, z=HGT):
        a, b, c, d = self.inv
        du = (u - self.ox) / self.s
        dv = (v - self.oy) / self.s + z * EZ
        return a * du + b * dv, c * du + d * dv

    def ip(self, x, y, z=HGT):
        u, v = self.p(x, y, z)
        return round(u), round(v)

    def line(self, c, a, b, v):
        c.line(*self.ip(*a), *self.ip(*b), v)

    def poly(self, c, pts, v, closed=True):
        for i in range(len(pts) - (0 if closed else 1)):
            self.line(c, pts[i], pts[(i + 1) % len(pts)], v)

    def bbox(self, x0, x1, y0, y1, z=HGT):
        pts = [self.p(x, y, z) for x in (x0, x1) for y in (y0, y1)]
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        return (int(min(xs)) - 1, int(min(ys)) - 1,
                int(max(xs)) + 2, int(max(ys)) + 2)

    def window_box(self):
        """Screen rectangle covering everything that animates."""
        x0, x1, y0, y1, _ = WIN
        return self.bbox(x0, x1, y0, y1)


def in_window(x, y):
    x0, x1, y0, y1, r = WIN
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return math.hypot(x - cx, y - cy) <= r


def window_outline(steps=40):
    x0, x1, y0, y1, r = WIN
    pts = []
    for cx, cy, a0 in ((x1 - r, y0 + r, -90), (x1 - r, y1 - r, 0),
                       (x0 + r, y1 - r, 90), (x0 + r, y0 + r, 180)):
        for i in range(steps // 4 + 1):
            a = math.radians(a0 + 90 * i / (steps // 4))
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a), HGT))
    return pts


def dotted(cas, c, a, b, v, gap=2):
    (u0, v0), (u1, v1) = cas.p(*a), cas.p(*b)
    n = max(1, int(math.hypot(u1 - u0, v1 - v0) / gap))
    for i in range(n + 1):
        t = i / n
        c.set(round(u0 + (u1 - u0) * t), round(v0 + (v1 - v0) * t), v)


def circle(cas, c, cx, cy, r, v, clip=None, steps=48, dots=False):
    """Circle on the top face, projected to an ellipse."""
    pts = [(cx + r * math.cos(2 * math.pi * i / steps),
            cy + r * math.sin(2 * math.pi * i / steps)) for i in range(steps + 1)]
    for i, ((x0, y0), (x1, y1)) in enumerate(zip(pts, pts[1:])):
        if clip and not (clip(x0, y0) and clip(x1, y1)):
            continue
        if dots:
            if i % 2 == 0:
                c.set(*cas.ip(x0, y0), v)
            continue
        cas.line(c, (x0, y0, HGT), (x1, y1, HGT), v)


def reels(cas, c, tape, angle):
    """Tape pack outlines and spoked hubs.

    tape: 0 at the start of the track (all tape on the left reel) .. 1.
    angle: hub rotation in degrees.
    """
    for hx, pack in ((HUB_L, 1 - tape), (HUB_R, tape)):
        circle(cas, c, hx, HUB_Y, PACK_MIN + (PACK_MAX - PACK_MIN) * pack,
               INK, clip=in_window, steps=72, dots=True)
        circle(cas, c, hx, HUB_Y, 5.5, INK)
        for i in range(3):
            a = math.radians(angle + i * 120)
            cas.line(c, (hx + 2.0 * math.cos(a), HUB_Y + 2.0 * math.sin(a), HGT),
                     (hx + 5.5 * math.cos(a), HUB_Y + 5.5 * math.sin(a), HGT), INK)


def clear_window(cas, c):
    """Wipe the window region back to the bare panel inside the outline."""
    bx0, by0, bx1, by1 = cas.window_box()
    for v in range(by0, by1):
        for u in range(bx0, bx1):
            if in_window(*cas.unproject(u + 0.5, v + 0.5)):
                c.set(u, v, PAPER)


def draw(cas, c, tape=0.5, angle=0.0):
    """Wireframe: visible edges solid, hidden ones dotted, no fills."""
    L, D, H = LEN, DEP, HGT
    for a, b in (((0, 0, 0), (LEN, 0, 0)), ((0, 0, 0), (0, DEP, 0)),
                 ((0, 0, 0), (0, 0, HGT))):
        dotted(cas, c, a, b, MID)

    clear_window(cas, c)
    reels(cas, c, tape, angle)
    cas.poly(c, window_outline(), INK)

    # label and the bottom ridge on the top face
    cas.poly(c, [(5, 3, H), (95, 3, H), (95, 44, H), (5, 44, H)], INK)
    cas.poly(c, [(18, D, H), (25, 51, H), (75, 51, H), (82, D, H)], INK,
             closed=False)
    # tape head openings on the front face
    for x0 in (30, 46, 62):
        cas.poly(c, [(x0, D, 3), (x0 + 8, D, 3), (x0 + 8, D, 8), (x0, D, 8)], INK)

    # visible box edges
    cas.poly(c, [(0, 0, H), (L, 0, H), (L, D, H), (0, D, H)], INK)
    for a, b in (((0, D, H), (0, D, 0)), ((L, D, H), (L, D, 0)),
                 ((L, 0, H), (L, 0, 0)), ((0, D, 0), (L, D, 0)),
                 ((L, 0, 0), (L, D, 0))):
        cas.line(c, a, b, INK)
