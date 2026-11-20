#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""A deterministic red/white package label, sized for the front seal.

The brand is made up for the game (Snappington's); the label names no real product. The lettering is drawn with the
system's DejaVu fonts (free licence; a rendering of their glyphs is not a copy of the fonts).
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parents[1]
image = Image.new("RGB", (1400, 200), (147, 9, 24))
draw = ImageDraw.Draw(image)
font_root = Path("/usr/share/fonts/truetype/dejavu")
large = ImageFont.truetype(str(font_root / "DejaVuSerif-Bold.ttf"), 66)
small = ImageFont.truetype(str(font_root / "DejaVuSans-Bold.ttf"), 23)
draw.rectangle((9, 9, 1390, 190), outline=(241, 222, 179), width=3)
draw.text((55, 28), "Snappington's", font=large, fill=(253, 241, 213))
draw.text((60, 126), "BEEF FRANKS", font=small, fill=(253, 241, 213))
draw.text((890, 62), "6 FRANKS", font=small, fill=(253, 241, 213))
draw.text((890, 111), "KEEP REFRIGERATED", font=small, fill=(253, 241, 213))
out = root / "assets/materials/package_label.png"
out.parent.mkdir(parents=True, exist_ok=True)
image.save(out, optimize=True)
print(out)
