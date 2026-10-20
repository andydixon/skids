# Super Skidmarks data formats

Notes from reverse engineering the AGA/CD32 release of *Super Skidmarks*
(Acid Software, 1995), worked out from all six disks. The one thing not
yet decoded is the executable's game logic. See [Gaps](#gaps).

All multi-byte values are big-endian unless noted.

## Disks

Each disk is a standard 880 KB double-density ADF (1760 blocks of 512
bytes). Disk 1 is FFS (`DOS\1`); disks 2–6 are OFS (`DOS\0`). The `[HD]` in
the file names refers to the hard-drive-installable release.

| Disk | Archive       | Game files |
|------|---------------|------------|
| 1    | `smcd32a.lha` | Executables, `sfx/`, `gfx/`, the tune, `vehicles/*.set`, `Camaro.aga`, `caravan.aga` |
| 2    | `smcd32b.lha` | `vehicles/cow.aga`, `track/TRACK1` |
| 3    | `smcd32c.lha` | `track/TRACK2`, `vehicles/TRUCK.aga` |
| 4    | `smcd32d.lha` | `track/TRACK3`, `vehicles/PORSCHE.aga` |
| 5    | `smcd32e.lha` | `track/track4`, `vehicles/vw.aga` |
| 6    | `smcd32f.lha` | `vehicles/F1.aga`, `Midget.aga`, `mini.aga`, `Police.aga` |

Disk 1 also carries an Installer script that installs from all six disks,
and a few AmigaDOS commands.

Each archive stands on its own; the set isn't one archive split across
disks. They use level-0 headers under a top-level `super_skidmarks_cd³²\`
directory.

- **Compression:** disks 2–6 store their members (`-lh0-`), which are
  themselves CrunchMania-packed. Disk 1 mostly uses `-lh5-` (LZSS with
  static Huffman blocks, decoded by `src/formats/lzh.rs`).
- **Integrity:** every member is checked against its header CRC-16. One
  file, `c/iconx` (an AmigaDOS utility the game doesn't use), fails its CRC
  in this dump. The reference `lha` tool reports the same failure.

## CrunchMania

```
0   4   id: "CrM!" (LZ + fixed codes) or "CrM2" (LZ + Huffman)
        lower-case "Crm" = delta-encoded samples (running byte sum)
4   2   unused
6   4   unpacked size
10  4   packed size (excludes this 14-byte header)
14  …   packed stream
```

The stream is read **backwards** from `14 + packed - 6`, a byte at a time,
LSB first. The last six bytes seed the bit buffer: a 32-bit word shifted
right by `16 - shift` with `16 + shift` valid bits, where `shift` is the
final 16-bit word. Output is also written from the end towards the start.
`src/formats/crm.rs` implements both variants. Every packed file on the
disks decodes exactly: the output buffer fills completely and the input is
consumed to its first byte.

## Tracks (`track/TRACKn`)

```
0x00  7 × u32   end offset of sections 0..6
0x1C  …         section 0: thumbnails and palette for the whole file (20622 bytes, raw)
…               sections 1..6: one track each, CrunchMania-packed
```

So the four files hold 24 tracks.

The last 142 bytes of section 0 hold:

| Offset | Size | Contents |
|--------|------|----------|
| 0      | 2    | File number (1–4) |
| 2      | 96   | The palette shared by the file's six tracks: 16 colours × R, G, B, each a `u16` from 0 to 15 (Amiga 12-bit colour) |
| 98     | 12   | Unknown |
| 110    | …    | The name of the set of tracks, NUL-terminated |

The first 20480 bytes of section 0 are the track-selector thumbnails:
- **Picture:** one 640 × 64, 16-colour picture in the file's palette,
  stored as four bitplanes one after another (80 bytes per row, 5120 bytes
  per plane).
- **Layout:** the six tracks' thumbnails, 123 × 32 each, sit in a 3 × 2
  grid at its top left, in track order: top row tracks 1–3, bottom row
  4–6.
- **Display:** the picture is Amiga hi-res, whose pixels are half as wide
  as low-res ones. So a thumbnail shows at 123 × 64 next to the low-res
  track pictures, the same shape as the track.

Each track section unpacks to 281240 (`0x44A98`) bytes:

| Offset   | Size   | Contents |
|----------|--------|----------|
| `0x0000` | 8192   | Height map: 64 × 64 `u16`, 8.8 fixed point, row-major. Each cell covers 16 × 8 screen pixels. |
| `0x2000` | 8192   | Unused, zero on every track. |
| `0x4000` | 8      | `u16` gate count, `u16` start x, `u16` start y, `u16` flags. |
| `0x4008` | 8 × n  | Gates: `x1 y1 x2 y2`, world coordinates × 32. The list repeats after the last gate. |
| …        |        | Unknown data, mostly a bit pattern, up to `0x4A98`. |
| `0x4A98` | 4 × 65536 | Four non-interleaved 1024 × 512 bitplanes, giving 16 colours. |

### Track geometry

Gates are line segments across the road and run in race order; gate 0 is
the start/finish line. They're defined on an isometric ground plane rotated
45° to the picture. The projection that best fits the data is:

```
screen x = wx - wy + 512
screen y = (wx + wy) × 31/64 - 252
```

With these constants, 96% of points sampled along the gates land on road
pixels across all 24 tracks (92% minimum). A plain screen-aligned 2:1 squash
gives 40–70%. Adding a height term doesn't improve the fit, so the height
map is used only for slope physics and body tilt.

The stored start point is on the road for 14 of the 24 tracks read as screen
coordinates, and 10 read as world coordinates. It isn't used; the grid forms
up behind gate 0.

### Road and scenery

Colours 8–15 are the driving surface on every track, so the fourth
bitplane is effectively a road mask. Off the road means less grip and
lower speed.

## Vehicles

### Sprite banks (`vehicles/*.aga`)

CrunchMania-packed, 409600 bytes unpacked:
25 tilt blocks × 32 headings × 512-byte frames.

A frame is a 64 × 32 AGA hardware sprite. Each line holds 8 bytes of plane
0, then 8 bytes of plane 1, giving 3 colours plus transparent:

| Index | Use                 |
|-------|---------------------|
| 0     | transparent         |
| 1     | tyres, shadow, dark |
| 2     | trim / accent       |
| 3     | bodywork            |

- **Heading:** frame 12 faces screen-right, and frames advance clockwise in
  11.25° steps. In world terms, frame = 12 + (angle + 45°) / 11.25°.
- **Tilt:** block = roll × 5 + pitch, each in 0..4, with 2 = level. Block
  12 is the car on flat ground.

The caravan (`caravan.aga`) uses the same layout. It's a trailer with a tow
bar, not a car.

`Police.aga` reads as all zeros with one third-party ADF tool, but it's
intact when read directly from the disk image.

### Settings (`vehicles/*.set`)

- **Header:** the car's name and its driver's name, each followed by a
  newline.
- **Settings:** 16 `u16` values. Eleven are used by the cars; the rest are
  zero. The caravan's values differ in kind; two of them are −4.
- **Menu picture:** 512 bytes. A 64 × 32 picture in two bitplanes, plane 0
  (256 bytes) then plane 1.

The settings are clearly handling values. The first is smallest for the
Cow, the second largest for the F1, and the Police car and Porsche are
nearly identical. What each one means lives in the executable and isn't
decoded yet.

## Sounds (`sfx/*.iff`, `gfx/*.iff`)

Uncompressed IFF 8SVX, 8-bit signed, mostly at Amiga rates (8363 Hz and
others). Looping sounds use the repeat part of `VHDR`. `gfx/` holds a few
extra and alternative sounds (`crash1`, `crash2`, `engine`, `motor`,
`screech`); `gfx/crash4.iff` isn't IFF.

## Pictures (`gfx/*.iff`)

IFF ILBM with ByteRun1 compression. `skidtoon.iff` is the 608 × 180,
16-colour title picture; `chat2`, `mouse` and `sign` are small odds and
ends.

`ball.spr` is a bank of 64 saved hardware sprites: a spinning ball with
the Acid logo. Each 544-byte frame is:

- **Header** (16 bytes): `u32` saved memory pointer, `u16` lines (33),
  `u16` strips (4), `u16` flags (`0x8020`), `u16` bytes per strip (132),
  4 unused bytes.
- **Strips:** four 16-pixel sprites, each line two bitplane words.

Flag bit 15 marks attached sprites. The strips pair up into two 16-colour
columns, so a frame is 32 × 33 pixels. The bank carries no colours.

## Music (`sfx/tune.crn`)

A CrunchMania-packed OctaMED `MMD1` module: 4 tracks, 21 plain 8-bit
sample instruments, 70 blocks played in order. The main song is positions
0–65, ending with a jump back to the start. Positions 66–69 hold separate
short pieces that end with "stop".

The song uses these commands:
- `1`, `2` (slide up, slide down)
- `3` (portamento)
- `9` (speed)
- `B` (jump)
- `C` (volume, decimal)
- `D` (volume slide)
- `F00` (break), `F1` (play twice), `F2` (delay), `FD` (set pitch),
  `FE` (stop)

`src/music.rs` plays these. Its loudness tracks OpenMPT's rendering of the
module second by second through the whole song (correlation 0.996 over the
first minute), and it loops at the same point.

## Executables

- **`SKIDMARKS_CD³²`** (237 KB): a single-hunk compiled Blitz Basic 2
  program. It includes IFF ILBM/8SVX/ANIM loaders and a tracker replay
  routine with note-period tables.
- **`skidmarks2`** (90 KB): a three-hunk program whose code is packed.

## Gaps

- **Game logic.** The handling, AI and rules in the executable aren't
  decoded; this port's versions are reconstructions. Decoding the `.set`
  values would be the first step towards the original handling.
- **Unknown data:** the 12 bytes after each track file's palette, the bit
  pattern after the gates in each track section, and the colours of
  `ball.spr` (probably set by the executable).

The engine's own editable formats, which `skids extract` converts all of
the above into, are described in [assets/README.md](../assets/README.md).
