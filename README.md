# Skids: a Super Skidmarks revival

A cross-platform (Windows, macOS, Linux) engine written in Rust that plays
*Super Skidmarks* (Acid Software, 1995) on the original Amiga data.

The game's data was converted from the original ADF disk images into
editable files in [`assets/`](assets/README.md):

- **Each track:** a 16-colour PNG (the PNG's palette is the track's
  palette), plus a text file with the lap gates and height map.
- **Each car:** a PNG sprite sheet, plus a text file with its name and
  handling.

Edit them with any image editor or text editor. The build embeds the
`assets/` folder, so the finished program is a single self-contained file.

The engine can still read the original disks directly: ADF images, the LHA
archives on them, and their CrunchMania-packed files.
`skids extract --data <adfs>` regenerates `assets/` from them.

Because the binary and `assets/` carry the original game data, anyone you
give either to also gets the game data.

## What works

- All 24 tracks across the four track files. Each has its isometric height
  map and lap gates.
- All 8 vehicles (Mini, Beetle, Porsche, Formula 1, Midget, Truck, Police,
  Cow), with 32 headings and 25 body-tilt frames for hills.
- 1 or 2 players (split screen) plus up to 7 computer drivers.
- Power-slide handling, slopes, car-to-car bumping, and skid marks burnt
  into the track.
- Lap and position tracking, lap times, and a results table.
- Procedural engine, tyre and crash sound.

## What's reconstructed

Disk 1 of the six-disk set is missing from this archive. It held the
executable, palettes, sound and music. As a result:

- **Handling, AI and race rules** are new code written to feel like the
  original, not a translation of the original 68000 code.
- **Colours** come from a reconstructed palette, built around how the
  tracks use their 16 colour indices. If you have the real palettes, put
  them in a `palettes/` folder next to the disks as `.pal` files (16 lines
  of `RRGGBB`):
  - `track1-3.pal` for one track
  - `track1.pal` for all six tracks in a file
  - `default.pal` for everything
- **Sound** is synthesised.

If you find disk 1, drop it in with the others. Anything it contains under
`track/` or `vehicles/` is picked up automatically. The format notes in
[docs/FORMATS.md](docs/FORMATS.md) cover everything reverse engineered so
far.

## Running

```sh
cargo run --release
```

The built executable (`target/release/skids`, or `skids.exe` on Windows)
runs on its own from anywhere.

`--data DIR` adds to or overrides the built-in data. `DIR` can be an
assets folder, which lets you try edits without rebuilding:
`skids --data assets`. It can also be the original ADF images or LHA
archives. Without `--data`, a folder like that in the current directory or
next to the executable is picked up automatically. When a track or car
exists both as an asset and on disk, the asset wins.

| Action      | Player 1                              | Player 2 |
|-------------|---------------------------------------|----------|
| Accelerate  | ↑ (or W in one-player games)          | W        |
| Brake / reverse | ↓ (or S)                          | S        |
| Steer       | ← → (or A D)                          | A D      |
| Pause       | P                                     |          |
| Leave race  | Esc                                   |          |

Options: `--scale 1|2|4` (window size), `--mute`.

### Tools

```sh
skids info    --data DIR                  # list tracks and vehicles found
skids extract --data ADFS [--out assets]  # convert original disks into editable assets
skids bench   --data DIR --seconds 120    # race CPU cars on every track: laps and time on road
skids trace   --data DIR --track 11 --out t.png   # plot CPU racing lines
skids render  --data DIR --track 3 --out shot.png # headless screenshot
```

## Building

You need a stable Rust toolchain.

- **Linux:** install the ALSA and X11 development packages, e.g.
  `sudo apt install libasound2-dev libx11-dev libxkbcommon-dev`. To build
  without sound (no ALSA needed), use
  `cargo build --release --no-default-features --features embed`.
- **macOS / Windows:** `cargo build --release` works as is.

The build embeds `assets/`. To use a different folder, set
`SKIDS_ASSETS_DIR=/path/to/assets`. To build without embedding, so the game
needs `--data` at run time, use `--no-default-features --features audio`.
The ADF images aren't needed to build.

GitHub Actions builds all three platforms on every push
(`.github/workflows/build.yml`).

## Layout

```
assets/        the game data as editable PNG and text files
build.rs       embeds assets/ into the executable
src/formats/   ADF (OFS/FFS), LHA and CrunchMania readers
src/data/      track, vehicle and palette decoding; locating the game files
src/game/      physics, AI and race rules
src/render/    software renderer at 320×256 (PAL low-res) and a bitmap font
src/audio.rs   synthesised sound (cpal)
src/app.rs     menus, race screens and results
```

## Credits

*Super Skidmarks* is by Acid Software. This project is an unofficial
revival for preservation. The CrunchMania bitstream layout follows Teemu
Suutari's [ancient](https://github.com/temisu/ancient) (BSD-2-Clause).
