# Game assets

Everything the game uses is stored here as editable files. The build embeds
this folder into the executable. To pick up changes:
- **Rebuild:** `cargo run --release`.
- **Or skip the rebuild:** run with `--data assets`. Files given with
  `--data` override the built-in copy.

These files were converted from the original Amiga disks with
`skids extract --data <folder with the ADFs>`. That conversion is lossless,
and running it again overwrites your edits.

## Tracks: `tracks/<name>/`

### `track.png`

The track picture: 1024 × 512, a 16-colour indexed (palette) image.

- **Palette:** the PNG's palette is the track's palette. Change a palette
  entry to recolour every pixel that uses it.
- **Road vs scenery:** colours 8–15 are road; colours 0–7 are scenery,
  where cars throw up dust. The picture doesn't affect handling. The
  walls come from the gates (below).
- **Painting a new route:** paint it with the upper eight colours and
  update the gates to match.
- **Saving:** keep the image indexed if your editor allows it. If it saves
  as full-colour RGB instead, each pixel is matched to the nearest default
  colour, which loses your palette.

### `track.txt`

Plain text; lines starting with `#` are comments.

- **`gate x1 y1 x2 y2`:** lap gates in race order.
  - Gate 0 is the start/finish line, and the grid forms up behind it.
  - Cars must cross every gate, in order, to complete a lap.
  - **Walls:** the gates' ends also shape the road. One edge is a smooth
    curve through every gate's first end (`x1 y1`), the other through
    every second end (`x2 y2`). Those curves are the walls, and computer
    drivers follow the road between them. The curves pass halfway
    between neighbouring gates' ends and round off the corners, as in
    the original.
  - Coordinates are on the isometric ground plane. To find where a point
    on the ground sits in the picture, with `z` its height in pixels
    (from `height`):
    `picture x = wx - wy + 512`, `picture y = (wx + wy) / 2 − 156 − z`.
  - `skids trace --data assets --track N --out t.png` draws the walls
    (grey), the gates (gate 0 in yellow) and the computer cars' paths over
    the picture.
- **`height`:** 64 rows of 64 numbers giving ground height in pixels × 64.
  - **Layout:** the rows run along world y and the numbers within a row
    along world x. Each covers 16 × 16 world units.
  - **Effect:** climbing slows cars and descending speeds them up. Cars
    leave the ground over crests that drop away faster than gravity, and
    the car sprites tilt with the slope.
- **`set`:** the name of the set of tracks this one belongs to, shown under
  the menu thumbnail.
- **`start`, `flags`:** values from the original data that the game
  doesn't use.

### `thumb.png` (optional)

The track's 123 × 32 thumbnail from the original track-select screen, in
the track's palette. It's drawn at double height in the menu, because the
original's pixels are half-width. Without one, the menu shows a shrunken
`track.png`.

To add a track, make a new folder with `track.png` and `track.txt`. Tracks are listed in
name order.

## Vehicles: `vehicles/<id>.png` and `<id>.txt`

### `<id>.png`

A 2048 × 800 sprite sheet: 32 headings across by 25 tilts down, each cell
64 × 32.

- **Colours:** use only palette indices 0–3:
  - 0 transparent
  - 1 tyres / shadow
  - 2 trim
  - 3 bodywork
  
  The colours in the file are only a preview; each player's car is
  recoloured at run time.
- **Headings:** column 12 faces right, and columns advance clockwise in
  11.25° steps.
- **Tilts:** row = roll × 5 + pitch (each 0–4), so row 12 is the car on
  flat ground.

### `<id>-menu.png` (optional)

The car's 64 × 32 picture from the original select screen, in the same
four colours as the sprite sheet.

### `<id>.txt`

The display name, driver and handling:

| Setting   | Meaning |
|-----------|---------|
| `name`    | name shown in the menu |
| `driver`  | driver's name, shown next to it |
| `trailer` | `yes` for something towed behind cars (the caravan) rather than driven |
| `handling`| the 16 values from the original `.set` file, used as the original game uses them (below) |
| `mass`    | weight when cars collide; not from the original data |

The `handling` values, counting from 1:

| Value | Meaning |
|-------|---------|
| 1, 2, 6 | computer drivers: drag, grip, power |
| 9, 10, 11 | human drivers: drag, grip, power |
| 3 | steering: 16 = 1/64 of a turn per frame at full lock |
| 5 | gravity: 8 = 1 pixel per frame² |
| 7 | tow hitch distance behind the car: 2 = 1 world unit |
| 4, 8, 12–16 | unused |

What the driving values do:
- **Drag** (below 64): how much of the engine's speed survives each frame.
  Low drag gives fast pick-up and fast slowing.
- **Power:** what the throttle adds.
- **Top speed:** `power × drag / (64 − drag) / 32` world units per frame,
  at 50 frames a second.
- **Grip:** how quickly the car's movement turns to follow where it's
  pointing. Low grip means long slides.

The menu's **Handling** option can replace every car's values with one of
the original's presets.

Older files with `original`, `top`, `accel`, `brake`, `grip`, `turn` and
`offroad` lines still load: `original` is read as `handling`, and the
others are ignored. To add a car, add a new `<id>.png` / `<id>.txt` pair.

## Sounds: `sounds/*.wav`

8-bit mono WAV files at the original Amiga sample rates. A loop point,
stored in the WAV's `smpl` chunk, makes a sound repeat from there; most
sample editors keep it. The game uses these names:

| Sound | Use |
|-------|-----|
| `motor3` | engine, pitched up and down with revs (falls back to `motor`, then `engine`) |
| `screech2` | tyres sliding (falls back to `screech3`, then `screech`) |
| `crash1`, `crash2`, `crash3` | collisions, picked at random |
| `backfire` | lifting off at high revs |
| `horn` | Right Shift (player 1) or Left Shift (player 2) |
| `beep`, `beep2` | countdown, then lights out |

Missing sounds fall back to synthesised ones. Any other WAV files are kept
but not used.

## Music: `music/*.med`

The tune, an OctaMED module. Edit it in a tracker that reads MED files,
such as OpenMPT. The game's player supports plain sample instruments (no
synth sounds) and the commands listed in `docs/FORMATS.md`. Preview it
with `skids music --data assets --out tune.wav`. If there are several
files, the first in name order plays.

## Pictures: `gfx/*.png`

Indexed PNGs. `skidtoon.png` is the title picture at the top of the menu,
shown at half size. The others aren't used:

- `ball.png`: 64 frames of a spinning ball with the Acid logo, 16 frames
  across, each 32 × 33. Its real colours aren't known, so it has a grey
  palette.
- `chat2.png`, `mouse.png`, `sign.png`: small odds and ends.

## Palettes: `palettes/*.pal` (optional)

16 lines of `RRGGBB`. Each file overrides the PNG palettes:

- `track1-3.pal`: one track
- `track1.pal`: every TRACK1 track
- `default.pal`: all tracks
