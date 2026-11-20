#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises Kung Fu's sounds into assets/audio (mono, 16-bit, 22050 Hz like the others). Nothing is sampled: every
sound is noise, sines and envelopes, the same every run (a fixed seed). src/fu.rs plays them on the fight's events
(some of them with, or instead of, the recorded sounds that tools/build_kungfu_user_sounds.py cuts: see src/fu.rs).

* fu-whoosh.wav, fu-whoosh-heavy.wav: a hand or a foot through the air. The light one short and high, the heavy one
  longer, lower and fuller. Played as a strike starts.
* fu-hit.wav, fu-hit-heavy.wav: a blow landing on a hotdog: a padded slap with a short knock under it; the heavy one a
  deep body thud with a crack on top. (Softer than the melee's thwack: these come many to a second.)
* fu-block.wav: a blow taken on the arms: a dull, damped thud, no crack.
* fu-parry.wav: a parry: a hard, bright wooden clack, two sticks struck, ringing a moment.
* fu-guardbreak.wav: a broken guard: a splintering crack over a thud, a little longer than a hit.
* fu-grab.wav: a grab: a quick cloth-and-skin rustle that tightens.
* fu-throw.wav: a thrown hotdog landing on the counter: a thump, a smaller bounce, a slide.
* fu-gong.wav: a knockout: the wok gong, inharmonic partials that beat and fade over two and a half seconds.
* fu-chi.wav: chi full: a small bright chime, two bell partials a fifth apart.
* fu-style.wav: a change of style: a short plucked flourish of three notes up a pentatonic scale.
* fu-clang.wav: a strike bouncing off a Toad's iron body: a metallic clang with a hard edge and a short ring.

Levels: each is normalised to a peak chosen for its job and then checked for loudness (RMS over its loudest 50 ms), so
the many small sounds (whooshes, hits) sit under the rare big ones (the gong, the chime). They have not been listened
to by whoever made them: they are deliberately conservative (no peak above 0.8, nothing harsh above 6 kHz), and fu.rs
plays them at modest volumes.

Run: python3 tools/build_kungfu_sounds.py   (numpy and scipy).
"""
import os
import wave

import numpy as np
from scipy.signal import butter, lfilter

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'assets', 'audio')


def t_of(seconds):
    return np.arange(int(seconds * RATE)) / RATE


def band(x, low, high, order=2):
    b, a = butter(order, [max(20, low) / (RATE / 2), min(high, RATE / 2 - 100) / (RATE / 2)], btype='band')
    return lfilter(b, a, x)


def lowpass(x, f, order=2):
    b, a = butter(order, f / (RATE / 2), btype='low')
    return lfilter(b, a, x)


def fades(x, attack=0.002, release=0.02):
    n = len(x)
    t = np.arange(n) / RATE
    return x * np.minimum(1, t / max(attack, 1e-4)) * np.minimum(1, (n / RATE - t) / max(release, 1e-4))


def sweep(noise, start, end, width):
    """Noise through a band whose middle slides from `start` to `end` Hz, in overlapping blocks."""
    out = np.zeros_like(noise)
    blocks = 20
    edges = np.linspace(0, len(noise), blocks + 1).astype(int)
    for i in range(blocks):
        f = start * (end / start) ** (i / (blocks - 1))
        a, b = edges[i], edges[i + 1]
        s, e = max(0, a - 80), min(len(noise), b + 80)
        piece = band(noise[s:e], f * (1 - width), f * (1 + width))
        ramp = np.ones(e - s)
        k = min(80, e - s)
        if s < a:
            ramp[:k] = np.linspace(0, 1, k)
        if e > b:
            ramp[-k:] = np.linspace(1, 0, k)
        out[s:e] += piece * ramp
    return out


def whoosh(rng, seconds, low, high, body):
    t = t_of(seconds)
    air = sweep(rng.standard_normal(len(t)), low, high, 0.5)
    shape = np.sin(np.pi * np.clip(t / seconds, 0, 1) ** 0.7) ** 2
    out = air * shape
    if body:
        # A heavy limb moves air low down too.
        out += lowpass(rng.standard_normal(len(t)), 260) * shape * body
    return fades(out, 0.01, 0.04)


def knock(t, f0, f1, decay):
    """A pitched thump falling from f0 to f1 Hz."""
    pitch = f1 + (f0 - f1) * np.exp(-t / 0.025)
    return np.sin(2 * np.pi * np.cumsum(pitch) / RATE) * np.exp(-t / decay)


def hit(rng, heavy):
    seconds = 0.26 if heavy else 0.15
    t = t_of(seconds)
    n = len(t)
    thump = knock(t, 170 if heavy else 240, 60 if heavy else 110, 0.06 if heavy else 0.03)
    slap = band(rng.standard_normal(n), 500, 2600) * np.exp(-t / (0.018 if heavy else 0.012))
    out = thump * (1.0 if heavy else 0.7) + slap * (0.55 if heavy else 0.8)
    if heavy:
        out += band(rng.standard_normal(n), 1500, 4500) * np.exp(-t / 0.006) * 0.45
    return fades(out, 0.0008, 0.03)


def block(rng):
    t = t_of(0.16)
    thud = knock(t, 150, 80, 0.035)
    pad = lowpass(rng.standard_normal(len(t)), 900) * np.exp(-t / 0.02)
    return fades(thud + pad * 0.5, 0.001, 0.03)


def parry(rng):
    t = t_of(0.3)
    n = len(t)
    click = band(rng.standard_normal(n), 1800, 5000) * np.exp(-t / 0.004)
    # Two hardwood sticks ring at their own pitches for a moment.
    ring = sum(np.sin(2 * np.pi * f * t) * np.exp(-t / d) * a for f, d, a in [(1180, 0.05, 0.5), (1730, 0.035, 0.35), (2650, 0.02, 0.2)])
    second = np.zeros(n)
    k = int(0.012 * RATE)
    second[k:] = (band(rng.standard_normal(n - k), 1500, 4000) * np.exp(-t[: n - k] / 0.003)) * 0.6
    return fades(click * 0.8 + ring + second, 0.0005, 0.05)


def guardbreak(rng):
    t = t_of(0.34)
    n = len(t)
    thud = knock(t, 160, 55, 0.07)
    # Splinters: a few cracks close together.
    crack = np.zeros(n)
    for start, size in [(0.0, 1.0), (0.018, 0.7), (0.041, 0.5), (0.07, 0.3)]:
        k = int(start * RATE)
        crack[k:] += band(rng.standard_normal(n - k), 1200, 5200) * np.exp(-t[: n - k] / 0.01) * size
    return fades(thud * 0.9 + crack * 0.7, 0.0008, 0.05)


def grab(rng):
    t = t_of(0.22)
    n = len(t)
    rustle = band(rng.standard_normal(n), 900, 4000) * (np.sin(np.pi * np.clip(t / 0.22, 0, 1)) ** 1.5)
    # It tightens: a quick squeeze of lower noise at the end.
    squeeze = band(rng.standard_normal(n), 250, 900) * np.exp(-np.maximum(0, 0.16 - t) / 0.02) * np.exp(-np.maximum(0, t - 0.16) / 0.03)
    return fades(rustle * 0.6 + squeeze * 0.8, 0.01, 0.03)


def throw(rng):
    t = t_of(0.5)
    n = len(t)
    out = np.zeros(n)
    for start, size, f0 in [(0.0, 1.0, 140), (0.13, 0.4, 170)]:
        k = int(start * RATE)
        tt = t[: n - k]
        out[k:] += (knock(tt, f0, 55, 0.07) + band(rng.standard_normal(n - k), 300, 1800) * np.exp(-tt / 0.02) * 0.5) * size
    # Sliding to a stop.
    k = int(0.2 * RATE)
    out[k:] += band(rng.standard_normal(n - k), 700, 3000) * np.exp(-t[: n - k] / 0.09) * 0.12
    return fades(out, 0.001, 0.08)


def gong(rng):
    seconds = 2.6
    t = t_of(seconds)
    n = len(t)
    # A wok is no tuned gong: inharmonic partials, a pair of close ones beating, the high ones dying first.
    partials = [(196, 2.2, 1.0), (199.5, 2.0, 0.55), (291, 1.4, 0.5), (412, 1.0, 0.45), (517, 0.8, 0.3), (688, 0.55, 0.25), (941, 0.35, 0.18), (1287, 0.22, 0.12)]
    out = sum(np.sin(2 * np.pi * f * t + rng.uniform(0, 6.28)) * np.exp(-t / d) * a for f, d, a in partials)
    # The spoon's strike: a soft knock, and the swell of the low partials just after it.
    out += knock(t, 260, 120, 0.02) * 0.6
    out *= 1 - 0.35 * np.exp(-t / 0.05)
    return fades(out, 0.002, 0.4)


def chime(rng):
    t = t_of(0.9)
    out = sum(np.sin(2 * np.pi * f * t) * np.exp(-t / d) * a for f, d, a in [(1568, 0.45, 1.0), (2349, 0.3, 0.5), (3136, 0.18, 0.25), (4709, 0.08, 0.1)])
    return fades(out, 0.002, 0.15)


def pluck(t, f, decay):
    """A plucked string: a few harmonics, the higher ones dying first, a soft bend down as it starts."""
    bend = f * (1 + 0.012 * np.exp(-t / 0.02))
    phase = 2 * np.pi * np.cumsum(bend) / RATE
    return sum(np.sin(k * phase) * np.exp(-t * k / decay) / k ** 1.2 for k in range(1, 6))


def sting(rng):
    seconds = 0.55
    t = t_of(seconds)
    n = len(t)
    out = np.zeros(n)
    # Up a pentatonic scale: D, E, A (an octave of the first an instant later as the last rings).
    for start, f, size in [(0.0, 587.3, 0.7), (0.07, 659.3, 0.8), (0.14, 880.0, 1.0)]:
        k = int(start * RATE)
        out[k:] += pluck(t[: n - k], f, 0.35) * size
    return fades(out, 0.001, 0.12)


def clang(rng):
    t = t_of(0.6)
    n = len(t)
    edge = band(rng.standard_normal(n), 1500, 5500) * np.exp(-t / 0.006)
    ring = sum(np.sin(2 * np.pi * f * t) * np.exp(-t / d) * a for f, d, a in [(523, 0.3, 0.6), (1187, 0.22, 0.8), (1913, 0.14, 0.55), (2771, 0.08, 0.35), (3517, 0.05, 0.2)])
    thud = knock(t, 200, 90, 0.03) * 0.5
    return fades(edge * 0.6 + ring + thud, 0.0005, 0.08)


def loudest(x, window=0.05):
    k = int(window * RATE)
    if len(x) <= k:
        return float(np.sqrt(np.mean(x ** 2)))
    energy = np.convolve(x ** 2, np.ones(k) / k, mode='valid')
    return float(np.sqrt(energy.max()))


def write(name, x, peak):
    x = x / np.max(np.abs(x)) * peak
    data = (x * 32767).astype('<i2').tobytes()
    with wave.open(os.path.join(OUT, name), 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data)
    print(f'{name}: {len(x) / RATE:.2f} s, peak {peak:.2f}, loudest 50 ms RMS {loudest(x):.3f}')


# Peak of each: the whooshes and hits under the rest; the gong, the knockout, the loudest.
SOUNDS = [
    ('fu-whoosh.wav', lambda rng: whoosh(rng, 0.17, 900, 4200, 0), 0.32),
    ('fu-whoosh-heavy.wav', lambda rng: whoosh(rng, 0.3, 380, 2600, 0.35), 0.4),
    ('fu-hit.wav', lambda rng: hit(rng, False), 0.5),
    ('fu-hit-heavy.wav', lambda rng: hit(rng, True), 0.62),
    ('fu-block.wav', block, 0.5),
    ('fu-parry.wav', parry, 0.5),
    ('fu-guardbreak.wav', guardbreak, 0.62),
    ('fu-grab.wav', grab, 0.42),
    ('fu-throw.wav', throw, 0.62),
    ('fu-gong.wav', gong, 0.75),
    ('fu-chi.wav', chime, 0.42),
    ('fu-style.wav', sting, 0.38),
    ('fu-clang.wav', clang, 0.5),
]

if __name__ == '__main__':
    rng = np.random.default_rng(27)
    for name, make, peak in SOUNDS:
        write(name, make(rng), peak)
