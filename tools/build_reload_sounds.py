#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the reload's three sounds into assets/audio (mono, 16-bit, 22050 Hz like the others):

* reload-out.wav: the magazine let go, a dry plastic click and the short scrape of the bottle sliding out of its well,
  with a soft wet pop as it comes free (about a fifth of a second).
* reload-in.wav: a fresh bottle pushed home, a scuff and then a hard, low clack as it seats, and a little squelch of
  sauce in it (about a fifth of a second).
* reload-rack.wav: the bolt or slide worked, two quick metallic clicks a tenth of a second apart, back and forward,
  the second the brighter (about a quarter of a second).

The game plays them as a reload passes its stages (src/reload.rs). Run: python3 tools/build_reload_sounds.py (numpy and
scipy). The output is the same every time (a fixed seed).
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


def click(rng, n, at, pitch, decay, width=0.35):
    """A sharp click at sample `at`: a burst of noise in a band about `pitch` Hz with a ringing partial, gone in `decay` s."""
    out = np.zeros(n)
    m = min(n - at, int(decay * 6 * RATE))
    t = np.arange(m) / RATE
    env = np.exp(-t / decay)
    burst = band(rng.standard_normal(m), pitch * (1 - width), pitch * (1 + width)) * env
    ring = np.sin(2 * np.pi * pitch * 1.37 * t) * env * 0.35
    out[at:at + m] = burst + ring
    return out


def scrape(rng, n, at, seconds, low, high):
    """A short scrape: band-passed noise swelling and fading over `seconds`."""
    out = np.zeros(n)
    m = min(n - at, int(seconds * RATE))
    t = np.arange(m) / RATE
    env = np.sin(np.pi * t / seconds) ** 1.5
    out[at:at + m] = band(rng.standard_normal(m), low, high) * env
    return out


def pop(n, at, pitch, seconds):
    """A wet pop: a sine whose pitch drops fast, fading quickly."""
    out = np.zeros(n)
    m = min(n - at, int(seconds * RATE))
    t = np.arange(m) / RATE
    f = pitch * np.exp(-t / (seconds * 0.4))
    out[at:at + m] = np.sin(2 * np.pi * np.cumsum(f) / RATE) * np.exp(-t / (seconds * 0.35))
    return out


def take_out(rng, seconds=0.2):
    n = int(seconds * RATE)
    return click(rng, n, 0, 2600, 0.006) * 0.9 + scrape(rng, n, int(0.012 * RATE), 0.11, 700, 2400) * 0.35 + pop(n, int(0.11 * RATE), 420, 0.06) * 0.45


def seat(rng, seconds=0.2):
    n = int(seconds * RATE)
    clack = click(rng, n, int(0.05 * RATE), 900, 0.012, 0.5) + click(rng, n, int(0.05 * RATE), 3100, 0.004)
    thump = pop(n, int(0.05 * RATE), 160, 0.07)
    squelch = band(rng.standard_normal(n), 300, 1200) * np.exp(-np.maximum(np.arange(n) / RATE - 0.06, 0) / 0.03) * (np.arange(n) / RATE > 0.06) * 0.25
    return scrape(rng, n, 0, 0.05, 900, 3000) * 0.3 + clack * 0.8 + thump * 0.7 + squelch


def rack(rng, seconds=0.26):
    n = int(seconds * RATE)
    back = click(rng, n, 0, 1900, 0.008) + click(rng, n, int(0.004 * RATE), 4200, 0.003) * 0.5
    slide = scrape(rng, n, int(0.01 * RATE), 0.08, 1500, 5000) * 0.25
    home = click(rng, n, int(0.11 * RATE), 2400, 0.01) + click(rng, n, int(0.112 * RATE), 5200, 0.004) * 0.7
    return back * 0.7 + slide + home


def write(name, x, peak):
    x = x / max(1e-9, np.max(np.abs(x))) * peak
    data = (np.clip(x, -1, 1) * 32767).astype('<i2').tobytes()
    with wave.open(os.path.join(OUT, name), 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data)


if __name__ == '__main__':
    rng = np.random.default_rng(20261118)
    write('reload-out.wav', take_out(rng), 0.55)
    write('reload-in.wav', seat(rng), 0.75)
    write('reload-rack.wav', rack(rng), 0.6)
