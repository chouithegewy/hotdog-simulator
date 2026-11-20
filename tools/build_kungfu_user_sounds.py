#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Cuts the royalty-free kung fu recordings in assets/audio/*.mp3 into game-ready one-shots in assets/audio/kungfu/.

The recordings themselves are not in the repository (THIRD_PARTY_NOTICES.md lists them by Pixabay id): download them
into assets/audio/ under the names in FILES below before running this.

The originals are never touched. Every output is a mono 16-bit 22050 Hz wave, like the rest of the game's sounds (the
client's Bevy has the `wav` feature and nothing else, so no mp3 or ogg is played, native or in the browser). src/fu.rs
says which file is played for what (its `POOLS`, `LOOP` and the stings); a test there checks every file named exists,
loads, and that none in this folder is left unused.

What each source holds (found by looking at its envelope and spectrogram; nothing here was listened to):

* kung-fu-punch-4: a swing's rising noise (0.15 to 0.38 s) and then one punch (0.39 s): cut into swing.wav and hit-punch.wav.
* big-punch-with-whoosh: a slow 0.6 s wind-up and one heavy punch at 0.75 s: the punch is hit-heavy.wav.
* big-punch-short-with-male-moan: one dull punch and a short exhale ("oof") under it: grunt.wav.
* kung-fu-clothes-hits-and-clothing-sounds: eight dull body thumps (95% of the energy under 100 Hz) in 16 s of hush, no
  cloth whooshes worth the name: thud-1 to thud-8.wav, found by detecting each onset.
* kung-fu-yell: three shouts, the last two joined by a held vowel: kiai-1 to kiai-4.wav.
* kung-fu-scream: five separate male screams between stretches of hiss: scream-1 to scream-5.wav.
* karate: not a punch: a held, steady 266 Hz harmonic tone, voice-like, 1.5 s: special.wav (its first 0.8 s).
* wu-swords-hip-hopc (and its byte-identical " (1)" copy): not music, no beat or bass: a burst of fast metallic clicks
  between 1 and 4 kHz for three seconds: rattle.wav (its first burst).
* kung-fu-in-bamboo-woods: a quiet bamboo-and-air bed with four knocks 1.25 s apart: ambience-dojo.wav, a seamless
  loop (equal-power crossfade of the end into the start, both quiet bed).
* asiatic-intro: ticks, a rising swell and a deep boom: sting-start.wav (from the swell).
* chinese-ident-transition-3: a boom, then climbing plucks: sting-win.wav, and two plucks of it: sting-style.wav.
* chinese-ident-transition-1: a boom, plucks, a second boom: sting-end.wav.

Treatment: high-pass (the recordings carry a lot of inaudible rumble that only eats headroom), cut at the onset with a
few ms of lead-in and where the decay is 35 to 45 dB down, fades so nothing clicks, then the gain: each file is set to a
target loudness (K-weighted per ITU-R BS.1770, loudest 100 ms for one-shots, loudest 400 ms for the stings, the whole
loop for the ambience) and then held under 0.89 of full scale (-1 dBFS) if that target would have clipped.

Run: python3 tools/build_kungfu_user_sounds.py [--check]   (numpy, scipy and ffmpeg; --check also asks ffmpeg's ebur128
for a second opinion on every output's loudness).
"""
import argparse
import hashlib
import os
import re
import subprocess
import sys
import wave

import numpy as np
from scipy.signal import butter, lfilter, sosfilt

RATE = 22050
HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, '..', 'assets', 'audio')
OUT = os.path.join(SRC, 'kungfu')
PEAK_MAX = 0.89

FILES = {
    'punch': 'freesound_community-kung-fu-punch-4-105262.mp3',
    'bigpunch': 'freesound_community-big-punch-with-whoosh-103638.mp3',
    'moan': 'freesound_community-big-punch-short-with-male-moan-83735.mp3',
    'clothes': 'freesound_community-kung-fu-clothes-hits-and-clothing-sounds-73455.mp3',
    'yell': 'freesound_community-kung-fu-yell-6314.mp3',
    'scream': 'freesound_community-kung-fu-scream-43668.mp3',
    'karate': 'freesound_community-karate-100238.mp3',
    'swords': 'freesound_community-wu-swords-hip-hopc-83035.mp3',
    'bamboo': 'pixelpond-kung-fu-in-bamboo-woods-361785.mp3',
    'intro': 'djartmusic-asiatic-intro_1-283723.mp3',
    'ident1': 'djartmusic-chinese-ident-transition-1-283708.mp3',
    'ident3': 'djartmusic-chinese-ident-transition-3-283709.mp3',
}
DUPLICATE = ('freesound_community-wu-swords-hip-hopc-83035 (1).mp3', FILES['swords'])


# ----- Measuring.

def kweight(x, fs=RATE):
    """ITU-R BS.1770 K-weighting: a high shelf and a 38 Hz high-pass."""
    gain, q, f0 = 3.999843853973347, 0.7071752369554196, 1681.974450955533
    a_, w0 = 10 ** (gain / 40), 2 * np.pi * f0 / fs
    al, c = np.sin(w0) / (2 * q), np.cos(w0)
    s = 2 * np.sqrt(a_) * al
    b = np.array([a_ * ((a_ + 1) + (a_ - 1) * c + s), -2 * a_ * ((a_ - 1) + (a_ + 1) * c), a_ * ((a_ + 1) + (a_ - 1) * c - s)])
    a = np.array([(a_ + 1) - (a_ - 1) * c + s, 2 * ((a_ - 1) - (a_ + 1) * c), (a_ + 1) - (a_ - 1) * c - s])
    y = lfilter(b / a[0], a / a[0], x)
    f1, q1 = 38.13547087602444, 0.5003270373238773
    k = np.tan(np.pi * f1 / fs)
    d = 1 + k / q1 + k * k
    return lfilter([1, -2, 1], [1, 2 * (k * k - 1) / d, (1 - k / q1 + k * k) / d], y)


def loudness(x, window=None, fs=RATE):
    """LUFS-style loudness of `x`: over its loudest `window` seconds, or all of it."""
    y = kweight(x, fs)
    if window is None:
        ms = float(np.mean(y * y))
    else:
        n = max(1, int(window * fs))
        if len(y) < n:
            y = np.pad(y, (0, n - len(y)))
        ms = float(np.convolve(y * y, np.ones(n) / n, mode='valid').max())
    return -0.691 + 10 * np.log10(ms + 1e-12)


def env_db(x, win=0.01):
    n = max(1, int(RATE * win))
    m = len(x) // n
    return 20 * np.log10(np.sqrt((x[:m * n].reshape(m, n) ** 2).mean(axis=1)) + 1e-9), win


# ----- Reading.

def decode(path):
    """The file as mono float at 22050 Hz (ffmpeg downmixes and resamples)."""
    out = subprocess.run(['ffmpeg', '-v', 'error', '-i', path, '-ac', '1', '-ar', str(RATE), '-f', 'f32le', '-'], capture_output=True, check=True).stdout
    return np.frombuffer(out, dtype=np.float32).astype(np.float64)


def stereo_check(path):
    """Correlation of a stereo file's channels (a mono sum of a wide mix can cancel)."""
    out = subprocess.run(['ffmpeg', '-v', 'error', '-i', path, '-ar', str(RATE), '-f', 'f32le', '-'], capture_output=True, check=True).stdout
    probe = subprocess.run(['ffprobe', '-v', 'error', '-select_streams', 'a:0', '-show_entries', 'stream=channels', '-of', 'csv=p=0', path], capture_output=True, check=True, text=True).stdout.strip()
    if probe != '2':
        return None
    s = np.frombuffer(out, dtype=np.float32).astype(np.float64).reshape(-1, 2)
    return float(np.corrcoef(s[:, 0], s[:, 1])[0, 1])


def highpass(x, f, order=4):
    return sosfilt(butter(order, f, btype='high', fs=RATE, output='sos'), x)


# ----- Finding things.

def onset(x, lo, hi, win=0.002, span=3):
    """The time (s) of the steepest rise of the level between `lo` and `hi` seconds."""
    e, w = env_db(x, win)
    a, b = int(lo / w), int(hi / w)
    rise = e[a + span:b] - e[a:b - span]
    return (a + int(np.argmax(rise))) * w


def attack(x, t, back=0.012, span=0.04, share=0.08):
    """The time (s) a click or knock really starts: the first sample within `back` s before and `span` s after the rough
    onset `t` that is `share` of the loudest in that stretch."""
    a, b = max(0, int((t - back) * RATE)), int((t + span) * RATE)
    seg = np.abs(x[a:b])
    return (a + int(np.argmax(seg > share * seg.max()))) / RATE


def rise_start(x, lo, hi, drop=8, win=0.01):
    """Where the final rise to the loudest frame between `lo` and `hi` s begins: walking back from that frame, the first
    time the level is `drop` dB under it. (For a punch that grows out of a wind-up rather than starting cold.)"""
    e, w = env_db(x, win)
    a, b = int(lo / w), int(hi / w)
    i = a + int(np.argmax(e[a:b]))
    while i > a and e[i] > e[a + int(np.argmax(e[a:b]))] - drop:
        i -= 1
    return i * w


def peak_at(x, t0, span, win=0.005):
    """(time, dB) of the loudest frame within `span` s of t0."""
    e, w = env_db(x, win)
    a = int(t0 / w)
    b = min(len(e), a + max(2, int(span / w)))
    i = a + int(np.argmax(e[a:b]))
    return i * w, float(e[i])


def fall_to(x, t_peak, level, below, cap, win=0.005):
    """The first time after `t_peak` that the level is `below` dB under `level`, but no later than `cap` s after it."""
    e, w = env_db(x, win)
    i = int(t_peak / w)
    for j in range(i, min(len(e), i + int(cap / w))):
        if e[j] < level - below:
            return j * w
    return t_peak + cap


def regions(x, below, gap, minlen=0.08, win=0.01):
    """Stretches louder than `below` dB under the loudest frame, joined across gaps under `gap` s (silence detection)."""
    e, w = env_db(x, win)
    on = e > e.max() - below
    found, i = [], 0
    while i < len(on):
        if on[i]:
            j = i
            while j < len(on) and on[j]:
                j += 1
            found.append([i, j])
            i = j
        else:
            i += 1
    merged = []
    for a, b in found:
        if merged and (a - merged[-1][1]) * w < gap:
            merged[-1][1] = b
        else:
            merged.append([a, b])
    return [(a * w, b * w) for a, b in merged if (b - a) * w >= minlen]


def trim_voice(x, a, b, lead=0.02, tail=0.03):
    """A found stretch with a little breath before and after it."""
    return max(0., a - lead), min(len(x) / RATE, b + tail)


# ----- Making.

def compress(x, threshold_db, ratio, attack=0.004, release=0.12):
    """A plain feed-forward compressor, to bring a bed's knocks nearer the bed."""
    level = np.abs(x)
    a, r = np.exp(-1 / (attack * RATE)), np.exp(-1 / (release * RATE))
    smooth, out = 0., np.empty_like(level)
    for i, v in enumerate(level):
        smooth = (a if v > smooth else r) * smooth + (1 - (a if v > smooth else r)) * v
        out[i] = smooth
    over = np.maximum(20 * np.log10(out + 1e-9) - threshold_db, 0)
    return x * 10 ** (-over * (1 - 1 / ratio) / 20)


def fade(x, fade_in, fade_out):
    y = x.copy()
    n = len(y)
    i, o = min(n, int(fade_in * RATE)), min(n, int(fade_out * RATE))
    if i:
        y[:i] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, i))
    if o:
        y[n - o:] *= 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, o))
    return y


class Writer:
    """Collects the clips, writes each as a 16-bit wave and keeps the numbers for the table."""

    def __init__(self, out):
        self.out, self.rows = out, []
        os.makedirs(out, exist_ok=True)

    def add(self, name, x, source, what, target, window=0.1, fade_in=0.002, fade_out=0.03, note=''):
        """`target` LUFS over the loudest `window` s (None: over the whole clip), held under PEAK_MAX."""
        x = fade(x, fade_in, fade_out)
        loud = loudness(x, window)
        gain = 10 ** ((target - loud) / 20)
        limited = ''
        peak = np.abs(x).max()
        if peak * gain > PEAK_MAX:
            gain, limited = PEAK_MAX / peak, ' (peak-limited)'
        y = np.clip(x * gain, -1, 1)
        pcm = np.round(y * 32767).astype('<i2')
        path = os.path.join(self.out, name)
        with wave.open(path, 'wb') as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(pcm.tobytes())
        self.rows.append(dict(name=name, source=source, what=what, secs=len(y) / RATE, peak=float(np.abs(y).max()), l100=loudness(y, 0.1), l400=loudness(y, 0.4), lall=loudness(y), size=os.path.getsize(path), limited=limited, target=target, window=window, note=note))


def build(out):
    for p in (DUPLICATE[0],):
        a, b = os.path.join(SRC, p), os.path.join(SRC, DUPLICATE[1])
        same = hashlib.sha256(open(a, 'rb').read()).hexdigest() == hashlib.sha256(open(b, 'rb').read()).hexdigest()
        print(f'{p}: {"byte-identical to" if same else "DIFFERENT from"} {DUPLICATE[1]}; only the latter is used')
    src = {k: decode(os.path.join(SRC, v)) for k, v in FILES.items()}
    wr = Writer(out)
    secs = lambda k: len(src[k]) / RATE

    # --- Punches. Targets are LUFS over the loudest 100 ms; the synthesised hits they join sit at -17 to -22.
    x = highpass(src['punch'], 60)
    t0 = attack(x, onset(x, 0.30, 0.50))
    tp, lp = peak_at(x, t0, 0.05)
    end = fall_to(x, tp, lp, 40, 0.30)
    wr.add('hit-punch.wav', x[int((t0 - 0.003) * RATE):int(end * RATE)], 'punch', 'the punch (a dull thump with a crack on top)', -17, fade_in=0.001, fade_out=0.03)
    # The swing before it: noise rising 0.15 to 0.38 s, up to the punch. Cut off 4 ms before the impact, fading out over 25 ms.
    e, w = env_db(x, 0.005)
    pre = e[:int(t0 / w)]
    s0 = (int(np.argmax(pre > pre.max() - 38))) * w
    wr.add('swing.wav', x[int(s0 * RATE):int((t0 - 0.003) * RATE)], 'punch', 'the swing that leads into the punch (rising air)', -20, fade_in=0.02, fade_out=0.025, note=f'{s0:.2f} to {t0 - 0.003:.2f} s')

    x = highpass(src['bigpunch'], 60)
    # (The punch grows out of a long rising rumble: it is the last 5 dB of that rise that is the blow.)
    t0 = attack(x, rise_start(x, 0.5, 0.95, drop=5))
    tp, lp = peak_at(x, t0, 0.08)
    end = fall_to(x, tp, lp, 40, 0.45)
    wr.add('hit-heavy.wav', x[int((t0 - 0.003) * RATE):int(end * RATE)], 'bigpunch', 'the heavy punch after the wind-up (the wind-up is left out)', -15, fade_in=0.001, fade_out=0.05, note=f'blow from {t0:.2f} s')

    x = highpass(src['moan'], 60)
    t0 = attack(x, onset(x, 0.05, 0.2))
    end = fall_to(x, t0 + 0.05, peak_at(x, t0, 0.3)[1], 42, 0.4)
    wr.add('grunt.wav', x[int((t0 - 0.003) * RATE):int(end * RATE)], 'moan', 'a dull punch and a short exhale under it', -19, fade_in=0.002, fade_out=0.06, note=f'{t0:.2f} to {end:.2f} s')

    # --- The clothes file's thumps, found one by one: an onset, then the decay to 40 dB under its peak.
    x = highpass(src['clothes'], 70)
    found = regions(highpass(src['clothes'], 80), 30, 0.05, minlen=0.02, win=0.005)
    assert len(found) == 8, f'expected eight thumps in the clothes file, found {found}'
    for i, (a, b) in enumerate(found, 1):
        t0 = attack(x, onset(x, max(0, a - 0.03), a + 0.06, win=0.002))
        tp, lp = peak_at(x, t0, 0.05)
        end = fall_to(x, tp, lp, 38, 0.22)
        wr.add(f'thud-{i}.wav', x[int((t0 - 0.003) * RATE):int(end * RATE)], 'clothes', f'body thump {i} of 8 at {t0:.2f} s', -21, fade_in=0.001, fade_out=0.03, note=f'{t0:.2f} to {end:.2f} s')

    # --- Voices.
    x = highpass(src['yell'], 120)
    # (26 dB under the loudest frame: the quiet held vowel between the last two shouts then counts as the gap it is.)
    parts = regions(x, 26, 0.1)
    assert len(parts) == 4, f'expected four shouts in the yell file, found {parts}'
    for i, (a, b) in enumerate(parts, 1):
        a, b = trim_voice(x, a, b)
        b = min(b, a + 0.8)
        wr.add(f'kiai-{i}.wav', x[int(a * RATE):int(b * RATE)], 'yell', f'shout {i} of 4', -16, window=0.1, fade_in=0.004, fade_out=0.15 if b - a > 0.5 else 0.06, note=f'{a:.2f} to {b:.2f} s')

    x = highpass(src['scream'], 120)
    found = regions(x, 30, 0.15)
    assert len(found) == 5, f'expected five screams, found {found}'
    for i, (a, b) in enumerate(found, 1):
        a, b = trim_voice(x, a, b, 0.03, 0.05)
        wr.add(f'scream-{i}.wav', x[int(a * RATE):int(b * RATE)], 'scream', f'scream {i} of 5', -17, fade_in=0.006, fade_out=0.09, note=f'{a:.2f} to {b:.2f} s')

    x = highpass(src['karate'], 100)
    t0 = onset(x, 0.0, 0.3)
    wr.add('special.wav', x[int((t0 - 0.003) * RATE):int((t0 + 0.8) * RATE)], 'karate', 'the held voice-like tone, first 0.8 s', -17, fade_in=0.003, fade_out=0.3, note=f'from {t0:.2f} s')

    # --- The metallic burst of the "swords" file (its first burst: a roll of eight clicks).
    x = highpass(src['swords'], 300)
    found = regions(x, 30, 0.12)
    a = found[0][0]
    wr.add('rattle.wav', x[int((a - 0.01) * RATE):int((a + 0.55) * RATE)], 'swords', 'the first burst of metallic clicks, 0.55 s', -22, fade_in=0.004, fade_out=0.2, note=f'from {a:.2f} s')

    # --- Stings, set by their loudest 400 ms.
    x = highpass(src['intro'], 35)
    t0 = onset(x, 2.55, 2.85, win=0.005)
    wr.add('sting-start.wav', x[int((t0 - 0.005) * RATE):int((t0 + 3.4) * RATE)], 'intro', 'the hit, the swell and the boom (3.4 s from 2.7 s)', -19, window=0.4, fade_in=0.003, fade_out=1.0, note=f'from {t0:.2f} s')

    x = highpass(src['ident3'], 35)
    e, w = env_db(x, 0.01)
    t0 = int(np.argmax(e > -45)) * w
    wr.add('sting-win.wav', x[int((t0 - 0.005) * RATE):int((t0 + 4.2) * RATE)], 'ident3', 'a boom, then climbing plucks (4.2 s)', -19, window=0.4, fade_in=0.003, fade_out=1.5, note=f'from {t0:.2f} s')
    p0 = onset(x, 2.7, 3.0, win=0.005)
    wr.add('sting-style.wav', x[int((p0 - 0.005) * RATE):int((p0 + 0.95) * RATE)], 'ident3', 'two plucks of the climb (0.95 s)', -17, window=0.1, fade_in=0.003, fade_out=0.3, note=f'from {p0:.2f} s')

    x = highpass(src['ident1'], 35)
    wr.add('sting-end.wav', x[:int(3.2 * RATE)], 'ident1', 'a boom, plucks and a second boom (the first 3.2 s)', -20, window=0.4, fade_in=0.003, fade_out=1.2, note='from 0.00 s')

    # --- The ambience loop: the end crossfaded into the start, so the knocks stay whole.
    x = compress(highpass(src['bamboo'], 40), -42, 2.5)
    n, xf = len(x), int(0.3 * RATE)
    body = x[xf:].copy()
    ramp = np.linspace(0, np.pi / 2, xf)
    body[len(body) - xf:] = x[n - xf:] * np.cos(ramp) + x[:xf] * np.sin(ramp)
    wr.add('ambience-dojo.wav', body, 'bamboo', 'the bamboo bed as a seamless 4.7 s loop (knocks eased 2.5:1 above -42 dB)', -34, window=None, fade_in=0.0, fade_out=0.0,
           note=f'seam: {body[-1]:+.4f} -> {body[0]:+.4f}, step {abs(body[0] - body[-1]):.4f}, typical step {np.abs(np.diff(body)).mean():.4f}')
    return wr


def check_ffmpeg(rows, out):
    """A second opinion: ffmpeg's ebur128 integrated loudness and true peak of each file (padded with silence to 1.2 s, so a
    short clip has a whole gating block; the gate then keeps only the clip's own blocks)."""
    for r in rows:
        p = subprocess.run(['ffmpeg', '-nostats', '-hide_banner', '-i', os.path.join(out, r['name']), '-af', 'apad=whole_dur=1.2,ebur128=peak=true', '-f', 'null', '-'], capture_output=True, text=True).stderr
        i = re.findall(r'I:\s+(-?[\d.]+) LUFS', p)
        tp = re.findall(r'Peak:\s+(-?[\d.]+) dBFS', p)
        r['ff_i'] = float(i[-1]) if i else float('nan')
        r['ff_peak'] = float(tp[-1]) if tp else float('nan')


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--check', action='store_true', help="also ask ffmpeg's ebur128 for each output's loudness")
    ap.add_argument('--out', default=OUT)
    args = ap.parse_args()
    for k, v in FILES.items():
        if not os.path.exists(os.path.join(SRC, v)):
            sys.exit(f'missing source {v}')
    for k in ('intro', 'ident1', 'ident3'):
        c = stereo_check(os.path.join(SRC, FILES[k]))
        print(f'{FILES[k]}: stereo channel correlation {c:.2f}' + ('  (low: the mono fold-down may thin it)' if c is not None and c < 0.3 else ''))
    wr = build(args.out)
    if args.check:
        check_ffmpeg(wr.rows, args.out)
    total = sum(r['size'] for r in wr.rows)
    print(f'\n{"file":22} {"secs":>5} {"peak":>5} {"LK100":>6} {"LK400":>6} {"LKall":>6}' + ('  ff-I  ff-pk' if args.check else '') + '  note')
    for r in wr.rows:
        line = f'{r["name"]:22} {r["secs"]:5.2f} {r["peak"]:5.2f} {r["l100"]:6.1f} {r["l400"]:6.1f} {r["lall"]:6.1f}'
        if args.check:
            line += f' {r["ff_i"]:6.1f} {r["ff_peak"]:6.1f}'
        print(line + f'  {r["note"]}{r["limited"]}')
    print(f'{len(wr.rows)} files, {total / 1024:.0f} KiB')


if __name__ == '__main__':
    main()
