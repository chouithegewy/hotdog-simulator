#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the titan's sounds into assets/audio (mono, 16-bit, 22050 Hz like the others), played by src/titan/sound.rs
louder the nearer the listener is:

* titan-step.wav: a footfall. A deep boom that drops from 70 Hz to 32 Hz under a dull thud of low noise, a little
  rattle of things on the floor jumping, and the boom's tail (about 1 s).
* titan-breath.wav: its breathing. A slow wet in-breath of low noise rising, then a longer out-breath, lower, gurgling,
  with a thin hiss of steam over it (about 2.6 s).
* titan-roar.wav: the warning before its charge. A growl of steam: a rough low tone with a sub-octave under it, through
  shifting vowel-like bands, swelling, and a rising scream of steam over it (about 1.3 s).
* titan-smash.wav: both fists on the floor. A crack, a huge boom from 55 Hz to 26 Hz, tiles breaking (a scatter of
  short bright clicks), and a long rumble (about 1.8 s).
* titan-swat.wav: a great arm swept through the air: noise through a band that sweeps up and back down (about 0.6 s).
* titan-groan.wav: brought to its knees. A low, rough, falling moan through vowel-like bands, with steam hissing out
  of it (about 1.7 s).
* titan-hiss.wav: its weak spot hit. A sharp burst of steam and a high, sour squeal of the casing (about 0.8 s).
* titan-topple.wav: its fall. A long creaking groan rising as it tips, then an enormous thud and a rumble dying away
  (about 3.6 s).

None of it has been listened to by whoever made it (it was made by a program for a program): the levels are kept
conservative, each peak at about -7 dBFS after a soft clip, and the game plays them quieter still (sound.rs).

Run: python3 tools/build_titan_sounds.py   (numpy and scipy). The output is the same every time (a fixed seed).
"""
import os
import wave

import numpy as np
from scipy.signal import butter, lfilter

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'assets', 'audio')
PEAK = 0.45


def band(x, low, high, order=2):
    b, a = butter(order, [max(20, low) / (RATE / 2), min(high, RATE / 2 - 100) / (RATE / 2)], btype='band')
    return lfilter(b, a, x)


def lowpass(x, cut, order=2):
    b, a = butter(order, cut / (RATE / 2), btype='low')
    return lfilter(b, a, x)


def highpass(x, cut, order=2):
    b, a = butter(order, cut / (RATE / 2), btype='high')
    return lfilter(b, a, x)


def sweep(t, start, end, length):
    """A sine whose pitch slides from `start` to `end` Hz over `length` seconds (exponentially), then stays."""
    k = np.clip(t / length, 0, 1)
    pitch = start * (end / start) ** k
    return np.sin(2 * np.pi * np.cumsum(pitch) / RATE)


def envelope(t, attack, decay):
    return np.minimum(1, t / max(attack, 1e-4)) * np.exp(-t / decay)


def moving_band(x, freqs, width):
    """Noise `x` through a band whose middle follows `freqs` (one per sample), in short overlapping blocks."""
    n = len(x)
    out = np.zeros(n)
    block = 256
    window = np.hanning(block * 2)
    for start in range(0, n, block):
        s, e = max(0, start - block // 2), min(n, start + block * 3 // 2)
        f = freqs[min(n - 1, start + block // 2)]
        piece = band(x[s:e], f / (1 + width), f * (1 + width))
        w = window[: e - s] if e - s == block * 2 else np.hanning(e - s)
        out[s:e] += piece * w * 0.5
    return out


def vowels(x, formants):
    """`x` through a few resonant bands (a vowel), summed."""
    return sum(band(x, f * 0.85, f * 1.15) * g for f, g in formants)


def finish(x):
    """A gentle fade at the ends, a soft clip, and the peak set to PEAK."""
    x = np.asarray(x, dtype=np.float64)
    fade = min(len(x) // 10, int(0.03 * RATE))
    x[:64] *= np.linspace(0, 1, 64)
    x[-fade:] *= np.linspace(1, 0, fade)
    x = x / (np.max(np.abs(x)) + 1e-9)
    x = np.tanh(1.6 * x) / np.tanh(1.6)
    return x * PEAK


def step(rng, seconds=1.0):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    boom = sweep(t, 70, 32, 0.35) * envelope(t, 0.006, 0.28)
    thud = lowpass(rng.standard_normal(n), 220, 3) * envelope(t, 0.002, 0.06) * 2.5
    rattle = band(rng.standard_normal(n) * (rng.random(n) < 0.004), 1500, 5000) * envelope(t - 0.05, 0.01, 0.12) * (t > 0.05) * 3
    return finish(boom * 1.0 + thud + rattle)


def breath(rng, seconds=2.6):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    noise = rng.standard_normal(n)
    inhale = (t < 1.0) * np.sin(np.pi * np.clip(t / 1.0, 0, 1)) ** 2
    exhale = (t >= 0.95) * np.sin(np.pi * np.clip((t - 0.95) / 1.6, 0, 1)) ** 1.5
    rise = moving_band(noise, 260 + 220 * np.clip(t / 1.0, 0, 1), 0.6) * inhale
    out = moving_band(noise, 330 - 170 * np.clip((t - 0.95) / 1.6, 0, 1), 0.7) * exhale
    gurgle = (1 + 0.6 * np.sin(2 * np.pi * 9 * t + 2 * np.sin(2 * np.pi * 1.3 * t))) * lowpass(noise, 140) * exhale * 1.5
    hiss = highpass(noise, 3500) * exhale * 0.12
    return finish(rise * 0.8 + out + gurgle + hiss)


def growl(t, base, rng):
    """A rough voiced tone at `base` Hz (an array), with a sub-octave and a jittered pulse train."""
    jitter = 1 + 0.03 * lowpass(rng.standard_normal(len(t)), 30)
    phase = 2 * np.pi * np.cumsum(base * jitter) / RATE
    tone = np.sign(np.sin(phase)) * 0.4 + np.sin(phase) * 0.6 + 0.5 * np.sin(phase / 2)
    return tone


def roar(rng, seconds=1.3):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    swell = np.minimum(1, t / 0.25) * np.where(t < 1.0, 1, np.exp(-(t - 1.0) / 0.12))
    voice = vowels(growl(t, 72 + 10 * np.sin(2 * np.pi * 0.8 * t), rng), [(380, 1.0), (850, 0.6), (2400, 0.25)])
    steam = moving_band(rng.standard_normal(n), 1200 + 2800 * np.clip(t / 1.1, 0, 1), 0.5) * np.clip(t / 0.9, 0, 1) ** 1.5
    return finish((voice + steam * 0.7) * swell)


def smash(rng, seconds=1.8):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    crack = band(rng.standard_normal(n), 800, 6000) * envelope(t, 0.001, 0.02) * 2
    boom = sweep(t, 55, 26, 0.6) * envelope(t, 0.004, 0.5) * 1.3
    thump = lowpass(rng.standard_normal(n), 160, 3) * envelope(t, 0.002, 0.09) * 3
    clicks = (rng.random(n) < 0.012) * rng.standard_normal(n) * envelope(t - 0.02, 0.01, 0.25) * (t > 0.02)
    tiles = band(clicks, 2000, 7000) * 5
    rumble = lowpass(rng.standard_normal(n), 70, 3) * np.exp(-t / 0.7) * 2.5
    return finish(crack + boom + thump + tiles + rumble)


def swat(rng, seconds=0.6):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    k = t / seconds
    freqs = 300 + 900 * np.sin(np.pi * k) ** 2
    body = moving_band(rng.standard_normal(n), freqs, 0.6) * np.sin(np.pi * k) ** 2
    low = lowpass(rng.standard_normal(n), 160) * np.sin(np.pi * k) ** 3
    return finish(body + low * 0.8)


def groan(rng, seconds=1.7):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    pitch = 105 * (68 / 105) ** np.clip(t / 1.5, 0, 1)
    voice = vowels(growl(t, pitch, rng), [(320, 1.0), (620, 0.5), (2200, 0.15)])
    shape = np.minimum(1, t / 0.15) * np.where(t < 1.3, 1, np.exp(-(t - 1.3) / 0.15))
    hiss = highpass(rng.standard_normal(n), 3000) * shape * 0.15
    return finish(voice * shape + hiss)


def hiss(rng, seconds=0.8):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    burst = highpass(rng.standard_normal(n), 2500) * envelope(t, 0.005, 0.25)
    squeal = sweep(t, 1500, 900, 0.5) * envelope(t, 0.02, 0.18) * 0.35
    return finish(burst + squeal)


def topple(rng, seconds=3.6):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    fall = 2.2
    creak_pitch = 60 + 50 * np.clip(t / fall, 0, 1) ** 2
    creak = vowels(growl(t, creak_pitch, rng), [(250, 1.0), (700, 0.4)]) * np.clip(t / fall, 0, 1) * (t < fall) * 0.6
    after = np.maximum(0, t - fall)
    hit = (t >= fall)
    boom = sweep(after, 48, 22, 0.8) * envelope(after, 0.004, 0.7) * hit * 1.5
    thud = lowpass(rng.standard_normal(n), 180, 3) * envelope(after, 0.002, 0.12) * hit * 3
    rumble = lowpass(rng.standard_normal(n), 60, 3) * np.exp(-after / 0.9) * hit * 2.5
    return finish(creak + boom + thud + rumble)


def write(name, x):
    path = os.path.join(OUT, name)
    data = (np.clip(x, -1, 1) * 32767).astype('<i2')
    with wave.open(path, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data.tobytes())
    print(f'{path}: {len(x) / RATE:.2f} s, peak {np.max(np.abs(x)):.2f}, rms {np.sqrt(np.mean(x * x)):.3f}')


def main():
    rng = np.random.default_rng(1207)
    for name, make in [('titan-step.wav', step), ('titan-breath.wav', breath), ('titan-roar.wav', roar), ('titan-smash.wav', smash),
                       ('titan-swat.wav', swat), ('titan-groan.wav', groan), ('titan-hiss.wav', hiss), ('titan-topple.wav', topple)]:
        write(name, make(rng))


if __name__ == '__main__':
    main()
