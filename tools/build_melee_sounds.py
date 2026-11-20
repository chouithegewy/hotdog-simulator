#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the melee strike's two sounds into assets/audio (mono, 16-bit, 22050 Hz like the others):

* whoosh.wav: the gun swung through the air, a band of noise that sweeps up as the swing speeds into the blow and
  falls away after it, with a thin whistle riding the sweep (about a quarter of a second). Played with every strike.
* thwack.wav: the blow landing, a body of low thump that drops in pitch, a hard crack on top of it and a wet slap
  of sauce (a fifth of a second). Played when a strike hits something.

Run: python3 tools/build_melee_sounds.py   (numpy and scipy). The output is the same every time (a fixed seed).
"""
import os
import wave

import numpy as np
from scipy.signal import butter, lfilter

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'assets', 'audio')


def band(x, low, high, order=2):
    b, a = butter(order, [low / (RATE / 2), min(high, RATE / 2 - 100) / (RATE / 2)], btype='band')
    return lfilter(b, a, x)


def sweep(noise, start, end, width, length):
    """Noise through a band whose middle slides from `start` to `end` Hz over the length, in short blocks."""
    out = np.zeros_like(noise)
    blocks = 24
    edges = np.linspace(0, len(noise), blocks + 1).astype(int)
    for i in range(blocks):
        f = start * (end / start) ** (i / (blocks - 1))
        a, b = edges[i], edges[i + 1]
        lo, hi = max(60, f - width * f), f + width * f
        # (A little overlap at each edge so that the blocks join without a click.)
        s, e = max(0, a - 80), min(len(noise), b + 80)
        piece = band(noise[s:e], lo, hi)
        ramp = np.ones(e - s)
        k = min(80, e - s)
        if s < a:
            ramp[:k] = np.linspace(0, 1, k)
        if e > b:
            ramp[-k:] = np.linspace(1, 0, k)
        out[s:e] += piece * ramp
    return out


def whoosh(rng, seconds=0.26):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    noise = rng.standard_normal(n)
    # The air: rises through the wind-up and the snap, peaks as the blow is fastest (about 60% in) and falls away.
    air = sweep(noise, 500, 4200, 0.45, seconds)
    shape = np.sin(np.pi * np.clip(t / seconds, 0, 1) ** 0.75) ** 2.2
    # A thin whistle riding the sweep.
    pitch = 900 + 1900 * np.clip(t / (seconds * 0.6), 0, 1) ** 1.3 - 700 * np.clip((t - seconds * 0.6) / (seconds * 0.4), 0, 1)
    whistle = np.sin(2 * np.pi * np.cumsum(pitch) / RATE) * 0.05
    out = (air * 0.6 + whistle) * shape
    out *= np.minimum(1, t / 0.01) * np.minimum(1, (seconds - t) / 0.05)
    return out


def thwack(rng, seconds=0.22):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    # The thump: a sine that drops from 190 to 70 Hz, dying in a tenth of a second.
    pitch = 70 + 120 * np.exp(-t / 0.03)
    thump = np.sin(2 * np.pi * np.cumsum(pitch) / RATE) * np.exp(-t / 0.055)
    # The crack: a hard burst of the upper middle, over almost at once.
    crack = band(rng.standard_normal(n), 1200, 5200) * np.exp(-t / 0.008)
    # The slap of sauce: a wet band of noise with a bubble in it.
    wet = band(rng.standard_normal(n), 300, 1500) * np.exp(-t / 0.035) * np.minimum(1, t / 0.003)
    bubble_t = np.clip(t - 0.03, 0, None)
    bubble = np.sin(2 * np.pi * np.cumsum(420 + 700 * np.clip(bubble_t / 0.05, 0, 1)) / RATE) * np.exp(-bubble_t / 0.02) * (t > 0.03)
    out = thump * 1.0 + crack * 0.55 + wet * 0.6 + bubble * 0.18
    out *= np.minimum(1, t / 0.001) * np.minimum(1, (seconds - t) / 0.03)
    return out


def write(name, x, peak):
    x = x / np.max(np.abs(x)) * peak
    data = (x * 32767).astype('<i2').tobytes()
    with wave.open(os.path.join(OUT, name), 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data)
    print(f'{name}: {len(x) / RATE:.2f} s')


if __name__ == '__main__':
    rng = np.random.default_rng(11)
    write('whoosh.wav', whoosh(rng), 0.6)
    write('thwack.wav', thwack(rng), 0.85)
