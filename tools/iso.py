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


def shade_window(cas, c, tape, angle):
    """Packs, hubs and spokes inside the window.

    tape: 0 at the start of the track (all tape on the left reel) .. 1.
    angle: hub rotation in degrees.
    """
    packs = ((HUB_L, PACK_MIN + (PACK_MAX - PACK_MIN) * (1 - tape)),
             (HUB_R, PACK_MIN + (PACK_MAX - PACK_MIN) * tape))
    bx0, by0, bx1, by1 = cas.window_box()
    for v in range(by0, by1):
        for u in range(bx0, bx1):
            x, y = cas.unproject(u + 0.5, v + 0.5)
            if not in_window(x, y):
                continue
            col = FAINT
            for hx, pr in packs:
                dx, dy = x - hx, y - HUB_Y
                r = math.hypot(dx, dy)
                if r <= pr:
                    col = MID
                if r <= 6.5:
                    col = INK if r > 4.6 else PAPER
                    if r <= 4.6:
                        for i in range(3):
                            a = math.radians(angle + i * 120)
                            along = dx * math.cos(a) + dy * math.sin(a)
                            perp = -dx * math.sin(a) + dy * math.cos(a)
                            if along > 0 and abs(perp) < 1.1:
                                col = INK
                        if r < 1.5:
                            col = INK
            c.set(u, v, col)


def draw(cas, c, tape=0.5, angle=0.0):
    p = cas.p
    # faces: front and right end shaded for depth, top face blank
    c.fill_poly([p(0, DEP, 0), p(LEN, DEP, 0), p(LEN, DEP), p(0, DEP)], FAINT)
    c.fill_poly([p(LEN, 0, 0), p(LEN, DEP, 0), p(LEN, DEP), p(LEN, 0)], MID)
    c.fill_poly([p(0, 0), p(LEN, 0), p(LEN, DEP), p(0, DEP)], PAPER)

    # tape head openings on the front face
    for x0 in (30, 46, 62):
        c.fill_poly([p(x0, DEP, 3), p(x0 + 8, DEP, 3), p(x0 + 8, DEP, 8),
                     p(x0, DEP, 8)], INK)

    shade_window(cas, c, tape, angle)

    # label, its colour band and the bottom ridge
    cas.poly(c, [(5, 3, HGT), (95, 3, HGT), (95, 44, HGT), (5, 44, HGT)], MID)
    cas.line(c, (7, 39, HGT), (93, 39, HGT), MID)
    cas.line(c, (7, 41, HGT), (93, 41, HGT), MID)
    cas.poly(c, [(18, DEP, HGT), (25, 51, HGT), (75, 51, HGT), (82, DEP, HGT)],
             INK, closed=False)
    for hx in (36, 64):
        u, v = cas.ip(hx, 58)
        c.rect(u, v, 2, 1, INK)
    for sx, sy in ((4, 4), (96, 4), (4, 60), (96, 60)):
        u, v = cas.ip(sx, sy)
        c.set(u, v, MID)

    cas.poly(c, window_outline(), INK)

    # silhouette and the visible edges
    cas.poly(c, [(0, 0, HGT), (LEN, 0, HGT), (LEN, DEP, HGT), (0, DEP, HGT)], INK)
    for a, b in (((0, DEP, HGT), (0, DEP, 0)), ((LEN, DEP, HGT), (LEN, DEP, 0)),
                 ((LEN, 0, HGT), (LEN, 0, 0)), ((0, DEP, 0), (LEN, DEP, 0)),
                 ((LEN, 0, 0), (LEN, DEP, 0))):
        cas.line(c, a, b, INK)
