# Original HD hotdog player

`hotdog-player.glb` is an original glTF 2.0 model with approximately 45,000
triangles, PBR sausage and bun materials, raised mustard, face details,
rounded limbs, a player identity badge, and a looping walk animation.
Texture files are referenced relatively from `../materials/food/`.

Copyright (C) 2026 David Nunes-Childs. GPL-3.0-only; see the root LICENSE.

Rebuild with `python3 tools/build_hotdog_player.py`. The live game generates
shared geometry with five identity colors in `src/player_model.rs`.
Rebuild the original 2K food maps with
`/usr/bin/python3 tools/build_food_materials.py --size 2048` (Pillow required).

It contains no third-party geometry, scans or photographs.
