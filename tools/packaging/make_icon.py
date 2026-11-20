# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""The packages' application icon, drawn here from shapes (no source image): a smiling hotdog in its bun with a
mustard squiggle, on a rounded dark tile.

    python3 tools/packaging/make_icon.py          writes hotdog.png (1024 x 1024), hotdog.ico and hotdog.icns beside it

Needs Pillow. tools/build-windows.sh embeds hotdog.ico in the game's .exe; tools/build-mac.sh puts hotdog.icns in the
app bundle.
"""
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
S = 2048  # drawn at twice the size and scaled down, for smooth edges


def capsule(draw, cx, cy, length, thickness, fill, outline=None, width=0):
    r = thickness / 2
    draw.rounded_rectangle((cx - length / 2, cy - r, cx + length / 2, cy + r), radius=r, fill=fill, outline=outline, width=width)


def hotdog():
    layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    cx, cy = S / 2, S / 2 + 40
    # The bun's back half, the sausage, then the bun's front lip.
    capsule(d, cx, cy + 40, 1500, 520, (196, 132, 64, 255))
    capsule(d, cx, cy + 10, 1450, 440, (226, 166, 94, 255))
    capsule(d, cx, cy - 70, 1720, 300, (150, 52, 34, 255))
    capsule(d, cx, cy - 92, 1680, 250, (186, 72, 46, 255))
    capsule(d, cx - 40, cy - 150, 1500, 60, (222, 120, 90, 255))
    capsule(d, cx, cy + 150, 1440, 250, (236, 182, 112, 255))
    capsule(d, cx, cy + 205, 1380, 120, (214, 150, 80, 255))
    # The mustard squiggle along the sausage.
    for i in range(1201):
        t = i / 1200
        x, y = cx - 660 + 1020 * t, cy - 95 + 42 * math.sin(t * math.pi * 6)
        d.ellipse((x - 23, y - 23, x + 23, y + 23), fill=(244, 196, 18, 255))
    # A friendly face at the sausage's front end: two eyes and a smile.
    for ex in (cx + 470, cx + 610):
        d.ellipse((ex - 58, cy - 210, ex + 58, cy - 82), fill=(255, 255, 255, 255))
        d.ellipse((ex - 22, cy - 170, ex + 26, cy - 112), fill=(24, 20, 22, 255))
    d.arc((cx + 470, cy - 120, cx + 620, cy - 10), start=20, end=160, fill=(70, 18, 14, 255), width=22)
    return layer.rotate(28, resample=Image.BICUBIC, center=(S / 2, S / 2))


def tile():
    base = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle((96, 96, S - 96, S - 96), radius=420, fill=255)
    top, bottom = (52, 62, 88), (22, 26, 40)
    gradient = Image.new("RGBA", (1, S))
    for y in range(S):
        t = y / (S - 1)
        gradient.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,))
    base.paste(gradient.resize((S, S)), (0, 0), mask)
    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    dog = hotdog()
    shadow.paste((0, 0, 0, 120), (0, 0), dog.split()[3])
    shadow = shadow.filter(ImageFilter.GaussianBlur(28))
    base.alpha_composite(shadow, (0, 36))
    base.alpha_composite(dog)
    return base.resize((1024, 1024), Image.LANCZOS)


def main():
    icon = tile()
    icon.save(HERE / "hotdog.png")
    icon.save(HERE / "hotdog.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    icon.save(HERE / "hotdog.icns")
    print("wrote", ", ".join(str(HERE / n) for n in ("hotdog.png", "hotdog.ico", "hotdog.icns")))


if __name__ == "__main__":
    main()
