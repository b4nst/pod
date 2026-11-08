#!/usr/bin/env python3
"""Generate a tiny tagged FLAC library for the simulator: mk-test-library.py <dir>"""
import math, os, struct, subprocess, sys, wave

LIB = [
    ("Neon Cassette", "Magnetic Dawn", 1984, "Synthwave",
     ["Reel to Reel", "Chrome Tape", "Night Drive", "Analog Heart"]),
    ("Neon Cassette", "Afterglow", 1991, "Synthwave", ["Dusk", "Rewind"]),
    ("Kollektiv 7", "Warehouse Sessions", 2023, "Techno",
     ["Sub Pressure", "Concrete", "Stack Up the Bins"]),
    ("Unit Δelta", "Signal / Noise", 2019, "Ambient",
     ["Carrier", "Low Orbit", "Dropout", "Hiss"]),
]

def tone(path, freq, secs=20, rate=22050):
    with wave.open(path, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
        w.writeframes(b"".join(
            struct.pack("<h", int(6000 * math.sin(2 * math.pi * freq * i / rate)))
            for i in range(secs * rate)))

out = sys.argv[1]
for artist, album, year, genre, tracks in LIB:
    d = os.path.join(out, "Music", artist, album.replace("/", "-"))
    os.makedirs(d, exist_ok=True)
    for n, title in enumerate(tracks, 1):
        wav = os.path.join(d, "tmp.wav")
        tone(wav, 220 * (1 + n / 4))
        subprocess.run(["flac", "-s", "-f", "-o", os.path.join(d, f"{n:02d} {title}.flac"),
                        f"--tag=ARTIST={artist}", f"--tag=ALBUM={album}",
                        f"--tag=TITLE={title}", f"--tag=TRACKNUMBER={n}",
                        f"--tag=DATE={year}", f"--tag=GENRE={genre}", wav], check=True)
        os.remove(wav)
