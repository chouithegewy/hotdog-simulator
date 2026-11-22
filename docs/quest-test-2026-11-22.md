# Quest 2 and multiplayer test, 22 November 2026

What was run and what it showed. Device: Meta Quest 2 (Adreno 650), Oculus OpenXR runtime 210.137.0; desktop: Linux,
Intel Arc B580. Frame rates are the headset's own (`VrApi` in `adb logcat`); the headset wants 72 a second.

## The app on the headset

| Check | Result |
| --- | --- |
| `tools/build-quest.sh install` | Builds, installs and starts. Eyes 1440 by 1584 as the headset asks. |
| First build, the intro | 4 frames a second. |
| First build, in a round (Lowest level) | 10 frames a second (80 to 100 ms a frame, all of it the GPU). |
| The same without the screen in the air | 60 ms. |
| The same without the sun's and the lamps' light | 30 ms. |
| Unlit materials | 22 ms. |
| Unlit, nothing done to the finished picture | 15 ms. |
| An empty world, no screen in the air | 5 ms (72 frames a second). |
| The headset's graphics as committed (unlit, eyes 1080 by 1188, screen 720 tall) | 72 frames a second in a round (8 to 10 ms); about 46 in the round's first seconds and in the intro. |

Shadows, the number of lamps, the game's own two cameras and the materials' extras (clear coat, normal maps) each
made no difference worth having: it is the lit path itself that a Quest 2 cannot afford at this size.

Reported from inside the headset on the first build: the view faced the back of the fridge (the room was turned by
where the head happened to face: fixed), and head movement felt inverted and laggy (at 4 frames a second; not yet
looked at again at 72).

Not yet checked by anyone wearing it: the controls, the gun in the hand, the screen in the air and its pointer.

## Multiplayer against hotdog-simulator.chilos.dev

The public server takes only TLS (`http://` answers 301), and the native client had none: no native client could
join it. With TLS added (rustls):

| Check | Result |
| --- | --- |
| Desktop client, `--connect wss://hotdog-simulator.chilos.dev/ws` | "Multiplayer connected as hotdog 1 (60 steps and 30 frames a second)", map Titan's Kitchen; connected for 45 s with no error. |
| Quest app, the same server | Connected as hotdog 1; 52 to 58 frames a second in the fridge. |
| Desktop and Quest together | Desktop hotdog 1, Quest hotdog 2, both connected for 55 s with no error. |

Not checked: a round played between them (nobody was at either), and what each sees of the other.

Found on the way: the native client online at the top graphics level stops with a validation error (the ambient
occlusion's depth texture for a view one pixel across). It does so before today's changes too; `--quality medium`
avoids it.
