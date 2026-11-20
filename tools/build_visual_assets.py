#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Build the deterministic food PBR maps (assets/materials/food), from seeded noise alone.

Run with /usr/bin/python3 (Pillow is installed for that interpreter); tools/build_food_materials.py runs it at another size.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
import math
import random

ROOT = Path(__file__).resolve().parents[1]
SIZE = 512


def noise(cells, seed):
    rng = random.Random(seed)
    values = [[rng.randrange(256) for _ in range(cells)] for _ in range(cells)]
    image = Image.new('L', (cells + 1, cells + 1))
    image.putdata([values[y % cells][x % cells] for y in range(cells + 1) for x in range(cells + 1)])
    return image.resize((SIZE, SIZE), Image.Resampling.BICUBIC)


def clamp(value):
    return max(0, min(255, int(value)))


def image_values(image):
    return list(image.get_flattened_data() if hasattr(image, 'get_flattened_data') else image.getdata())


def build_food_maps():
    out = ROOT / 'assets/materials/food'
    out.mkdir(parents=True, exist_ok=True)
    for index, kind in enumerate(('sausage', 'crust', 'crumb')):
        large = image_values(noise(12, 300 + index))
        small = image_values(noise(100, 400 + index))
        grain = image_values(noise(400, 500 + index))
        height = Image.new('L', (SIZE, SIZE))
        height.putdata([clamp(122 + (a - 128) * .14 + (b - 128) * .13) for a, b in zip(small, grain)])
        draw = ImageDraw.Draw(height)
        rng = random.Random(810 + index)
        if kind != 'sausage':
            for _ in range(3300 if kind == 'crumb' else 600):
                x, y = rng.randrange(SIZE), rng.randrange(SIZE)
                radius = rng.uniform(.6, 3.4 if kind == 'crumb' else 1.5)
                draw.ellipse((x-radius, y-radius*.7, x+radius, y+radius*.7), fill=rng.randrange(32, 88))
        height = height.filter(ImageFilter.GaussianBlur(.4))
        heights = image_values(height)
        colors, packed = [], []
        for i, (a, b, c, h) in enumerate(zip(large, small, grain, heights)):
            if kind == 'sausage':
                variation = (a - 128) * .14 + (b - 128) * .045 + (c - 128) * .025
                colors.append((clamp(161 + variation), clamp(69 + variation * .63), clamp(43 + variation * .43)))
                roughness = clamp(145 + (a - 128) * .11 + (c - 128) * .04)
            elif kind == 'crust':
                toast = (a - 128) * .26 + (b - 128) * .11 + min(0, h - 110) * .22
                colors.append((clamp(200 + toast), clamp(133 + toast * .87), clamp(61 + toast * .52)))
                roughness = clamp(216 + (b - 128) * .08)
            else:
                pore = min(0, h - 110) * .40
                variation = (a - 128) * .07 + (b - 128) * .04 + pore
                colors.append((clamp(239 + variation), clamp(210 + variation), clamp(154 + variation * .85)))
                roughness = clamp(236 + (c - 128) * .035)
            # StandardMaterial reads roughness from green and metalness from blue.
            packed.append((255, roughness, 0))
        normals = []
        strength = .50 if kind == 'sausage' else (.65 if kind == 'crust' else 1.0)
        for y in range(SIZE):
            for x in range(SIZE):
                dx = (heights[y*SIZE+(x+1) % SIZE] - heights[y*SIZE+(x-1) % SIZE]) / 255 * strength
                dy = (heights[((y+1) % SIZE)*SIZE+x] - heights[((y-1) % SIZE)*SIZE+x]) / 255 * strength
                length = math.sqrt(dx*dx + dy*dy + 1)
                normals.append((clamp(128-dx/length*127), clamp(128+dy/length*127), clamp(128+127/length)))
        for name, pixels in [('albedo', colors), ('normal', normals), ('roughness', packed)]:
            image = Image.new('RGB', (SIZE, SIZE))
            image.putdata(pixels)
            image.save(out / f'{kind}_{name}.png', optimize=True)
    print(f'Generated nine {SIZE}px food material maps')


if __name__ == '__main__':
    build_food_maps()
