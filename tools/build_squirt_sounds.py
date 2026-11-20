#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Synthesises the squeeze-bottle sounds into assets/audio (mono, 16-bit, 22050 Hz like the others):

* squelch.wav: the mayonnaise bottle squeezed, a wet stream with bubbles popping in it and the bottle's air
  sputtering, about a second long and seamless, played on a loop for as long as the stream flows.
* squirt.wav: one squirt of the mustard pistol, a short wet "pfft" ending in a splutter.

Run: python3 tools/build_squirt_sounds.py   (numpy and scipy). The output is the same every time (a fixed seed).
"""
import os
import wave

import numpy as np
from scipy.signal import butter, lfilter

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'assets', 'audio')


def band(x, low, high, order=2):
    b, a = butter(order, [low / (RATE / 2), high / (RATE / 2)], btype='band')
    return lfilter(b, a, x)


def bubble(length, start, end, decay, rng):
    """A bubble popping: a sine whose pitch rises from `start` to `end` Hz as it dies away."""
    t = np.arange(int(length * RATE)) / RATE
    pitch = start + (end - start) * (t / t[-1]) ** 0.6
    phase = 2 * np.pi * np.cumsum(pitch) / RATE + rng.uniform(0, 2 * np.pi)
    attack = np.minimum(1, t / 0.002)
    return np.sin(phase) * np.exp(-t / decay) * attack


def place(out, sound, at):
    i = int(at * RATE)
    n = min(len(sound), len(out) - i)
    if n > 0:
        out[i:i + n] += sound[:n]


def squelch(rng, seconds=1.2, fade=0.12):
    n = int(seconds * RATE)
    noise = rng.standard_normal(n)
    # The stream: soft hiss in the middle of the range, swelling and easing as the hand squeezes.
    flow = band(noise, 350, 2200) * 0.32
    swell = 0.75 + 0.25 * np.sin(2 * np.pi * np.arange(n) / RATE * 3.1 + 0.7) * np.sin(2 * np.pi * np.arange(n) / RATE * 1.3)
    out = flow * swell
    # Thick wet gurgle under it: low noise, gated in irregular blobs.
    gurgle = band(rng.standard_normal(n), 90, 420) * 0.9
    gate = np.zeros(n)
    at = 0.0
    while at < seconds:
        blob = int(rng.uniform(0.03, 0.09) * RATE)
        i = int(at * RATE)
        shape = np.hanning(blob)[: max(0, min(blob, n - i))]
        gate[i:i + len(shape)] += shape * rng.uniform(0.4, 1.0)
        at += rng.uniform(0.04, 0.11)
    out += gurgle * gate
    # Bubbles popping in the stream, and now and then air spluttering out of the nozzle.
    at = 0.0
    while at < seconds:
        f = rng.uniform(380, 900)
        place(out, bubble(rng.uniform(0.025, 0.05), f, f * rng.uniform(1.6, 2.6), rng.uniform(0.006, 0.014), rng) * rng.uniform(0.25, 0.6), at)
        at += rng.exponential(0.045) + 0.012
    for _ in range(int(seconds * 3)):
        at = rng.uniform(0, seconds - 0.06)
        burst = band(rng.standard_normal(int(0.05 * RATE)), 600, 3200) * np.hanning(int(0.05 * RATE)) * 0.55
        place(out, burst, at)
    # Seamless: the last `fade` seconds are faded into the first.
    k = int(fade * RATE)
    ramp = np.linspace(0, 1, k)
    head = out[:k] * ramp + out[-k:] * (1 - ramp)
    return np.concatenate([head, out[k:-k]])


def squirt(rng, seconds=0.2):
    n = int(seconds * RATE)
    t = np.arange(n) / RATE
    # A puff of sauce through a nozzle: a noise burst whose band falls as the pressure goes, then a splutter.
    noise = rng.standard_normal(n)
    high = band(noise, 900, 3600) * np.exp(-t / 0.05) * np.minimum(1, t / 0.004)
    low = band(noise, 250, 1100) * np.exp(-((t - 0.05) / 0.045) ** 2)
    out = high * 0.7 + low * 0.9
    place(out, bubble(0.04, 520, 1250, 0.012, rng) * 0.5, 0.06)
    place(out, bubble(0.035, 700, 1500, 0.009, rng) * 0.35, 0.11)
    place(out, band(rng.standard_normal(int(0.03 * RATE)), 500, 2600) * np.hanning(int(0.03 * RATE)) * 0.4, 0.15)
    return out * np.minimum(1, (seconds - t) / 0.02)


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
    rng = np.random.default_rng(7)
    write('squelch.wav', squelch(rng), 0.55)
    write('squirt.wav', squirt(rng), 0.7)
