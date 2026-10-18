"""Cut the recorded web session into one clip per control.

    python cut_web.py rust|go

Reads <lang>/raw/web-session.webm (the 1280x760 window; Playwright records at
1x even when the page renders at 2x) and
<lang>/raw/segments.json from capture_web.py. Each clip starts half a second
before its control opens, ends after it closes, and is cropped to the same
box as the still and doubled to the still's size. Writes
<lang>/raw/web-NN-name.mp4 (for compose4.py) and
<lang>/web-NN-name.gif.
"""
import json
import subprocess
import sys

LANG = sys.argv[1]
RAW = f"{LANG}/raw"
LEAD = 0.5   # seconds shown before the control opens
SCALE = 1    # video pixels per CSS pixel
GIF_W = 760   # max GIF width

segs = json.load(open(f"{RAW}/segments.json"))
for name, s in sorted(segs.items()):
    x0, y0, x1, y1 = (round(v * SCALE) for v in s["box"])
    w, h = x1 - x0, y1 - y0
    start = max(0.0, s["open"] - LEAD)
    crop = f"crop={w}:{h}:{x0}:{y0}"
    # seek after -i (decode from the start): Playwright's WebM seeks badly
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{RAW}/web-session.webm",
                    "-ss", f"{start:.2f}", "-to", f"{s['end']:.2f}", "-vf", f"{crop},scale={w * 2}:{h * 2}:flags=lanczos,fps=24",
                    "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p", f"{RAW}/web-{name}.mp4"],
                   check=True)
    gw = min(GIF_W, w * 2) // 2 * 2
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{RAW}/web-{name}.mp4", "-filter_complex",
                    f"fps=10,scale={gw}:-1:flags=lanczos,split[a][b];"
                    "[a]palettegen=max_colors=96:stats_mode=diff[p];"
                    "[b][p]paletteuse=dither=none:diff_mode=rectangle",
                    f"{LANG}/web-{name}.gif"], check=True)
    print("ok", name, f"{s['end'] - start:.1f}s")
