"""Cut the recorded web session into one clip per item.

    python cut_web.py rust|go

Reads <lang>/raw/web-session.webm and <lang>/raw/segments.json from
capture_web.py. Each clip starts LEAD seconds before its control opens and
ends after the still was taken. Writes <lang>/raw/web-NN-name.mp4 (for
compose4.py) and <lang>/web-NN-name.gif.
"""
import json
import subprocess
import sys

LANG = sys.argv[1]
RAW = f"{LANG}/raw"
LEAD = 0.5    # seconds shown before the control opens
GIF_W = 1000  # GIF width

segs = json.load(open(f"{RAW}/segments.json"))
for name, s in sorted(segs.items()):
    start = max(0.0, s["open"] - LEAD)
    # seek after -i (decode from the start): Playwright's WebM seeks badly
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{RAW}/web-session.webm",
                    "-ss", f"{start:.2f}", "-to", f"{s['end']:.2f}", "-vf", "fps=24",
                    "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p", f"{RAW}/web-{name}.mp4"],
                   check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{RAW}/web-{name}.mp4", "-filter_complex",
                    f"fps=10,scale={GIF_W}:-1:flags=lanczos,split[a][b];"
                    "[a]palettegen=max_colors=96:stats_mode=diff[p];"
                    "[b][p]paletteuse=dither=none:diff_mode=rectangle",
                    f"{LANG}/web-{name}.gif"], check=True)
    print("ok", name, f"{s['end'] - start:.1f}s")
