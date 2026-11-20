# The breakfast intro's people

Maya, the breakfast chef, and her husband are original characters of this project, made entirely in code when the game
starts. Copyright (C) 2026 David Nunes-Childs; GPL-3.0-only, like the rest of the project's own work. No scan, photograph,
third-party mesh, rig, morph target or texture is used for them, and no file is loaded for them.

- **Shapes** (`src/human/shapes.rs`): ellipsoids, bodies lofted through stacked ellipses, tubes swept along smooth paths
  with round ends, surfaces of revolution (the pleated toque, his fringe), cartoon hands with four fingers and a thumb
  (open, half closed for a handshake, closed round a handle), and six mouth shapes (rest, Ah, Ee, Oh, Mm and a laugh)
  drawn as lips, mouth, teeth and tongue on the face. Colours are in the vertices.
- **Who they are** (`src/human/people.rs`): their proportions and every piece. Maya wears a tall pleated toque, a white
  double-breasted jacket with its sleeves rolled up, a red neckerchief, a mustard bistro apron with a pocket and a bow,
  black-and-white check trousers and black clogs; her husband has round glasses, a ginger moustache and fringe round a bald
  crown, a teal cable-knit jumper over a round belly, corduroy trousers and slippers. The check, the knit and the corduroy
  are 32-pixel tiles painted in code.
- **Rig** (`src/human/mod.rs`): a root, a pelvis, a chest that bends at the waist and a head that nods and turns; arms
  and legs placed every frame between joints found by two-bone reaching to where the hands and feet are asked to be; eyes
  that look and blink; the hand's grip and the mouth's shape swapped per frame. The meshes are remade with about a third
  of the triangles at the Low and Lowest graphics levels (`quality::Detail::lean`).
- **Acting** (`src/maya/choreography.rs`, `src/maya/act.rs`): the scene's clock, paths, camera shots and planted steps
  (written for this project in Python in 2026, ported here unchanged), and the poses made from them: the handshake and the
  hug, the reach into the fridge with both hands on the tray's rims, the carry, the knife held by its handle and the
  package steadied by its rim, glances, blinks, and the mouth shapes of their lines, taken from the lines' letters.

The hotdogs' faces in the fridge (the sleepers' closed eyes and smiles, the self-aware hotdog's wide eyes and open mouth)
are also original mesh details, drawn in code in `src/hotdog.rs`.

Earlier versions of the project used other characters for this scene: a model adapted from a Renderpeople scan and a
portrait made from a photograph. They, the scripts that made them and everything derived from them were removed from the
project's files; nothing of them is in the current source or assets. They remain in the repository's earlier history
(see THIRD_PARTY_NOTICES.md).
