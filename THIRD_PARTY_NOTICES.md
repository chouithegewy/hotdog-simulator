# Asset and copyright notes

The GPL-3.0-only project license covers the project's original work. It does not grant rights to the third-party
material listed below, which keeps its own terms.

## Original work

These are the project's own, copyright 2026 David Nunes-Childs, GPL-3.0-only. None uses a scan, a photograph or a
third-party model, rig, texture or recording.

- **The breakfast intro's people**, Maya and her husband: made in code at load (`src/human/`, `src/maya/`). See
  `assets/CHARACTER_CREDITS.md`.
- **The hotdogs**: `src/player_model.rs`, `src/hotdog.rs`, and the glTF model `assets/models/hotdog-player.glb` made by
  `tools/build_hotdog_player.py` (not loaded by the game).
- **The food textures** in `assets/materials/food` (PNG, and WebP copies for the browser): generated from seeded noise by
  `tools/build_visual_assets.py` (`tools/build_food_materials.py` runs it); the script reproduces the committed PNGs
  exactly.
- **The package label**, `assets/materials/package_label.png`: drawn by `tools/build_package_label.py`. Its brand,
  "Snappington's", is made up for the game. Its lettering is rendered with the DejaVu fonts (a free font licence; the
  picture of the letters is not a copy of the fonts).
- **Synthesised sounds**: `sizzle.wav` and `click.wav` (`tools/build_griddle_sounds.py`), the `fu-*.wav` sounds
  (`tools/build_kungfu_sounds.py`), `whoosh.wav` and `thwack.wav` (`tools/build_melee_sounds.py`), `relish-splat.wav`
  and `relish-thump.wav` (`tools/build_relish_sounds.py`), `reload-*.wav` (`tools/build_reload_sounds.py`),
  `squelch.wav` and `squirt.wav` (`tools/build_squirt_sounds.py`), the `titan-*.wav` sounds
  (`tools/build_titan_sounds.py`): noise, sines and envelopes, nothing sampled.
- `pew.wav`, `bite.wav`, `boom.wav` and `ding.wav` came with the project's first commit: short mono 22050 Hz synthetic
  effects. No script that made them is in the repository; the project owner states that they, like every other `.wav`
  file here, are freely distributable.
- **Shaders** in `assets/shaders` and `src/`: the project's own. `hotdog_bend.wgsl` and `hotdog_bend_prepass.wgsl` are
  Bevy's mesh and prepass vertex shaders with the project's bend added; Bevy's code is MIT OR Apache-2.0 (notice below).
- **Maps** in `assets/maps`, the page, the map editor and the tools.

## Kung Fu recordings (Pixabay)

Twelve sound recordings, supplied by the project owner, come from Pixabay's sound-effects search for "kung fu"
(pixabay.com/sound-effects). The project owner supplied them as royalty-free, to be used under the Pixabay Content
License; the licence's own terms are Pixabay's and are not reproduced or checked here, and they are not covered by the
project's GPL-3.0-only license. The uploader is the first part of each file name and the number at its end is the
Pixabay id, as supplied:

| File (in `assets/audio/`) | Uploader | Pixabay id |
| --- | --- | --- |
| `freesound_community-big-punch-short-with-male-moan-83735.mp3` | freesound_community | 83735 |
| `freesound_community-big-punch-with-whoosh-103638.mp3` | freesound_community | 103638 |
| `freesound_community-karate-100238.mp3` | freesound_community | 100238 |
| `freesound_community-kung-fu-clothes-hits-and-clothing-sounds-73455.mp3` | freesound_community | 73455 |
| `freesound_community-kung-fu-punch-4-105262.mp3` | freesound_community | 105262 |
| `freesound_community-kung-fu-scream-43668.mp3` | freesound_community | 43668 |
| `freesound_community-kung-fu-yell-6314.mp3` | freesound_community | 6314 |
| `freesound_community-wu-swords-hip-hopc-83035.mp3` | freesound_community | 83035 |
| `pixelpond-kung-fu-in-bamboo-woods-361785.mp3` | pixelpond | 361785 |
| `djartmusic-asiatic-intro_1-283723.mp3` | djartmusic | 283723 |
| `djartmusic-chinese-ident-transition-1-283708.mp3` | djartmusic | 283708 |
| `djartmusic-chinese-ident-transition-3-283709.mp3` | djartmusic | 283709 |

The original downloads are not kept in this repository (the owner has them; anyone else can fetch them from Pixabay by
the ids above and put them in `assets/audio/`, which `.gitignore` leaves untracked) and are never packaged: the game
plays derived files instead.
`tools/build_kungfu_user_sounds.py` cuts them into short one-shots (and, for the stings and the loop, longer pieces),
filters, fades, normalises and resamples them to mono 22050 Hz wave files in `assets/audio/kungfu/`, which is where
`src/fu.rs` reads them. Those files remain the uploaders' work under the same terms.

As the project understands the Pixabay Content License (check its current text before relying on this): it allows the
sounds to be used, changed and published as part of a larger work such as this game, commercially or not, without
attribution; it does not allow the content to be sold or redistributed on its own, as standalone files. The cut clips
inside the game are the use it allows. The original downloads, kept as files in a public repository, would be
closest to redistributing them on their own, and nothing in the game needs them: that is why they are left out (the
owner's own copy is enough to cut the clips again).

## Fonts

- The map editor's fonts, `editor/fonts/` (Barlow Semi Condensed by The Barlow Project Authors, Fredoka by The Fredoka
  Project Authors): SIL Open Font License 1.1, whose texts are beside them (`OFL-Barlow.txt`, `OFL-Fredoka.txt`).
- The game's own text is drawn with Bevy's built-in font (Fira Mono, by Mozilla and Carrois Apostrophe), compiled into the
  game by Bevy's `default_font` feature: SIL Open Font License 1.1. (Bevy's windowing library embeds Cantarell for Wayland
  title bars: the same licence.) The texts, with the copyright lines, are in `THIRD_PARTY_LICENSES.html`.

## Code

- `vendor/wgpu-hal`: wgpu-hal 29.0.4 from crates.io with one change (`vendor/wgpu-hal/Cargo.toml` says which), MIT OR
  Apache-2.0, its licence texts in `vendor/wgpu-hal/LICENSE.MIT` and `LICENSE.APACHE`.
- Rust and JavaScript dependencies keep their own licences (Bevy and most crates: MIT OR Apache-2.0; others MIT,
  Apache-2.0, BSD-2/3-Clause, ISC, Zlib, BSL-1.0, Unicode-3.0, Unlicense, CC0-1.0, MIT-0 or 0BSD; all compatible with
  GPL-3.0-only). GPL-3.0-only applies to this project's original work, not to the dependencies, whose notices go with
  any build that contains them: **`THIRD_PARTY_LICENSES.html`** (made by `tools/build-licenses.sh` from the `Cargo.lock`
  files with cargo-about, and checked current by CI) lists every crate the programs and the browser builds are made of
  with its licence's full text, and the notices for the fonts, the adapted Bevy shader code, the lookup tables Bevy's
  crates embed, the vendored wgpu-hal and the Pixabay clips as well. Every package and the browser game carry it, and
  the menu and the page link to it. The project's own licence and where its source code is published are stated there,
  in `HOW-TO-RUN.txt` and in the menu.

## Removed material

Earlier commits of this repository contain material that is not the project's and whose terms restrict public
redistribution: models adapted from Renderpeople scans (`assets/maya.glb`, `assets/giant.glb`,
`assets/giant_expressive.glb`), a texture made from a person's photograph (`assets/reconstructed/face.png`), the
reference data and scripts used with them, and MediaPipe's canonical face model (Apache-2.0). They were removed from the
project's files, and nothing in the current source or assets is derived from them; they remain in the earlier history.
