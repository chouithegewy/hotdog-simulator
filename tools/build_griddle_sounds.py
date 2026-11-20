#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the electric griddle's two sounds into assets/audio (mono, 16-bit, 22050 Hz like the others):

* sizzle.wav: fat frying on a hot plate, a loop (1.6 seconds, seamless): a steady hiss of high noise with a crackle of tiny
  pops through it, each a short bright burst that dies in a few milliseconds, at random times and strengths. Played, looped,
  while a hotdog stands on a plate hot enough to burn, quieter the cooler the plate and the further it is.
* click.wav: the thermostat's switch, a small hard click with a short hollow knock after it (a tenth of a second). Played
  whenever a griddle is flipped, by anyone.

Run: python3 tools/build_griddle_sounds.py   (numpy and scipy). The output is the same every time (a fixed seed).
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


def seamless(x, fade):
    """Makes a loop of `x`: its last `fade` seconds are crossfaded into its first, so that the end runs into the start."""
    k = int(fade * RATE)
    head, tail = x[:k].copy(), x[-k:].copy()
    ramp = np.linspace(0, 1, k)
    out = x[:-k].copy()
    out[:k] = head * ramp + tail * (1 - ramp)
    return out


def sizzle(rng, seconds=1.6, fade=0.12):
    n = int((seconds + fade) * RATE)
    t = np.arange(n) / RATE
    # The hiss: noise through the upper middle, breathing a little.
    hiss = band(rng.standard_normal(n), 2500, 9500)
    hiss *= 0.55 + 0.25 * np.sin(2 * np.pi * 3.1 * t + 1.3) * np.sin(2 * np.pi * 1.7 * t)
    # A low bed of frying under it.
    bed = band(rng.standard_normal(n), 380, 1400) * 0.22
    # The crackle: pops at random times (about 70 a second), each a short burst of bright noise, loud or soft.
    pops = np.zeros(n)
    count = int(70 * (seconds + fade))
    for at in rng.uniform(0, seconds + fade - 0.03, count):
        length = int(rng.uniform(0.002, 0.009) * RATE)
        i = int(at * RATE)
        burst = rng.standard_normal(length) * np.exp(-np.arange(length) / (length / 3.0))
        pops[i:i + length] += burst * rng.uniform(0.15, 1.0) ** 2
    pops = band(pops, 1800, 10000) * 2.2
    out = hiss + bed + pops
    return seamless(out, fade)


def click(rng, seconds=0.11):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    # The click: a hard burst of the upper middle, over in about three milliseconds.
    tick = band(rng.standard_normal(n), 1500, 7000) * np.exp(-t / 0.0022)
    # The knock after it: a short hollow tone, falling a little.
    knock = np.sin(2 * np.pi * np.cumsum(260 - 90 * np.clip(t / 0.05, 0, 1)) / RATE) * np.exp(-t / 0.022) * (t > 0.004) * 0.5
    out = tick * 1.0 + knock
    out *= np.minimum(1, t / 0.0005) * np.minimum(1, (seconds - t) / 0.02)
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
    rng = np.random.default_rng(23)
    write('sizzle.wav', sizzle(rng), 0.7)
    write('click.wav', click(rng), 0.8)
