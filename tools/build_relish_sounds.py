#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the relish bazooka's burst into assets/audio (mono, 16-bit, 22050 Hz like the others). Both are layered
over boom.wav where a glob bursts (src/arena/splatter/boom.rs), louder the nearer the listener is:

* relish-splat.wav: the glob bursting wet. A hard pop, a low wet slosh that drops in pitch as the relish flies apart,
  squelching bubbles, and then the patter of globs and pickle chunks landing all round, thinning out over half a second
  (about 0.9 s in all).
* relish-thump.wav: the body of the blast felt up close: a deep thump that drops from 120 Hz to under 40 Hz with a low
  rumble behind it, a little saturated so that small speakers still give some of it (about 0.7 s). It is played only
  near the burst, so the nearer the listener the bassier the whole.

Run: python3 tools/build_relish_sounds.py   (numpy and scipy). The output is the same every time (a fixed seed).
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


def lowpass(x, cut, order=2):
    b, a = butter(order, cut / (RATE / 2), btype='low')
    return lfilter(b, a, x)


def chirp(t, start, end, length):
    """A sine whose pitch slides from `start` to `end` Hz over `length` seconds (and stays there)."""
    pitch = start + (end - start) * np.clip(t / length, 0, 1)
    return np.sin(2 * np.pi * np.cumsum(pitch) / RATE)


def splat(rng, seconds=0.92):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    # The pop: a hard burst of the whole band, over in a few hundredths of a second.
    pop = band(rng.standard_normal(n), 90, 3800) * np.exp(-t / 0.018) * np.minimum(1, t / 0.0008)
    # The slosh: low noise through a band that falls from 900 to 180 Hz as the relish flies apart.
    noise = rng.standard_normal(n)
    slosh = np.zeros(n)
    blocks = 20
    edges = np.linspace(0, int(0.42 * RATE), blocks + 1).astype(int)
    for i in range(blocks):
        f = 900 * (180 / 900) ** (i / (blocks - 1))
        a, b = edges[i], edges[i + 1]
        s, e = max(0, a - 60), min(n, b + 60)
        piece = band(noise[s:e], max(60, f * 0.5), f * 1.6)
        ramp = np.ones(e - s)
        k = min(60, e - s)
        if s < a:
            ramp[:k] = np.linspace(0, 1, k)
        if e < n:
            ramp[-k:] = np.linspace(1, 0, k)
        slosh[s:e] += piece * ramp
    slosh *= np.exp(-t / 0.14) * np.minimum(1, t / 0.004)
    # Squelching bubbles: a few rising chirps in the first fifth of a second.
    bubbles = np.zeros(n)
    for k in range(5):
        at = 0.015 + 0.035 * k + 0.01 * rng.random()
        u = np.clip(t - at, 0, None)
        length = 0.025 + 0.02 * rng.random()
        bubbles += chirp(u, 260 + 120 * rng.random(), 700 + 500 * rng.random(), length) * np.exp(-u / (length * 0.6)) * (t > at) * (0.6 + 0.4 * rng.random())
    # The patter: globs and chunks landing all round, thick at first and thinning out, each a wet tick of its own pitch.
    patter = np.zeros(n)
    for k in range(70):
        at = 0.06 + 0.62 * rng.random() ** 1.7
        start = int(at * RATE)
        size = 0.004 + 0.012 * rng.random()
        m = min(n - start, int(size * 5 * RATE))
        if m <= 8:
            continue
        u = np.arange(m) / RATE
        centre = 500 + 2200 * rng.random()
        tick = band(rng.standard_normal(m), centre * 0.6, centre * 1.5) * np.exp(-u / size)
        patter[start:start + m] += tick * (0.35 + 0.65 * rng.random()) * (1.1 - at)
    out = pop * 0.75 + slosh * 1.5 + bubbles * 0.16 + patter * 0.6
    out *= np.minimum(1, t / 0.0005) * np.minimum(1, (seconds - t) / 0.08)
    return out


def thump(rng, seconds=0.7):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    # The thump: a sine that drops from 120 Hz to 38 Hz, dying over a fifth of a second.
    pitch = 38 + 82 * np.exp(-t / 0.05)
    body = np.sin(2 * np.pi * np.cumsum(pitch) / RATE) * np.exp(-t / 0.2) * np.minimum(1, t / 0.003)
    # The rumble behind it: low noise, slower to die.
    rumble = lowpass(rng.standard_normal(n), 140, 3) * np.exp(-t / 0.3) * np.minimum(1, t / 0.02)
    rumble /= np.max(np.abs(rumble))
    out = body + rumble * 0.45
    # A little saturation, for harmonics a small speaker can give.
    out = np.tanh(out * 1.8) / np.tanh(1.8)
    out *= np.minimum(1, (seconds - t) / 0.12)
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
    write('relish-splat.wav', splat(rng), 0.85)
    write('relish-thump.wav', thump(rng), 0.9)
