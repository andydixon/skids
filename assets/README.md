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
- **Road vs scenery:** colours 8–15 are road (full grip, full speed).
  Colours 0–7 are scenery (less grip, slower).
- **Painting a new route:** paint it with the upper eight colours and
  update the gates to match.
- **Saving:** keep the image indexed if your editor allows it. If it saves
  as full-colour RGB instead, each pixel is matched to the nearest default
  colour, which loses your palette.

### `track.txt`

Plain text; lines starting with `#` are comments.

- **`gate x1 y1 x2 y2`:** lap gates in race order.
  - Gate 0 is the start/finish line, and the grid forms up behind it.
  - Cars must cross every gate, in order, to complete a lap. Computer
    drivers steer from one gate to the next.
  - Coordinates are on the isometric ground plane. To find where a point
    sits in the picture:
    `picture x = wx - wy + 512`, `picture y = (wx + wy) × 31/64 − 252`.
  - `skids trace --data assets --track N --out t.png` draws the gates
    (gate 0 in yellow) and the computer cars' paths over the picture.
- **`height`:** 64 rows of 64 numbers giving ground height × 256. Each
  number covers a 16 × 8 pixel cell of the picture. Slopes push cars
  downhill and tilt the car sprites.
- **`start`, `flags`:** values from the original data that the game
  doesn't use.

To add a track, make a new folder with both files. Tracks are listed in
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

### `<id>.txt`

The display name and handling:

| Setting   | Meaning |
|-----------|---------|
| `top`     | top speed on tarmac |
| `accel`   | acceleration |
| `brake`   | braking |
| `grip`    | sideways grip; higher means less sliding |
| `turn`    | steering rate |
| `mass`    | weight in collisions |
| `offroad` | speed and grip multiplier on scenery |

To add a car, add a new `<id>.png` / `<id>.txt` pair.

## Palettes: `palettes/*.pal` (optional)

16 lines of `RRGGBB`. Each file overrides the PNG palettes:

- `track1-3.pal`: one track
- `track1.pal`: every TRACK1 track
- `default.pal`: all tracks
