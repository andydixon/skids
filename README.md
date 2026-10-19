# Skids: a Super Skidmarks revival

A cross-platform (Windows, macOS, Linux) engine written in Rust that plays
*Super Skidmarks* (Acid Software, 1995) on the original Amiga data.

At build time it reads the game's ADF disk images from this repository:
it unpacks the LHA archives on them and builds the game files into the
executable. At run time it decrunches the CrunchMania-packed files and
decodes the planar track graphics and AGA sprite banks. The finished
program is a single self-contained file, with no disks needed to play.

Because the binary carries the original game data, anyone you give a build
to also gets the game data.

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

`--data DIR` adds to or overrides the built-in data with more ADF images,
LHA archives, or extracted `track/`, `vehicles/` and `palettes/` folders.
Without `--data`, a folder like that in the current directory or next to
the executable is picked up automatically. That means dropping disk 1 or
real palette files next to the game just works.

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
skids extract --data DIR --out pngs/      # dump every track (with gates) and car sheet
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

The build embeds the data from the `*.adf` files next to `Cargo.toml`. To
use images from somewhere else, set `SKIDS_EMBED_DIR=/path/to/adfs`. To
build without embedding, so the game needs `--data` at run time, use
`--no-default-features --features audio`.

GitHub Actions builds all three platforms on every push
(`.github/workflows/build.yml`).

## Layout

```
build.rs       unpacks the ADFs and embeds the game files at build time
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
