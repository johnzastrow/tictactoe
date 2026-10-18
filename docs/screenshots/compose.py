"""Combine the two halves of a live-demo recording.

    python compose.py rust|go

<lang>/raw/{tui.mp4,tui.png} come from tapes/live.tape and
<lang>/raw/{web.webm,web.png,web-offset.txt} from live.py. Writes
<lang>/live.gif (terminal and browser side by side, in sync) and
<lang>/live.png (the final position in both).
"""
import os
import subprocess
import sys

from PIL import Image, ImageDraw, ImageFont

LANG = sys.argv[1]
RAW = f"{LANG}/raw"
H = 640  # output height, px
TUI_FIRST_MOVE = 1.25  # seconds into the TUI recording when X's first move shows (tuned by eye)
BG, FG = (17, 21, 31), (230, 233, 240)


def font(size):
    for f in ("/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc",
              "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


# ---- still: final position, terminal | browser ----
tui = Image.open(f"{RAW}/tui.png").convert("RGB")
web = Image.open(f"{RAW}/web.png").convert("RGB")
tui = tui.resize((round(tui.width * H / tui.height), H), Image.LANCZOS)
web = web.resize((round(web.width * H / web.height), H), Image.LANCZOS)
M, GAP, TOP = 24, 24, 60
out = Image.new("RGB", (M * 2 + tui.width + GAP + web.width, TOP + H + M), BG)
d = ImageDraw.Draw(out)
d.text((M, 16), "Terminal (X, keyboard)", fill=FG, font=font(26))
d.text((M + tui.width + GAP, 16), "Browser (O, mouse)", fill=FG, font=font(26))
out.paste(tui, (M, TOP))
out.paste(web, (M + tui.width + GAP, TOP))
out.save(f"{LANG}/live.png", optimize=True)
print("ok", f"{LANG}/live.png")

# ---- video: line up X's first move in both recordings, then stack ----
offset = float(open(f"{RAW}/web-offset.txt").read())
skip = max(0.0, offset - TUI_FIRST_MOVE)
graph = (
    f"[0:v]fps=12,scale=-2:{H}:flags=lanczos[t];"
    f"[1:v]trim=start={skip:.2f},setpts=PTS-STARTPTS,fps=12,scale=-2:{H}:flags=lanczos[w];"
    "[t][w]hstack=inputs=2:shortest=1,split[a][b];"
    "[a]palettegen=max_colors=128:stats_mode=diff[p];"
    "[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle"
)
subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{RAW}/tui.mp4", "-i", f"{RAW}/web.webm",
                "-filter_complex", graph, f"{LANG}/live.gif"], check=True)
print("ok", f"{LANG}/live.gif", f"(browser trimmed {skip:.2f}s)")
