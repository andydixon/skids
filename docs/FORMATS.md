# Super Skidmarks data formats

Notes from reverse engineering the AGA/CD32 release of *Super Skidmarks*
(Acid Software, 1995). Everything here was worked out from the data on disks
2–6. Disk 1, which holds the executable, is missing from this archive, so
anything that lives only in the executable (palettes, sound, handling) is
unknown. See [Gaps](#gaps).

All multi-byte values are big-endian unless noted.

## Disks

Each disk is a standard 880 KB double-density ADF (1760 blocks of 512 bytes)
formatted as OFS (`DOS\0`). The `[HD]` in the file names refers to the
hard-drive-installable release, not high-density disks. Each disk holds a
single file:

| Disk | File          | Contents                           |
|------|---------------|------------------------------------|
| 1    | `smcd32a.lha` | *missing*                          |
| 2    | `smcd32b.lha` | `vehicles/cow.aga`, `track/TRACK1` |
| 3    | `smcd32c.lha` | `track/TRACK2`, `vehicles/TRUCK.aga` |
| 4    | `smcd32d.lha` | `track/TRACK3`, `vehicles/PORSCHE.aga` |
| 5    | `smcd32e.lha` | `track/track4`, `vehicles/vw.aga`  |
| 6    | `smcd32f.lha` | `vehicles/F1.aga`, `Midget.aga`, `mini.aga`, `Police.aga` |

Each disk's archive stands on its own (the set isn't one archive split
across disks). They use level-0 headers and the `-lh0-` method (stored),
under a top-level `super_skidmarks_cd³²\` directory. Most members are
individually packed with CrunchMania.

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
0x1C  …         section 0: track-selector thumbnails (20622 bytes, raw)
…               sections 1..6: one track each, CrunchMania-packed
```

So the four files hold 24 tracks. Each track section unpacks to 281240
(`0x44A98`) bytes:

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

### Palette use

The track pixel indices are ordered from dark to light. Indices 0–7 are
scenery and 8–15 the driving surface, so the fourth bitplane is effectively
a road mask. The game uses it as such here: off-road means less grip and
lower speed.

## Vehicles (`vehicles/*.aga`)

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
- **Tilt:** block = roll × 5 + pitch, each in 0..4, with 2 = level.
  Block 12 is the car on flat ground.

`Police.aga` reads as all zeros with one third-party ADF tool, but it's
intact when read directly from the disk image.

## Gaps

These almost certainly live on the missing disk 1:

- The main executable, and with it the real handling model, AI, rules,
  menus and fonts. This port's versions are reconstructions.
- Colour palettes. Neither the track nor the vehicle files carry any. The
  engine uses a reconstructed palette, and real ones can be dropped in as
  `.pal` files (see the README).
- Sound effects and music.
- Any tracks or vehicles beyond the 24 and 8 above.

The engine's own editable formats, which `skids extract` converts all of
the above into, are described in [assets/README.md](../assets/README.md).

If disk 1 turns up, put it next to the others and run `skids info`.
Anything in a `track/` or `vehicles/` directory inside its archive is
picked up automatically.
