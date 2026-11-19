#!/usr/bin/env python3
"""Generate the pod theme for the iPod Classic: bitmaps, WPS/SBS skins,
theme config, splash, USB screen and the bootloader's splash.

    mktheme6g.py <font-dir> <out-dir> <splash.bmp> <usb.bmp> <podlogo.h>

<font-dir> holds the BDF sources (ter12, ter14b, ter18b). The look is
autoreverse's: a dark panel, a lighter band for the header and the footer,
amber readouts, teal status, and four stripes from yellow to plum for what
is lit. The greys lean to violet, for the purple shell. Everything is drawn
from the layout constants below so the skins and the bitmaps cannot drift
apart.
"""
import math
import os
import sys

from canvas import BDF, RGBCanvas, rgb

LCD_W, LCD_H = 320, 240

PALETTE = {
    "PANEL": "0B0D14",
    "BAND": "14111F",
    "RULE": "2C2540",
    "WELL": "171329",
    "TEXT": "EDE3CF",
    "TEXT2": "CFC6B3",
    "TEXT3": "8C8678",
    "OFF": "655E7A",
    "AMBER": "FFA526",
    "TEAL": "46DCC8",
    "STRIPES": ("F7B52C", "F2701C", "D9302A", "8E2A7E"),
    "SEL0": "31284A",
    "SEL1": "1D1830",
}
P = PALETTE
MAGENTA = RGBCanvas.MAGENTA

fontdir, out, splash_out, usb_out, podlogo_out = sys.argv[1:6]
ter12 = BDF(os.path.join(fontdir, "ter12.bdf"))
ter14b = BDF(os.path.join(fontdir, "ter14b.bdf"))
ter18b = BDF(os.path.join(fontdir, "ter18b.bdf"))

img_dir = os.path.join(out, "wps", "pod")
os.makedirs(img_dir, exist_ok=True)


def save(c, name):
    c.save_bmp(os.path.join(img_dir, name))


# ---------------------------------------------------------------- layout

HEAD_H = 24                         # the band, and a rule of 1 px under it
LIST_Y, ROW_H, ROWS = 34, 24, 7
LIST_H = ROW_H * ROWS
FOOT_Y = 208                        # the rule over the footer band
PROG_H = 2                          # the progress line, on the last rows

# the readouts from the right edge, as the deck has them: clock, battery,
# its percentage, hold, the cassette
CLOCK_X = 284
BAT_X, BAT_Y = 253, 6               # outline, 23x12
BAT_FILL = (BAT_X + 2, BAT_Y + 2, 19, 8)
PCT_X = 215
LOCK_X, LOCK_Y = 202, 7
TAPE_X, TAPE_Y = 171, 5             # 25x15


# ---------------------------------------------------------------- shared bits

def stripes(c, y, t, kick):
    """Four bands of t px from y down, turning up 45 degrees at x = kick and
    running off the top."""
    for k, col in enumerate(P["STRIPES"]):
        top = y + k * t
        for x in range(c.w):
            d = max(0, x - kick)
            c.rect(x, top - d, 1, t, col)


def header(c):
    c.rect(0, 0, LCD_W, HEAD_H, P["BAND"])
    c.hline(0, LCD_W - 1, HEAD_H, P["RULE"])


def footer(c):
    c.rect(0, FOOT_Y, LCD_W, LCD_H - FOOT_Y, P["BAND"])
    c.hline(0, LCD_W - 1, FOOT_Y, P["RULE"])


def segments(c, x, y, w, h, v, seg=3, gap=1):
    xx = x
    while xx + seg <= x + w:
        c.rect(xx, y, seg, h, v)
        xx += seg + gap


def delta(c, cx, top, size, stroke, colour, bg):
    """A geometric delta, apex at (cx, top), size wide. The edges are
    antialiased in 4 coverage steps (25, 50, 75, 100 %) over bg, so the
    bootloader's splash needs few colours."""
    h = size * math.sqrt(3) / 2
    outer = ((cx, top), (cx + size / 2, top + h), (cx - size / 2, top + h))
    # the inner triangle is the outer one shrunk about its centre by the stroke
    mid = top + 2 * h / 3
    k = 1 - stroke / (h / 3)
    inner = tuple((cx + (px - cx) * k, mid + (py - mid) * k) for px, py in outer)

    def inside(tri, px, py):
        sign = 0
        for i in range(3):
            (x0, y0), (x1, y1) = tri[i], tri[(i + 1) % 3]
            cr = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0)
            if cr:
                if sign == 0:
                    sign = 1 if cr > 0 else -1
                elif (cr > 0) != (sign > 0):
                    return False
        return True

    c0, c1 = rgb(bg), rgb(colour)
    n = 4
    for y in range(int(top) - 1, int(top + h) + 2):
        for x in range(int(cx - size / 2) - 1, int(cx + size / 2) + 2):
            hit = 0
            for sy in range(n):
                for sx in range(n):
                    px, py = x + (sx + .5) / n, y + (sy + .5) / n
                    if inside(outer, px, py) and not inside(inner, px, py):
                        hit += 1
            level = round(hit * 4 / (n * n))
            if level:
                c.set(x, y, tuple(c0[i] + (c1[i] - c0[i]) * level // 4
                                  for i in range(3)))


def delta_mark(c, x, y, colour):
    """A small delta, 14 px wide, for the header."""
    delta(c, x + 7, y, 14, 2.4, colour, P["BAND"])


# ---------------------------------------------------------------- images

# battery: the outline is in the backdrop, the lit segments are the bar image
batt = RGBCanvas(BAT_FILL[2], BAT_FILL[3], P["BAND"])
segments(batt, 0, 0, batt.w, batt.h, P["TEAL"])
save(batt, "batt.bmp")

# the padlock while HOLD is on, 7x9
LOCK = ("..###..", ".#...#.", ".#...#.", "#######", "#######", "###.###",
        "###.###", "#######", "#######")
lock = RGBCanvas(7, 9, P["BAND"])
for y, row in enumerate(LOCK):
    for x, ch in enumerate(row):
        if ch == "#":
            lock.set(x, y, P["AMBER"])
save(lock, "lock.bmp")

# the cassette, as pod_deck.c draws it in the header: a strip of the eight
# steps of the hubs' notches in teal, and the one in grey with nothing to play
NOTCH = ((-2, -2), (0, -2), (2, -2), (2, 0), (2, 2), (0, 2), (-2, 2), (-2, 0))


def tape_icon(c, x, y, colour, step):
    c.frame(x, y, 25, 15, colour)
    c.hline(x + 6, x + 18, y + 11, colour)
    for h in range(2):
        cx, cy = x + 7 + h * 11, y + 6
        c.rect(cx - 2, cy - 2, 5, 5, colour)
        c.set(cx + NOTCH[step][0], cy + NOTCH[step][1], P["PANEL"])


tape = RGBCanvas(25, 15 * 8, P["BAND"])
for step in range(8):
    tape_icon(tape, 0, 15 * step, P["TEAL"], step)
save(tape, "tape.bmp")
tape_off = RGBCanvas(25, 15, P["BAND"])
tape_icon(tape_off, 0, 0, P["OFF"], 0)
save(tape_off, "tape_off.bmp")

tab = RGBCanvas(3, ROW_H, P["AMBER"])
save(tab, "tab.bmp")

prog = RGBCanvas(LCD_W, PROG_H, P["STRIPES"][1])
save(prog, "prog.bmp")

# ---------------------------------------------------------------- SBS (lists)

sbs_bg = RGBCanvas(LCD_W, LCD_H, P["PANEL"])
c = sbs_bg
header(c)
footer(c)
c.frame(BAT_X, BAT_Y, 23, 12, P["OFF"])
c.rect(BAT_X + 23, BAT_Y + 4, 2, 4, P["OFF"])
c.rect(BAT_X + 1, BAT_Y + 1, 21, 10, P["BAND"])
segments(c, *BAT_FILL, P["RULE"])
c.rect(0, LCD_H - PROG_H, LCD_W, PROG_H, P["RULE"])
save(sbs_bg, "sbs_bg.bmp")

FONT_TITLE = 3      # 14-Tape-Bold, the header title
FONT_SMALL = 2      # 12-Tape, status and footer
TAB_W = 12
FOOT_TEXT_Y = FOOT_Y + 5

sbs = f"""# pod - library chrome for the iPod Classic (bΔnst ⧉ design)
%X(sbs_bg.bmp)
%Fl({FONT_SMALL},12-Tape.fnt)
%Fl({FONT_TITLE},14-Tape-Bold.fnt)
%xl(T,tab.bmp,0,0)
%xl(K,lock.bmp,{LOCK_X},{LOCK_Y})
%xl(C,tape.bmp,{TAPE_X},{TAPE_Y},8)
%xl(G,tape_off.bmp,{TAPE_X},{TAPE_Y})
%Vi(-,0,{LIST_Y},{LCD_W},{LIST_H},1)
%Lb(item,{LCD_W},{ROW_H})

# header: title in amber, hold, the cassette, battery and clock in teal
%V(0,0,{LCD_W},{HEAD_H},-)
%?mh<%xd(K)|>
%?mp<%xd(G)|%xd(C,%pW)|%xd(C)|%xd(C,%pW)|%xd(C,%pW)>
%V(8,5,{TAPE_X - 16},16,{FONT_TITLE})%Vf({P["AMBER"]})%Vb({P["BAND"]})
%s%Lt
%V({BAT_FILL[0]},{BAT_FILL[1]},{BAT_FILL[2]},{BAT_FILL[3]},1)
%bl(0,0,{BAT_FILL[2]},{BAT_FILL[3]},image,batt.bmp)
%V({PCT_X},6,32,12,{FONT_SMALL})%Vf({P["TEAL"]})%Vb({P["BAND"]})
%ar%bl%%
%V({CLOCK_X},6,30,12,{FONT_SMALL})%Vf({P["TEAL"]})%Vb({P["BAND"]})
%ar%cH:%cM

# the rows: the gradient bar and the amber tab under the selection. The UI
# font is as tall as a row (see build-theme.sh), so the bar fills it.
%Vl(item,0,0,{TAB_W},{ROW_H},1)%Vf({P["TEXT2"]})%Vb({P["PANEL"]})%Vg({P["SEL0"]},{P["SEL1"]},{P["AMBER"]})
%?Lc<%Vs(gradient)%xd(T) | >
%Vl(item,{TAB_W},0,{LCD_W - TAB_W},{ROW_H},1)%Vf({P["TEXT2"]})%Vb({P["PANEL"]})%Vg({P["SEL0"]},{P["SEL1"]},{P["AMBER"]})
%?Lc<%Vs(gradient)%s%LT|%LT>

# footer: the state, then the title and the artist
%V(8,{FOOT_TEXT_Y},52,12,{FONT_SMALL})%Vf({P["AMBER"]})%Vb({P["BAND"]})
%?mp<%Vf({P["OFF"]})NO TAPE|PLAY|PAUSE|FF|REW>
%V(64,{FOOT_TEXT_Y},248,12,{FONT_SMALL})%Vf({P["TEXT"]})%Vb({P["BAND"]})
%?mp<|%s%?it<%it|%fn>%?ia< · %ia|>|%s%?it<%it|%fn>%?ia< · %ia|>|%s%?it<%it|%fn>%?ia< · %ia|>|%s%?it<%it|%fn>%?ia< · %ia|>>
%V(0,{LCD_H - PROG_H},{LCD_W},{PROG_H},1)
%?mp<|%pb(0,0,{LCD_W},{PROG_H},image,prog.bmp)|%pb(0,0,{LCD_W},{PROG_H},image,prog.bmp)|%pb(0,0,{LCD_W},{PROG_H},image,prog.bmp)|%pb(0,0,{LCD_W},{PROG_H},image,prog.bmp)>
"""

# ---------------------------------------------------------------- WPS

# The player is the deck in play mode: the firmware draws the whole screen
# (%pD: header, bargraph, panes, the track band, the progress and the
# footer, everywhere but the counter) and the counter row with the time and
# the cassette (%pC) on top of it. COUNTER is pod_deck.c's.
COUNTER = (0, 183, LCD_W, 32)

wps = f"""# pod - cassette deck WPS for the iPod Classic (bΔnst ⧉ design)
%wd

%V(0,0,{LCD_W},{LCD_H},1)%Vf({P["TEXT"]})%Vb({P["PANEL"]})
%pD

%V({COUNTER[0]},{COUNTER[1]},{COUNTER[2]},{COUNTER[3]},1)%Vf({P["TEXT"]})%Vb({P["PANEL"]})
%pC
"""

with open(os.path.join(out, "wps", "pod.wps"), "w") as f:
    f.write(wps)
with open(os.path.join(out, "wps", "pod.sbs"), "w") as f:
    f.write(sbs)

# ---------------------------------------------------------------- theme config

with open(os.path.join(out, "themes", "pod.cfg"), "w") as f:
    f.write(f"""# pod - autoreverse look for the iPod Classic (bΔnst ⧉ design)
wps: /.pod/wps/pod.wps
sbs: /.pod/wps/pod.sbs
font: /.pod/fonts/16-Tape.fnt
statusbar: top
scrollbar: off
show icons: off
iconset: -
viewers iconset: -
backdrop: -
ui viewport: -
foreground color: {P["TEXT"]}
background color: {P["PANEL"]}
selector type: bar (gradient)
line selector start color: {P["SEL0"]}
line selector end color: {P["SEL1"]}
line selector text color: {P["AMBER"]}
list separator height: off
""")

# ---------------------------------------------------------------- splash

def copy_mark(c, x, y, v, s=7):
    """The two overlapping squares."""
    c.frame(x, y + 3, s, s, v)
    c.frame(x + 3, y, s, s, v)


sp = RGBCanvas(LCD_W, LCD_H, P["PANEL"])
SP_Y = 148
stripes(sp, y=SP_Y, t=4, kick=232)
delta(sp, 160, 36, 104, 15, P["AMBER"], P["PANEL"])
foot = "bΔnst"
fw = ter12.width(foot) + 8 + 10 + 8 + ter12.width("design")
fx = (LCD_W - fw) // 2
fy = LCD_H - 24
x = fx + sp.text(ter12, fx, fy, foot, P["OFF"]) + 8
copy_mark(sp, x, fy + 1, P["OFF"])
sp.text(ter12, x + 10 + 8, fy, "design", P["OFF"])
sp.save_bmp(splash_out)

# the bootloader has no room for the bitmap: runs of (count, palette index)
# for each row, over a palette of the few colours the splash uses
colours = []
rows = []
for y in range(LCD_H):
    runs = []
    for x in range(LCD_W):
        col = sp.px[y][x]
        if col not in colours:
            colours.append(col)
        i = colours.index(col)
        if runs and runs[-1][1] == i and runs[-1][0] < 255:
            runs[-1][0] += 1
        else:
            runs.append([1, i])
    rows.append(runs)
assert len(colours) <= 16, f"{len(colours)} colours in the splash"
with open(podlogo_out, "w") as f:
    f.write(f"""/* The pod splash for the s5l87xx bootloader, which has no room for the 16-bit
 * bitmap: {LCD_W}x{LCD_H} pixels, row by row, as (count, palette index) runs.
 * Made by firmware/tools/mktheme6g.py, as rockboxlogo.{LCD_W}x{LCD_H}x16.bmp. */
#define PODLOGO_WIDTH   {LCD_W}
#define PODLOGO_HEIGHT  {LCD_H}
#define PODLOGO_PALETTE \\
""")
    f.write(", \\\n".join("    0x%02x%02x%02x" % c for c in colours) + "\n\n")
    f.write("static const unsigned char podlogo[] = {\n")
    flat = [v for runs in rows for run in runs for v in run]
    for i in range(0, len(flat), 16):
        f.write("    " + ", ".join(str(v) for v in flat[i:i + 16]) + ",\n")
    f.write("};\n")

# ---------------------------------------------------------------- USB link


def reel(c, cx, cy, r):
    c.disc(cx, cy, r, P["TEXT3"])
    c.disc(cx, cy, r - 3, P["WELL"])
    c.disc(cx, cy, 6, P["TEXT2"])
    c.disc(cx, cy, 2, P["WELL"])
    for a in range(6):
        ang = a * math.pi / 3
        c.line(cx, cy, round(cx + 6 * math.cos(ang)), round(cy + 6 * math.sin(ang)), P["WELL"])


def cassette(c, x, y):
    """200x112 tape: label with stripes, window with both reels."""
    c.round_rect(x, y, 200, 112, 6, P["OFF"], fill=P["WELL"])
    c.rect(x + 14, y + 10, 172, 38, P["BAND"])
    c.frame(x + 14, y + 10, 172, 38, P["RULE"])
    for k, col in enumerate(P["STRIPES"]):
        c.rect(x + 14, y + 34 + k * 3, 172, 3, col)
    c.text(ter14b, x + 22, y + 15, "POD", P["AMBER"])
    c.text(ter12, x + 140, y + 17, "C60", P["TEAL"])
    # the window, the tape between the reels
    c.round_rect(x + 40, y + 56, 120, 40, 6, P["RULE"], fill=P["PANEL"])
    c.hline(x + 66, x + 134, y + 76, P["TEXT3"])
    reel(c, x + 66, y + 76, 15)
    reel(c, x + 134, y + 76, 15)
    c.rect(x + 78, y + 100, 44, 4, P["RULE"])


usb = RGBCanvas(LCD_W, LCD_H, P["PANEL"])
c = usb
header(c)
delta_mark(c, 8, 5, P["AMBER"])
c.text(ter14b, 28, 5, "DATA LINK", P["AMBER"])
chip = ter12.width("USB") + 8
c.frame(LCD_W - 8 - chip, 6, chip, 14, P["TEAL"])
c.text_center(ter12, LCD_W - 8 - chip // 2 - 1, 7, "USB", P["TEAL"])
cassette(c, 60, 44)
c.text_center(ter18b, LCD_W // 2, 170, "LINKED", P["AMBER"])
c.hline(0, LCD_W - 1, 204, P["RULE"])
c.text_center(ter12, LCD_W // 2, 216, "EJECT BEFORE UNPLUGGING", P["OFF"])
usb.save_bmp(usb_out)

print(f"theme: {ROWS} list rows of {ROW_H}px, {len(colours)} splash colours")
