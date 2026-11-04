#!/usr/bin/env python3
"""Generate the pod theme: bitmaps, WPS/SBS skins, theme config, splash.

    mktheme.py <font-dir> <delta.pgm> <out-dir> <splash.bmp> <ui-h> <usb.bmp>

<font-dir> holds the BDF sources (jb11, or10, or14b, tiny6). Everything is
drawn from code so coordinates in the skins and bitmaps cannot drift apart.
"""
import math
import os
import sys

from canvas import BDF, Canvas, PAPER, FAINT, MID, INK, lcd, vstack, load_pgm_levels

LCD_W, LCD_H = 138, 110

fontdir, delta_pgm, out, splash_out = sys.argv[1:5]
jb = BDF(os.path.join(fontdir, "jb11.bdf"))
orb = BDF(os.path.join(fontdir, "or10.bdf"))
orbb = BDF(os.path.join(fontdir, "or14b.bdf"))
tiny = BDF(os.path.join(fontdir, "tiny6.bdf"))

UI_H = int(sys.argv[5]) if len(sys.argv) > 5 else jb.height  # line height after convbdf

img_dir = os.path.join(out, "wps", "pod")
os.makedirs(img_dir, exist_ok=True)


def save(c, name):
    c.save_bmp(os.path.join(img_dir, name))


# ---------------------------------------------------------------- shared bits

def delta_mark(c, x, y, v):
    """9x8 Δ outline."""
    for i in range(8):
        c.set(x + 4 - i // 2, y + i, v)
        c.set(x + 4 + (i + 1) // 2, y + i, v)
    c.hline(x, x + 8, y + 7, v)


def copy_mark(c, x, y, v, s=5):
    """⧉: two overlapping squares."""
    c.frame(x, y + 2, s, s, v)
    c.frame(x + 2, y, s, s, v)


def segments(c, x, y, w, h, v, seg=2, gap=1):
    xx = x
    while xx + seg <= x + w:
        c.rect(xx, y, seg, h, v)
        xx += seg + gap


# battery: outline in the backdrop, segments as the bar image
BAT_X, BAT_Y = 118, 1
BAT_FILL = (121, 3, 11, 3)


def battery_outline(c):
    c.frame(BAT_X, BAT_Y, 17, 7, MID)
    c.rect(BAT_X + 17, BAT_Y + 2, 2, 3, MID)
    segments(c, *BAT_FILL, FAINT)


bat = Canvas(BAT_FILL[2], BAT_FILL[3])
segments(bat, 0, 0, bat.w, bat.h, INK)
save(bat, "batt.bmp")

# playback state icons, 7x7: stop, play, pause, ffwd, rew (matches %mp)
states = []
for kind in ("stop", "play", "pause", "ff", "rew"):
    s = Canvas(7, 7)
    if kind == "stop":
        s.rect(1, 1, 5, 5, INK)
    elif kind == "play":
        for i in range(7):
            s.hline(1, 1 + min(i, 6 - i), i, INK)
    elif kind == "pause":
        s.rect(1, 0, 2, 7, INK)
        s.rect(4, 0, 2, 7, INK)
    elif kind == "ff":
        for i in range(7):
            n = min(i, 6 - i) // 2
            s.hline(0, n, i, INK)
            s.hline(3, 3 + n, i, INK)
    else:
        for i in range(7):
            n = min(i, 6 - i) // 2
            s.hline(3 - n, 3, i, INK)
            s.hline(6 - n, 6, i, INK)
    states.append(s)
save(vstack(states), "state.bmp")


# ---------------------------------------------------------------- WPS

SHELL = (3, 12, 132, 64)
LABEL = (12, 15, 114, 25)
WINDOW = (28, 43, 82, 25)
REEL = 23
REEL_L = (34, 44)
REEL_R = (81, 44)
TAPE_LEVELS = 6
REEL_FRAMES = 4

TIME_VP = (2, 79, 48, orbb.height)
VU = (58, 79, 78, 4)
VU_GAP = 7
PROG = (2, 95, 134, 5)
INFO_Y = 103

wps_bg = Canvas(LCD_W, LCD_H)
c = wps_bg

# status row
c.hline(0, LCD_W - 1, 9, FAINT)
for x in range(0, LCD_W, 23):
    c.set(x, 10, FAINT)
battery_outline(c)

# cassette shell
x, y, w, h = SHELL
c.round_rect(x, y, w, h, 4, MID, fill=FAINT)
for sx, sy in ((x + 4, y + 4), (x + w - 5, y + 4), (x + 4, y + h - 5), (x + w - 5, y + h - 5)):
    c.disc(sx, sy, 1.5, PAPER)
    c.set(sx, sy, MID)

# paper label with a colour band between title and artist
x, y, w, h = LABEL
c.round_rect(x, y, w, h, 2, MID, fill=MID)
c.hline(x + 2, x + w - 3, y + 15, FAINT)
c.hline(x + 2, x + w - 3, y + 16, FAINT)

# window with the tape packs
x, y, w, h = WINDOW
c.round_rect(x, y, w, h, 3, MID, fill=PAPER)
c.text(orb, 15, 50, "A", INK)
c.text_center(tiny, 121, 46, "TRK", MID)

# bottom ridge: trapezoid with guide holes
sx, sy, sw, sh = SHELL
by = sy + sh - 1
c.line(30, by, 36, by - 6, MID)
c.hline(36, sx + sw - 1 - 33, by - 6, MID)
c.line(sx + sw - 1 - 33, by - 6, sx + sw - 1 - 27, by, MID)
for hx in (50, 87):
    c.disc(hx, by - 3, 1.5, PAPER)
for hx in (41, 96):
    c.rect(hx, by - 4, 2, 2, PAPER)

# VU meters
x, y, w, h = VU
c.text(tiny, x - 6, y - 1, "L", MID)
c.text(tiny, x - 6, y + VU_GAP - 1, "R", MID)
segments(c, x, y, w, h, FAINT)
segments(c, x, y + VU_GAP, w, h, FAINT)
c.hline(x, x + w - 1, y + h + 1, FAINT, step=3)

# progress track
segments(c, *PROG, FAINT)
save(wps_bg, "wps_bg.bmp")

vu = Canvas(VU[2], VU[3])
segments(vu, 0, 0, vu.w, vu.h, MID)
hot = vu.w * 4 // 5
for xx in range(hot, vu.w):
    for yy in range(vu.h):
        if vu.get(xx, yy) == MID:
            vu.set(xx, yy, INK)
save(vu, "vu.bmp")

prog = Canvas(PROG[2], PROG[3])
segments(prog, 0, 0, prog.w, prog.h, INK)
save(prog, "progress.bmp")


def reel(level, frame):
    """Tape pack radius grows with level; hub spokes rotate with frame."""
    r = Canvas(REEL, REEL, PAPER)
    cx = cy = REEL // 2
    pack = 6 + level
    r.disc(cx, cy, pack, FAINT)
    r.ring(cx, cy, pack, MID, 1.0)
    r.disc(cx, cy, 5.5, PAPER)
    r.ring(cx, cy, 5.5, INK, 1.6)
    for i in range(3):
        a = math.radians(frame * 30 + i * 120)
        r.line(cx, cy, round(cx + 4.4 * math.cos(a)), round(cy + 4.4 * math.sin(a)), INK)
    r.disc(cx, cy, 1.2, INK)
    return r


for lvl in range(TAPE_LEVELS):
    save(vstack([reel(lvl, f) for f in range(REEL_FRAMES)]), f"reel{lvl}.bmp")

reels = "|".join(
    f"%xd(L{TAPE_LEVELS - 1 - i},%pW)%xd(R{i},%pW)" for i in range(TAPE_LEVELS))

wps = f"""# pod - cassette deck WPS for the iPod mini (bΔnst ⧉ design)
%wd
%X(wps_bg.bmp)
%Fl(2,10-Orbitron-Medium.fnt)
%Fl(3,14-Orbitron-Bold.fnt)
%Fl(4,06-Tiny.fnt)
%xl(S,state.bmp,1,1,5)
""" + "".join(
    f"%xl(L{i},reel{i}.bmp,{REEL_L[0]},{REEL_L[1]},{REEL_FRAMES})\n"
    f"%xl(R{i},reel{i}.bmp,{REEL_R[0]},{REEL_R[1]},{REEL_FRAMES})\n"
    for i in range(TAPE_LEVELS)) + f"""
%xd(S,%mp)
%?px<{reels}>

# status row
%V(10,2,40,6,4)%Vf({lcd(INK)})
%?mp<STOP|PLAY|PAUSE|FFWD|REW>
%V(52,2,34,6,4)%Vf({lcd(MID)})
%ac%?cc<%cH:%cM|>
%V(84,2,32,6,4)%Vf({lcd(MID)})
%ar%?ps<SHF|>%?mm<| RPT| RP1|| A-B>
%V({BAT_FILL[0]},{BAT_FILL[1]},{BAT_FILL[2]},{BAT_FILL[3]},1)
%bl(0,0,{BAT_FILL[2]},{BAT_FILL[3]},image,batt.bmp)

# cassette label
%V({LABEL[0] + 2},{LABEL[1] + 1},{LABEL[2] - 4},{UI_H},1)%Vf({lcd(PAPER)})
%s%?it<%it|%fn>
%V({LABEL[0] + 2},{LABEL[1] + 18},{LABEL[2] - 4},6,4)%Vf({lcd(PAPER)})
%s%?ia<%ia|UNKNOWN ARTIST>
%V(110,54,22,{orb.height},2)%Vf({lcd(INK)})
%ac%pp

# counter, meters, tape position
%V({TIME_VP[0]},{TIME_VP[1]},{TIME_VP[2]},{TIME_VP[3]},3)%Vf({lcd(INK)})
%?mv(1)<%pv|%pc>
%V({VU[0]},{VU[1]},{VU[2]},{VU[3] + VU_GAP},1)
%pL(0,0,{VU[2]},{VU[3]},image,vu.bmp)%pR(0,{VU_GAP},{VU[2]},{VU[3]},image,vu.bmp)
%V({PROG[0]},{PROG[1]},{PROG[2]},{PROG[3]},1)
%pb(0,0,{PROG[2]},{PROG[3]},image,progress.bmp)

# album, remaining
%V(2,{INFO_Y},96,6,4)%Vf({lcd(MID)})
%s%?id<%id|>%?iy< - %iy|>
%V(98,{INFO_Y},38,6,4)%Vf({lcd(MID)})
%ar-%pr
"""

# ---------------------------------------------------------------- SBS (lists)

HEAD_H = 12
LIST_Y = HEAD_H + 3
ROWS = (LCD_H - LIST_Y - 10) // UI_H
LIST_H = ROWS * UI_H
FOOT_Y = LIST_Y + LIST_H + 1

sbs_bg = Canvas(LCD_W, LCD_H)
c = sbs_bg
delta_mark(c, 2, 2, INK)
c.hline(0, LCD_W - 1, HEAD_H, FAINT)
for x in range(0, LCD_W, 23):
    c.set(x, HEAD_H + 1, FAINT)
battery_outline(c)
c.vline(LCD_W - 2, LIST_Y, LIST_Y + LIST_H - 1, FAINT, step=2)
c.hline(0, LCD_W - 1, FOOT_Y, FAINT)
save(sbs_bg, "sbs_bg.bmp")

scroll = Canvas(2, LIST_H)
scroll.rect(0, 0, 2, LIST_H, INK)
save(scroll, "scroll.bmp")

sbs = f"""# pod - library chrome for the iPod mini (bΔnst ⧉ design)
%X(sbs_bg.bmp)
%Fl(2,10-Orbitron-Medium.fnt)
%Fl(4,06-Tiny.fnt)
%xl(S,state.bmp,1,{FOOT_Y + 2},5)
%xl(Sl,knob.bmp,0,0)
%Vi(-,0,{LIST_Y},{LCD_W - 4},{LIST_H},1)
%Lb(item,{LCD_W - 4},{UI_H})
%xd(S,%mp)

%V(13,2,103,{orb.height},2)%Vf({lcd(INK)})
%s%Lt
%V({BAT_FILL[0]},{BAT_FILL[1]},{BAT_FILL[2]},{BAT_FILL[3]},1)
%bl(0,0,{BAT_FILL[2]},{BAT_FILL[3]},image,batt.bmp)

%Vl(item,0,0,{LCD_W - 4},{UI_H},1)%Vf({lcd(INK)})%Vb({lcd(PAPER)})
%?Lc<%Vs(invert)▸ %s%LT|  %LT>

%V({LCD_W - 2},{LIST_Y},2,{LIST_H},1)
%?LB<%LB(0,0,2,{LIST_H},noborder,slider,Sl)|>

%V(10,{FOOT_Y + 3},92,6,4)%Vf({lcd(MID)})
%?mp<NO TAPE|%s%it|%s%it|%s%it|%s%it>
%V(102,{FOOT_Y + 3},34,6,4)%Vf({lcd(MID)})
%?mp<|%ar%pc|%ar%pc|%ar%pc|%ar%pc>
"""

knob = Canvas(2, 6)
knob.rect(0, 0, 2, 6, INK)
save(knob, "knob.bmp")

with open(os.path.join(out, "wps", "pod.wps"), "w") as f:
    f.write(wps)
with open(os.path.join(out, "wps", "pod.sbs"), "w") as f:
    f.write(sbs)

# ---------------------------------------------------------------- splash

sp = Canvas(LCD_W, LCD_H)
glyph = load_pgm_levels(delta_pgm)
sp.blit(glyph, (LCD_W - glyph.w) // 2, 44 - glyph.h // 2, transparent=PAPER)

foot = "bΔnst"
fw = jb.width(foot) + 4 + 7 + 4 + jb.width("design")
fx = (LCD_W - fw) // 2
fy = LCD_H - jb.height - 4
x = fx + sp.text(jb, fx, fy, foot, INK) + 4
copy_mark(sp, x, fy + jb.ascent - 7, MID)
sp.text(jb, x + 7 + 4, fy, "design", MID)
for (bx, by, dx, dy) in ((1, 1, 1, 1), (LCD_W - 2, 1, -1, 1),
                         (1, LCD_H - 2, 1, -1), (LCD_W - 2, LCD_H - 2, -1, -1)):
    for i in range(5):
        sp.set(bx + i * dx, by, FAINT)
        sp.set(bx, by + i * dy, FAINT)
sp.save_bmp(splash_out)

# ---------------------------------------------------------------- USB link


def ipod(c, x, y):
    """22x34 iPod mini: body, screen, click wheel."""
    c.round_rect(x, y, 22, 34, 3, INK)
    c.frame(x + 4, y + 4, 14, 11, MID)
    c.ring(x + 11, y + 24, 6, INK, 1.2)
    c.disc(x + 11, y + 24, 2, MID)


def host(c, x, y):
    """34x30 terminal: screen with scanlines, stand."""
    c.round_rect(x, y, 34, 24, 2, INK)
    for yy in range(y + 4, y + 21, 2):
        c.hline(x + 4, x + 29, yy, FAINT)
    c.rect(x + 14, y + 24, 6, 3, INK)
    c.hline(x + 8, x + 25, y + 27, INK)
    c.hline(x + 8, x + 25, y + 28, INK)


usb = Canvas(LCD_W, LCD_H)
c = usb
delta_mark(c, 2, 2, INK)
c.text(orb, 13, 2, "DATA LINK", INK)
chip = tiny.width("USB") + 8
c.rect(LCD_W - chip - 1, 1, chip, 9, INK)
c.text_center(tiny, LCD_W - 1 - chip // 2, 3, "USB", PAPER)
c.hline(0, LCD_W - 1, HEAD_H, FAINT)
for x in range(0, LCD_W, 23):
    c.set(x, HEAD_H + 1, FAINT)

ipod(c, 14, 22)
host(c, 90, 24)
c.text_center(tiny, 25, 60, "POD", MID)
c.text_center(tiny, 107, 60, "HOST", MID)
# transfer line with arrowheads and a block meter under it
c.hline(41, 86, 38, INK, step=2)
for i in range(3):
    c.vline(41 + i, 38 - i, 38 + i, INK)
    c.vline(86 - i, 38 - i, 38 + i, INK)
segments(c, 44, 44, 40, 3, FAINT)
segments(c, 44, 44, 25, 3, INK)
c.text_center(tiny, 64, 50, "MASS STORAGE", MID)

c.text_center(orbb, LCD_W // 2, 72, "LINKED", INK)
c.hline(0, LCD_W - 1, 96, FAINT)
c.text_center(tiny, LCD_W // 2, 101, "EJECT BEFORE UNPLUGGING", MID)
if len(sys.argv) > 6:
    usb.save_bmp(sys.argv[6])

print(f"theme: {ROWS} list rows of {UI_H}px")
