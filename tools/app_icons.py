"""Draws Dango's app icons: assets/brand/icons/*.png (one per AppIcon in src/app_icon.rs), and from the
default (bone) the macOS .icns and Windows .ico that release builds embed.

    python3 tools/app_icons.py

Needs Pillow; the .icns step needs macOS (iconutil).
"""

import math
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets/brand/icons"

SIZE = 512
# Drawn at 4x and scaled down, which is all the antialiasing Pillow needs.
SS = 4
CANVAS = SIZE * SS
# The macOS icon grid: an 824/1024 tile, centred.
TILE = (50 * SS, 462 * SS)

CHILI = "#E25B26"
CHILI_LIGHT = "#FF9A82"
CHILI_DEEP = "#8F2A17"
WOOD = "#C79A6B"

# Dumplings in a 100-unit box: (centres, radius, skewer ends and width).
# Bone, Brand and Graphite overlap them, each cut from the one behind by a ring of the backdrop's
# colour (design/names.html); Sticker and Peek space them just apart, as in assets/brand/dango-mark.svg.
OVERLAPPING = ([(36, 64), (50, 50), (64, 36)], 15, ((20, 80), (80, 20), 6))
SPACED = ([(28, 72), (50, 50), (72, 28)], 14.5, ((12, 88), (88, 12), 6))
RING = 2.5


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, draw in VARIANTS.items():
        draw().save(OUT / f"{name}.png")
        print("wrote", OUT / f"{name}.png")
    write_ico(Image.open(OUT / "bone.png"), ROOT / "assets/windows/app.ico")
    write_icns(OUT / "bone.png", ROOT / "assets/macos/AppIcon.icns")


# Shapes


def canvas():
    return Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))


def squircle_mask():
    lo, hi = TILE
    a = (hi - lo) / 2
    c = lo + a
    pts = []
    for i in range(720):
        t = i / 720 * math.tau
        x, y = math.cos(t), math.sin(t)
        pts.append((c + a * math.copysign(abs(x) ** 0.4, x), c + a * math.copysign(abs(y) ** 0.4, y)))
    mask = Image.new("L", (CANVAS, CANVAS), 0)
    ImageDraw.Draw(mask).polygon(pts, fill=255)
    return mask


MASK = squircle_mask()


def rgb(hex_):
    hex_ = hex_.lstrip("#")
    return tuple(int(hex_[i : i + 2], 16) for i in (0, 2, 4))


def gradient(top, bottom, angle=0.35):
    """A linear gradient across the canvas, tilted `angle` of the way towards a diagonal."""
    ramp = Image.linear_gradient("L").resize((CANVAS, CANVAS))
    diag = ramp.rotate(45, resample=Image.BICUBIC, fillcolor=128)
    t = Image.blend(ramp, diag, angle)
    solid = lambda c: Image.new("RGBA", (CANVAS, CANVAS), rgb(c) + (255,))
    return Image.composite(solid(bottom), solid(top), t)


def tile(top, bottom):
    out = canvas()
    out.paste(gradient(top, bottom), (0, 0), MASK)
    return out


def finish(img):
    clipped = canvas()
    clipped.paste(img, (0, 0), ImageChops.multiply(img.split()[3], MASK))
    return clipped.resize((SIZE, SIZE), Image.LANCZOS)


def place(scale, dx=0.0, dy=0.0):
    """Maps mark units to canvas pixels: the mark `scale` of the tile wide, centred, then shifted."""
    lo, hi = TILE
    span = (hi - lo) * scale
    ox = lo + (hi - lo - span) / 2 + dx * (hi - lo)
    oy = lo + (hi - lo - span) / 2 + dy * (hi - lo)
    return lambda x, y: (ox + x / 100 * span, oy + y / 100 * span), span / 100


def circle(center, r):
    x, y = center
    return (x - r, y - r, x + r, y + r)


def shine(img, box, opacity):
    # Its own layer, composited: ImageDraw replaces pixels, so a translucent fill drawn straight onto
    # the ball would punch a see-through hole instead of brightening it.
    x0, y0, x1, _ = box
    r = (x1 - x0) / 2
    cx, cy = x0 + r * 0.62, y0 + r * 0.58
    layer = canvas()
    ImageDraw.Draw(layer).ellipse((cx - r * 0.3, cy - r * 0.2, cx + r * 0.3, cy + r * 0.2), fill=(255, 255, 255, round(255 * opacity)))
    img.alpha_composite(layer)


def soft_shadow(img, offset, blur, color, opacity):
    alpha = img.split()[3].filter(ImageFilter.GaussianBlur(blur)).point(lambda v: round(v * opacity))
    shadow = Image.new("RGBA", img.size, rgb(color) + (0,))
    shadow.putalpha(alpha)
    moved = canvas()
    moved.paste(shadow, offset, shadow)
    return moved


def dango(at, unit, colors, ring, stick, shine_opacity=0.55, shape=OVERLAPPING):
    """The mark on its own layer. `ring` is the backdrop colour cut between dumplings, or None to skip it."""
    centers, radius, ((x1, y1), (x2, y2), w) = shape
    layer = canvas()
    d = ImageDraw.Draw(layer)
    d.line([at(x1, y1), at(x2, y2)], fill=stick, width=round(w * unit))
    for end in (at(x1, y1), at(x2, y2)):
        d.ellipse(circle(end, w * unit / 2), fill=stick)
    for (x, y), color in zip(centers, colors):
        if ring:
            d.ellipse(circle(at(x, y), (radius + RING) * unit), fill=ring)
        d.ellipse(circle(at(x, y), radius * unit), fill=color)
    for x, y in centers:
        shine(layer, circle(at(x, y), radius * unit), shine_opacity)
    return layer


# Variants


def plain(top, bottom, ink, ring, stick):
    img = tile(top, bottom)
    at, unit = place(0.74)
    img.alpha_composite(dango(at, unit, [ink] * 3, ring, stick))
    return finish(img)


def bone():
    return plain("#FBF8F2", "#EDE6DA", CHILI, "#F6F1E8", "#C9A27A")


def brand():
    return plain(CHILI_LIGHT, CHILI, "#FFFFFF", CHILI, CHILI_DEEP)


def graphite():
    return plain("#3A3438", "#17131A", CHILI_LIGHT, "#221D22", "#8C7A6A")


def sticker():
    img = tile("#F48A5C", "#DC4E22")
    d = ImageDraw.Draw(img)
    lo, hi = TILE
    step = 22 * SS
    for y in range(lo, hi, step):
        for x in range(lo + (step // 2 if (y // step) % 2 else 0), hi, step):
            d.ellipse(circle((x, y), 2.2 * SS), fill=(255, 255, 255, 46))
    at, unit = place(0.8)
    # Hanami colours, cut out with a thick white border and lifted off the page.
    art = dango(at, unit, ["#9BCB6C", "#FFF6E6", "#F7A1B8"], None, "#B9875A", 0.45, SPACED)
    border = art.split()[3].filter(ImageFilter.MaxFilter(odd(9 * unit)))
    cut = Image.new("RGBA", art.size, (255, 255, 255, 0))
    cut.putalpha(border)
    img.alpha_composite(soft_shadow(cut, (0, round(8 * SS)), 10 * SS, "#5A1A08", 0.4))
    img.alpha_composite(cut)
    img.alpha_composite(art)
    return finish(img)


def odd(v):
    v = round(v)
    return v if v % 2 else v + 1


def peek():
    img = tile("#FFDB73", "#F4B02E")
    # The dango, big, rising in from the bottom left corner.
    at, unit = place(1.15, -0.12, 0.22)
    img.alpha_composite(soft_shadow(dango(at, unit, [CHILI] * 3, None, WOOD, 0, SPACED), (0, round(10 * SS)), 16 * SS, "#8A4A00", 0.3))
    img.alpha_composite(dango(at, unit, [CHILI] * 3, None, WOOD, 0.55, SPACED))
    return finish(img)


VARIANTS = {
    "bone": bone,
    "brand": brand,
    "graphite": graphite,
    "sticker": sticker,
    "peek": peek,
}


# Platform icon files


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
            if n * 2 <= SIZE:
                src.resize((n * 2, n * 2), Image.LANCZOS).save(iconset / f"icon_{n}x{n}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(path)], check=True)
    print("wrote", path)


if __name__ == "__main__":
    main()
