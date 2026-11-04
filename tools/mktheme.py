#!/usr/bin/env python3
"""Generate the pod theme: bitmaps, WPS/SBS skins, theme config, splash.

    mktheme.py <font-dir> <delta.pgm> <out-dir> <splash.bmp> <ui-h> <usb.bmp>

<font-dir> holds the BDF sources (jb11, or10, or14b, tiny6). Everything is
drawn from code so coordinates in the skins and bitmaps cannot drift apart.
"""
import math
import os
import sys

import iso
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
BAT_X, BAT_Y = 118, 2
BAT_FILL = (121, 4, 11, 3)


def battery_outline(c, v=MID, empty=FAINT):
    c.frame(BAT_X, BAT_Y, 17, 7, v)
    c.rect(BAT_X + 17, BAT_Y + 2, 2, 3, v)
    segments(c, *BAT_FILL, empty)


def battery_image(v):
    b = Canvas(BAT_FILL[2], BAT_FILL[3], INK if v == PAPER else PAPER)
    segments(b, 0, 0, b.w, b.h, v)
    return b


save(battery_image(INK), "batt.bmp")
save(battery_image(PAPER), "batt_inv.bmp")


def dot_grid(c, x0, y0, x1, y1, step=4):
    for y in range(y0, y1 + 1, step):
        for x in range(x0, x1 + 1, step):
            c.set(x, y, FAINT)


def crosshair(c, x, y, v=INK, r=3):
    c.hline(x - r, x + r, y, v)
    c.vline(x, y - r, y + r, v)


def hatch(c, x0, y0, x1, y1, v=INK, step=3):
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            if (x + y) % step == 0:
                c.set(x, y, v)


def head_bar(c, title=None):
    """Inverted header strip with the Δ mark, as on a HUD."""
    c.rect(0, 0, LCD_W, HEAD_H, INK)
    delta_mark(c, 2, 2, PAPER)
    if title:
        c.text(orb, 13, 2, title, PAPER)
    hatch(c, 0, HEAD_H + 1, LCD_W - 1, HEAD_H + 2)


HEAD_H = 11

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

# cassette, projected; the window region animates
CAS = iso.Cassette(22, 22, 0.84)
WBOX = CAS.window_box()
TAPE_LEVELS = 6
REEL_FRAMES = 4

COL_X = 109                        # HUD readout column right of the cassette
TITLE_Y = 70
TIME_Y = 90
VU = (70, 91, 66, 3)
VU_GAP = 6
PROG = (2, 104, 134, 5)

wps_bg = Canvas(LCD_W, LCD_H)
c = wps_bg
dot_grid(c, 0, 13, LCD_W - 1, 66)

# status row
c.hline(0, LCD_W - 1, 10, INK)
battery_outline(c, INK)

iso.draw(CAS, c)

# HUD column: side, tape position, track; dotted leader from the take-up hub
cx = COL_X
c.rect(cx, 13, 29, 8, INK)
c.text_center(tiny, cx + 14, 14, "SIDE A", PAPER)
c.text(tiny, cx, 26, "POS", MID)
c.text(tiny, cx, 44, "TRK", MID)
rx, ry = CAS.ip(iso.HUB_R, iso.HUB_Y)
crosshair(c, rx, ry - 12, INK, 2)
c.hline(rx + 3, cx - 2, ry - 12, INK, step=2)
c.vline(cx - 2, 26, ry - 12, INK, step=2)
c.hline(cx - 2, cx - 1, 26, INK)
crosshair(c, 4, 64)
crosshair(c, LCD_W - 5, 64)

# title strip
c.hline(0, LCD_W - 1, TITLE_Y - 2, INK)

# counter and meters
c.hline(0, LCD_W - 1, TIME_Y - 2, FAINT, step=2)
c.text(tiny, VU[0] - 6, VU[1] - 1, "L", INK)
c.text(tiny, VU[0] - 6, VU[1] + VU_GAP - 1, "R", INK)
segments(c, VU[0], VU[1], VU[2], VU[3], FAINT)
segments(c, VU[0], VU[1] + VU_GAP, VU[2], VU[3], FAINT)

# tape position
segments(c, *PROG, FAINT)
save(wps_bg, "wps_bg.bmp")

vu = Canvas(VU[2], VU[3])
segments(vu, 0, 0, vu.w, vu.h, MID)
for xx in range(vu.w * 3 // 4, vu.w):
    for yy in range(vu.h):
        if vu.get(xx, yy) == MID:
            vu.set(xx, yy, INK)
save(vu, "vu.bmp")

prog = Canvas(PROG[2], PROG[3])
segments(prog, 0, 0, prog.w, prog.h, INK)
save(prog, "progress.bmp")

# one strip per tape level, one subimage per hub angle, cropped from a full
# render so the edges line up with the backdrop
bx0, by0, bx1, by1 = WBOX
for lvl in range(TAPE_LEVELS):
    frames = []
    for f in range(REEL_FRAMES):
        full = Canvas(LCD_W, LCD_H)
        iso.draw(CAS, full, tape=lvl / (TAPE_LEVELS - 1), angle=f * 30)
        frames.append(full.crop(bx0, by0, bx1 - bx0, by1 - by0))
    save(vstack(frames), f"tape{lvl}.bmp")

tapes = "|".join(f"%xd(T{i},%pW)" for i in range(TAPE_LEVELS))

wps = f"""# pod - cassette deck WPS for the iPod mini (bΔnst ⧉ design)
%wd
%X(wps_bg.bmp)
%Fl(2,10-Orbitron-Medium.fnt)
%Fl(3,14-Orbitron-Bold.fnt)
%Fl(4,06-Tiny.fnt)
%xl(S,state.bmp,1,2,5)
""" + "".join(
    f"%xl(T{i},tape{i}.bmp,{bx0},{by0},{REEL_FRAMES})\n"
    for i in range(TAPE_LEVELS)) + f"""
%xd(S,%mp)
%?px<{tapes}>

# status row
%V(10,3,40,6,4)%Vf({lcd(INK)})
%?mp<STOP|PLAY|PAUSE|FFWD|REW>
%V(52,3,34,6,4)%Vf({lcd(MID)})
%ac%?cc<%cH:%cM|>
%V(84,3,32,6,4)%Vf({lcd(MID)})
%ar%?ps<SHF|>%?mm<| RPT| RP1|| A-B>
%V({BAT_FILL[0]},{BAT_FILL[1]},{BAT_FILL[2]},{BAT_FILL[3]},1)
%bl(0,0,{BAT_FILL[2]},{BAT_FILL[3]},image,batt.bmp)

# HUD column
%V({COL_X},32,{LCD_W - COL_X},{orb.height},2)%Vf({lcd(INK)})
%px%%
%V({COL_X},50,{LCD_W - COL_X},{orb.height},2)%Vf({lcd(INK)})
%pp/%pe

# title, artist and album
%V(2,{TITLE_Y},{LCD_W - 4},{UI_H},1)%Vf({lcd(INK)})
%s%?it<%it|%fn>
%V(2,{TITLE_Y + UI_H + 1},{LCD_W - 30},6,4)%Vf({lcd(MID)})
%s%?ia<%ia|UNKNOWN ARTIST>%?id< - %id|>
%V({LCD_W - 28},{TITLE_Y + UI_H + 1},26,6,4)%Vf({lcd(MID)})
%ar-%pr

# counter, meters, tape position
%V(2,{TIME_Y},56,{orbb.height},3)%Vf({lcd(INK)})
%?mv(1)<%pv|%pc>
%V({VU[0]},{VU[1]},{VU[2]},{VU[3] + VU_GAP},1)
%pL(0,0,{VU[2]},{VU[3]},image,vu.bmp)%pR(0,{VU_GAP},{VU[2]},{VU[3]},image,vu.bmp)
%V({PROG[0]},{PROG[1]},{PROG[2]},{PROG[3]},1)
%pb(0,0,{PROG[2]},{PROG[3]},image,progress.bmp)
"""

# ---------------------------------------------------------------- SBS (lists)

LIST_Y = HEAD_H + 4
ROWS = (LCD_H - LIST_Y - 10) // UI_H
LIST_H = ROWS * UI_H
FOOT_Y = LIST_Y + LIST_H + 1
NUM_W = 16

sbs_bg = Canvas(LCD_W, LCD_H)
c = sbs_bg
head_bar(c)
battery_outline(c, PAPER, MID)
c.vline(LCD_W - 2, LIST_Y, LIST_Y + LIST_H - 1, FAINT, step=2)
c.hline(0, LCD_W - 1, FOOT_Y, INK)
segments(c, 0, LCD_H - 1, LCD_W, 1, FAINT)
save(sbs_bg, "sbs_bg.bmp")

foot_prog = Canvas(LCD_W, 1)
segments(foot_prog, 0, 0, LCD_W, 1, INK)
save(foot_prog, "foot_prog.bmp")

sbs = f"""# pod - library chrome for the iPod mini (bΔnst ⧉ design)
%X(sbs_bg.bmp)
%Fl(2,10-Orbitron-Medium.fnt)
%Fl(4,06-Tiny.fnt)
%xl(S,state.bmp,1,{FOOT_Y + 2},5)
%xl(Sl,knob.bmp,0,0)
%Vi(-,0,{LIST_Y},{LCD_W - 4},{LIST_H},1)
%Lb(item,{LCD_W - 4},{UI_H})
%xd(S,%mp)

%V(13,2,103,{orb.height},2)%Vf({lcd(PAPER)})%Vb({lcd(INK)})
%s%Lt
%V({BAT_FILL[0]},{BAT_FILL[1]},{BAT_FILL[2]},{BAT_FILL[3]},1)
%bl(0,0,{BAT_FILL[2]},{BAT_FILL[3]},image,batt_inv.bmp)

%Vl(item,0,0,{LCD_W - 4 - NUM_W},{UI_H},1)%Vf({lcd(INK)})%Vb({lcd(PAPER)})
%?Lc<%Vs(invert)▸ %s%LT|  %LT>
%Vl(item,{LCD_W - 4 - NUM_W},0,{NUM_W},{UI_H},4)%Vf({lcd(INK)})%Vb({lcd(PAPER)})
%?Lc<%Vs(invert)|>%ar%LN 

%V({LCD_W - 2},{LIST_Y},2,{LIST_H},1)
%?LB<%LB(0,0,2,{LIST_H},noborder,slider,Sl)|>

%V(10,{FOOT_Y + 2},92,6,4)%Vf({lcd(MID)})
%?mp<NO TAPE|%s%it|%s%it|%s%it|%s%it>
%V(102,{FOOT_Y + 2},34,6,4)%Vf({lcd(MID)})
%?mp<|%ar%pc|%ar%pc|%ar%pc|%ar%pc>
%V(0,{LCD_H - 1},{LCD_W},1,1)
%?mp<|%pb(0,0,{LCD_W},1,image,foot_prog.bmp)|%pb(0,0,{LCD_W},1,image,foot_prog.bmp)|%pb(0,0,{LCD_W},1,image,foot_prog.bmp)|%pb(0,0,{LCD_W},1,image,foot_prog.bmp)>
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
    c.round_rect(x, y, 22, 34, 3, INK, fill=PAPER)
    c.frame(x + 4, y + 4, 14, 11, MID)
    c.ring(x + 11, y + 24, 6, INK, 1.2)
    c.disc(x + 11, y + 24, 2, MID)


def host(c, x, y):
    """34x30 terminal: screen with scanlines, stand."""
    c.round_rect(x, y, 34, 24, 2, INK, fill=PAPER)
    for yy in range(y + 4, y + 21, 2):
        c.hline(x + 4, x + 29, yy, FAINT)
    c.rect(x + 14, y + 24, 6, 3, INK)
    c.hline(x + 8, x + 25, y + 27, INK)
    c.hline(x + 8, x + 25, y + 28, INK)


usb = Canvas(LCD_W, LCD_H)
c = usb
head_bar(c, "DATA LINK")
chip = tiny.width("USB") + 8
c.rect(LCD_W - chip - 2, 2, chip, 7, PAPER)
c.text_center(tiny, LCD_W - 2 - chip // 2, 3, "USB", INK)
dot_grid(c, 0, 16, LCD_W - 1, 68)

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
