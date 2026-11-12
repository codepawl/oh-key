"""Builds docs/media/typing.gif and typing.mp4.

Each frame shows the real engine output for the keys typed so far (oh-key-cli
type on every prefix), drawn by headless Chrome and encoded with ffmpeg.
Run it through the GPU queue: bash ~/.agents/tools/gpu-lock.sh uv run --no-project python tools/demo/make_demo.py
"""
import html
import json
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parents[2]
CLI = ROOT / "target/lead/x86_64-pc-windows-msvc/release/oh-key-cli.exe"
FONT = ROOT / "crates/oh-key-app/assets/fonts/JetBrainsMono-Regular.ttf"
CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
GIT_BASH = r"C:\Program Files\Git\bin\bash.exe"
WORK = ROOT / "target/demo"
OUT = ROOT / "docs/media"
KEYS = "Hoom nay deadline project, check email nhes"
WIDTH, HEIGHT = 960, 540
KEY_SECONDS = 0.13
SPACE_SECONDS = 0.22
END_HOLD_SECONDS = 2.2
START_HOLD_SECONDS = 0.6


def engine_text(prefix: str) -> str:
    if not prefix:
        return ""
    result = subprocess.run(
        [str(CLI), "type", prefix], capture_output=True, text=True, encoding="utf-8"
    )
    return result.stdout.rstrip("\r\n")


def page(prefix: str, text: str) -> str:
    recent = prefix[-14:]
    caps = []
    for index, key in enumerate(recent):
        label = "space" if key == " " else html.escape(key)
        newest = " new" if index == len(recent) - 1 else ""
        wide = " wide" if key == " " else ""
        caps.append(f'<span class="cap{newest}{wide}">{label}</span>')
    split = len(text) - len(text.split(" ")[-1])
    done, composing = html.escape(text[:split]), html.escape(text[split:])
    return f"""<!doctype html><meta charset="utf-8"><style>
@font-face {{ font-family: JB; src: url("{FONT.as_uri()}"); }}
* {{ box-sizing: border-box; margin: 0; }}
body {{ width: {WIDTH}px; height: {HEIGHT}px; background: #1F1E1D; color: #F0EEE6;
  font-family: JB, monospace; padding: 48px 56px; display: flex; flex-direction: column; gap: 28px; }}
.bar {{ display: flex; align-items: center; gap: 14px; font-size: 20px; color: #A8A59B; }}
.mode {{ background: #D97757; color: #1F1E1D; border-radius: 8px; padding: 4px 12px; font-weight: 700; }}
.field {{ background: #262624; border-radius: 14px; padding: 28px 32px; min-height: 190px;
  font-size: 40px; line-height: 1.5; word-break: break-word; }}
.composing {{ border-bottom: 3px solid #D97757; }}
.caret {{ display: inline-block; width: 3px; height: 1.1em; background: #D97757; vertical-align: -0.18em; margin-left: 2px; }}
.label {{ font-size: 18px; color: #A8A59B; margin-bottom: 10px; }}
.caps {{ display: flex; gap: 8px; flex-wrap: wrap; min-height: 52px; }}
.cap {{ background: #262624; border-radius: 8px; min-width: 44px; height: 48px; padding: 0 12px;
  display: inline-flex; align-items: center; justify-content: center; font-size: 24px; color: #F0EEE6;
  box-shadow: 0 3px 0 #141413; }}
.cap.wide {{ min-width: 90px; font-size: 18px; color: #A8A59B; }}
.cap.new {{ background: #D97757; color: #1F1E1D; }}
</style>
<div class="bar"><span class="mode">V</span><span>oh-key · Telex</span></div>
<div><div class="label">Text field</div>
<div class="field">{done}<span class="composing">{composing}</span><span class="caret"></span></div></div>
<div><div class="label">Keys pressed</div><div class="caps">{''.join(caps)}</div></div>
"""


def main() -> None:
    WORK.mkdir(parents=True, exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    frames = []
    for count in range(len(KEYS) + 1):
        prefix = KEYS[:count]
        text = engine_text(prefix)
        seconds = START_HOLD_SECONDS if count == 0 else (
            SPACE_SECONDS if prefix[-1] == " " else KEY_SECONDS
        )
        if count == len(KEYS):
            seconds = END_HOLD_SECONDS
        path = WORK / f"frame{count:03d}.html"
        path.write_text(page(prefix, text), encoding="utf-8")
        frames.append((count, text, seconds))
    (WORK / "frames.json").write_text(
        json.dumps([{"keys": KEYS[:c], "text": t} for c, t, _ in frames], ensure_ascii=False, indent=1),
        encoding="utf-8",
    )
    shots = WORK / "shoot.sh"
    lines = ["set -e"]
    for count, _, _ in frames:
        src = (WORK / f"frame{count:03d}.html").as_uri()
        png = (WORK / f"frame{count:03d}.png").as_posix()
        lines.append(
            f'"{CHROME}" --headless=new --use-angle=d3d11 --enable-gpu --ignore-gpu-blocklist '
            f'--allow-file-access-from-files --hide-scrollbars --force-device-scale-factor=1 '
            f'--window-size={WIDTH},{HEIGHT} --virtual-time-budget=1500 --screenshot="{png}" "{src}" >/dev/null 2>&1'
        )
    shots.write_text("\n".join(lines) + "\n", encoding="utf-8")
    subprocess.run([GIT_BASH, shots.as_posix()], check=True)

    concat = []
    for count, _, seconds in frames:
        concat.append(f"file 'frame{count:03d}.png'\nduration {seconds}")
    concat.append(f"file 'frame{len(KEYS):03d}.png'")
    (WORK / "list.txt").write_text("\n".join(concat) + "\n", encoding="utf-8")
    base = ["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(WORK / "list.txt")]
    subprocess.run(base + ["-vf", "fps=30,format=yuv420p", "-c:v", "libx264", "-crf", "20",
                           "-movflags", "+faststart", str(OUT / "typing.mp4")], check=True)
    palette = str(WORK / "palette.png")
    scale = "fps=15,scale=720:-1:flags=lanczos"
    subprocess.run(base + ["-vf", f"{scale},palettegen=max_colors=64", palette], check=True)
    subprocess.run(base + ["-i", palette, "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=none",
                           str(OUT / "typing.gif")], check=True)


main()
