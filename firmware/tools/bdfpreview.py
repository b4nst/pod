#!/usr/bin/env python3
"""Render text with a BDF font to a PGM: bdfpreview.py font.bdf out.pgm "line" ["line" ...]"""
import sys

def load(path):
    glyphs, fbb, cur, bitmap = {}, None, None, None
    for line in open(path, encoding="latin-1"):
        k = line.split()
        if not k: continue
        if k[0] == "FONTBOUNDINGBOX": fbb = list(map(int, k[1:5]))
        elif k[0] == "ENCODING": cur = {"enc": int(k[1])}
        elif k[0] == "DWIDTH": cur["dw"] = int(k[1])
        elif k[0] == "BBX": cur["bbx"] = list(map(int, k[1:5]))
        elif k[0] == "BITMAP": bitmap = []
        elif k[0] == "ENDCHAR":
            cur["bits"] = bitmap; glyphs[cur["enc"]] = cur; bitmap = None
        elif bitmap is not None: bitmap.append(int(k[0], 16) << (32 - 4 * len(k[0])))
    return glyphs, fbb

glyphs, (fw, fh, fx, fy) = load(sys.argv[1])
lines = sys.argv[3:]
W = max(sum(glyphs.get(ord(c), glyphs.get(63))["dw"] for c in l) for l in lines) + 4
H = fh * len(lines) + 4
px = [[0] * W for _ in range(H)]
for li, l in enumerate(lines):
    x = 2; base = 2 + li * fh + fh + fy
    for c in l:
        g = glyphs.get(ord(c), glyphs.get(63))
        w, h, ox, oy = g["bbx"]
        for r, row in enumerate(g["bits"]):
            for col in range(w):
                if row & (1 << (31 - col)):
                    yy = base - oy - h + r; xx = x + ox + col
                    if 0 <= yy < H and 0 <= xx < W: px[yy][xx] = 1
        x += g["dw"]
with open(sys.argv[2], "w") as f:
    f.write(f"P2 {W} {H} 1\n" + "\n".join(" ".join(str(1 - v) for v in r) for r in px))
