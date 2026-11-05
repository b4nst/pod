"""Tiny 2-bit greyscale canvas: BDF text, primitives, 24-bit BMP output.

Levels are ink: 0 is the bare panel, 3 is full ink. The mini's backlit LCD
looks best with a light background, so ink is drawn dark.
"""
import math
import struct

PAPER, FAINT, MID, INK = 0, 1, 2, 3
RGB = {0: (255, 255, 255), 1: (170, 170, 170), 2: (85, 85, 85), 3: (0, 0, 0)}


def lcd(level):
    """Ink level as a Rockbox greyscale colour (0 black .. 3 white)."""
    return 3 - level


class BDF:
    def __init__(self, path):
        self.glyphs = {}
        cur, bitmap = None, None
        for line in open(path, encoding="latin-1"):
            k = line.split()
            if not k:
                continue
            if k[0] == "FONT_ASCENT":
                self.ascent = int(k[1])
            elif k[0] == "FONT_DESCENT":
                self.descent = int(k[1])
            elif k[0] == "ENCODING":
                cur = {"enc": int(k[1])}
            elif k[0] == "DWIDTH":
                cur["dw"] = int(k[1])
            elif k[0] == "BBX":
                cur["bbx"] = list(map(int, k[1:5]))
            elif k[0] == "BITMAP":
                bitmap = []
            elif k[0] == "ENDCHAR":
                cur["bits"] = bitmap
                self.glyphs[cur["enc"]] = cur
                bitmap = None
            elif bitmap is not None:
                bitmap.append((int(k[0], 16), len(k[0]) * 4))

    def glyph(self, ch):
        return self.glyphs.get(ord(ch)) or self.glyphs[ord("?")]

    def width(self, s, spacing=0):
        return sum(self.glyph(c)["dw"] + spacing for c in s) - (spacing if s else 0)

    @property
    def height(self):
        return self.ascent + self.descent


class Canvas:
    def __init__(self, w, h, fill=PAPER):
        self.w, self.h = w, h
        self.px = [[fill] * w for _ in range(h)]

    def set(self, x, y, v):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = v

    def get(self, x, y):
        return self.px[y][x]

    def rect(self, x, y, w, h, v):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                self.set(xx, yy, v)

    def frame(self, x, y, w, h, v):
        self.hline(x, x + w - 1, y, v)
        self.hline(x, x + w - 1, y + h - 1, v)
        self.vline(x, y, y + h - 1, v)
        self.vline(x + w - 1, y, y + h - 1, v)

    def hline(self, x1, x2, y, v, step=1):
        for x in range(x1, x2 + 1, step):
            self.set(x, y, v)

    def vline(self, x, y1, y2, v, step=1):
        for y in range(y1, y2 + 1, step):
            self.set(x, y, v)

    def line(self, x0, y0, x1, y1, v):
        dx, dy = abs(x1 - x0), -abs(y1 - y0)
        sx, sy = (1 if x0 < x1 else -1), (1 if y0 < y1 else -1)
        err = dx + dy
        while True:
            self.set(x0, y0, v)
            if x0 == x1 and y0 == y1:
                return
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x0 += sx
            if e2 <= dx:
                err += dx
                y0 += sy

    def disc(self, cx, cy, r, v):
        """Filled circle centred on a pixel centre (cx, cy may be .5)."""
        for y in range(int(cy - r) - 1, int(cy + r) + 2):
            for x in range(int(cx - r) - 1, int(cx + r) + 2):
                if (x - cx) ** 2 + (y - cy) ** 2 <= r * r + 0.25:
                    self.set(x, y, v)

    def ring(self, cx, cy, r, v, width=1.0):
        for y in range(int(cy - r) - 1, int(cy + r) + 2):
            for x in range(int(cx - r) - 1, int(cx + r) + 2):
                d = math.hypot(x - cx, y - cy)
                if r - width < d <= r + 0.3:
                    self.set(x, y, v)

    def round_rect(self, x, y, w, h, r, v, fill=None):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                cx = min(max(xx, x + r), x + w - 1 - r)
                cy = min(max(yy, y + r), y + h - 1 - r)
                d = math.hypot(xx - cx, yy - cy)
                if d <= r + 0.3:
                    edge = d > r - 0.7 or xx in (x, x + w - 1) or yy in (y, y + h - 1)
                    if edge:
                        self.set(xx, yy, v)
                    elif fill is not None:
                        self.set(xx, yy, fill)

    def fill_poly(self, pts, v):
        """Fill a convex polygon given as (x, y) float points."""
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        n = len(pts)
        for y in range(int(min(ys)), int(max(ys)) + 2):
            for x in range(int(min(xs)), int(max(xs)) + 2):
                px, py = x + 0.5, y + 0.5
                sign = 0
                inside = True
                for i in range(n):
                    (x0, y0), (x1, y1) = pts[i], pts[(i + 1) % n]
                    cr = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0)
                    if cr != 0:
                        if sign == 0:
                            sign = 1 if cr > 0 else -1
                        elif (cr > 0) != (sign > 0):
                            inside = False
                            break
                if inside:
                    self.set(x, y, v)

    def text(self, font, x, y, s, v, spacing=0):
        """Draw s with its cell top at y. Returns the advance."""
        x0 = x
        base = y + font.ascent
        for c in s:
            g = font.glyph(c)
            w, h, ox, oy = g["bbx"]
            for r, (row, bits) in enumerate(g["bits"]):
                for col in range(w):
                    if row & (1 << (bits - 1 - col)):
                        self.set(x + ox + col, base - oy - h + r, v)
            x += g["dw"] + spacing
        return x - x0

    def text_center(self, font, cx, y, s, v, spacing=0):
        return self.text(font, cx - font.width(s, spacing) // 2, y, s, v, spacing)

    def blit(self, other, x, y, transparent=None):
        for yy in range(other.h):
            for xx in range(other.w):
                v = other.px[yy][xx]
                if v != transparent:
                    self.set(x + xx, y + yy, v)

    def crop(self, x, y, w, h):
        c = Canvas(w, h)
        for yy in range(h):
            for xx in range(w):
                c.px[yy][xx] = self.px[y + yy][x + xx]
        return c

    def save_bmp(self, path):
        row = (self.w * 3 + 3) & ~3
        data = bytearray()
        for y in range(self.h - 1, -1, -1):
            line = bytearray()
            for x in range(self.w):
                r, g, b = RGB[self.px[y][x]]
                line += bytes((b, g, r))
            data += line + b"\0" * (row - len(line))
        hdr = struct.pack("<2sIHHI", b"BM", 54 + len(data), 0, 0, 54)
        info = struct.pack("<IiiHHIIiiII", 40, self.w, self.h, 1, 24, 0,
                           len(data), 2835, 2835, 0, 0)
        with open(path, "wb") as f:
            f.write(hdr + info + data)


def vstack(frames):
    """Rockbox subimages are stacked vertically in one bitmap."""
    out = Canvas(frames[0].w, sum(f.h for f in frames))
    y = 0
    for f in frames:
        out.blit(f, 0, y)
        y += f.h
    return out


def load_pgm_levels(path):
    """Read an 8-bit binary/ascii PGM and quantise to 4 levels."""
    data = open(path, "rb").read()
    parts, i = [], 0
    while len(parts) < 4:
        while data[i:i + 1].isspace():
            i += 1
        if data[i:i + 1] == b"#":
            while data[i:i + 1] != b"\n":
                i += 1
            continue
        j = i
        while not data[j:j + 1].isspace():
            j += 1
        parts.append(data[i:j])
        i = j
    magic, w, h, mx = parts[0], int(parts[1]), int(parts[2]), int(parts[3])
    i += 1
    c = Canvas(w, h)
    if magic == b"P5":
        for y in range(h):
            for x in range(w):
                v = data[i + y * w + x] / mx
                c.px[y][x] = min(3, int(v * 3 + 0.5))
    else:
        vals = data[i:].split()
        for y in range(h):
            for x in range(w):
                c.px[y][x] = min(3, int(int(vals[y * w + x]) / mx * 3 + 0.5))
    return c
