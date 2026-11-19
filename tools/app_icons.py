"""Writes Waffle's brand assets from the design system, so the mark and icons have one source:
design/waffle.html. Headless Chrome renders its export mode.

    python3 tools/app_icons.py

Writes assets/brand/waffle-mark.svg (the flat mark), assets/brand/icons/*.png (one per AppIcon in
src/app_icon.rs), and from the default (paper) the macOS .icns and Windows .ico that release builds embed.
Needs Chrome (or CHROME=<path>) and Pillow; the .icns step needs macOS (iconutil).
"""

import html
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
PAGE = (ROOT / "design/waffle.html").as_uri()
OUT = ROOT / "assets/brand/icons"
SIZE = 512
# Order and names match `AppIcon` in src/app_icon.rs; the first is the default.
VARIANTS = ["paper", "peek", "sticker", "riso", "blueprint"]
CHROME_PATHS = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "google-chrome",
    "chromium",
]


def main():
    chrome = find_chrome()
    OUT.mkdir(parents=True, exist_ok=True)
    write_mark(chrome, ROOT / "assets/brand/waffle-mark.svg")
    for name in VARIANTS:
        screenshot(chrome, name, SIZE, OUT / f"{name}.png")
    with tempfile.TemporaryDirectory() as tmp:
        big = Path(tmp) / "default-1024.png"
        screenshot(chrome, VARIANTS[0], 1024, big)
        write_ico(Image.open(big), ROOT / "assets/windows/app.ico")
        write_icns(big, ROOT / "assets/macos/AppIcon.icns")


def find_chrome():
    for candidate in [os.environ.get("CHROME"), *CHROME_PATHS]:
        if candidate and (Path(candidate).exists() or shutil.which(candidate)):
            return candidate
    raise SystemExit("Chrome not found; set CHROME to its binary")


def chrome_run(chrome, *args):
    return subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--virtual-time-budget=4000", *args], check=True, capture_output=True, text=True)


def screenshot(chrome, variant, size, path):
    chrome_run(chrome, "--default-background-color=00000000", f"--window-size={size},{size}", f"--screenshot={path}", f"{PAGE}?export={variant}&size={size}")
    print("wrote", path)


def write_mark(chrome, path):
    dom = chrome_run(chrome, "--dump-dom", f"{PAGE}?export=mark").stdout
    found = re.search(r'<pre id="export">(.*?)</pre>', dom, re.S)
    if not found:
        raise SystemExit("design/waffle.html did not export the mark")
    path.write_text(html.unescape(found.group(1)) + "\n")
    print("wrote", path)


def write_ico(img, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("wrote", path)


def write_icns(png, path):
    if not shutil.which("iconutil"):
        print("skipped", path, "(iconutil is macOS only)")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "AppIcon.iconset"
        iconset.mkdir()
        src = Image.open(png)
        for n in (16, 32, 128, 256, 512):
            src.resize((n, n), Image.LANCZOS).save(iconset / f"icon_{n}x{n}.png")
            src.resize((n * 2, n * 2), Image.LANCZOS).save(iconset / f"icon_{n}x{n}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(path)], check=True)
    print("wrote", path)


if __name__ == "__main__":
    main()
