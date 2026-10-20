# Super Skidmarks data formats

Notes from reverse engineering the AGA/CD32 release of *Super Skidmarks*
(Acid Software, 1995), worked out from all six disks and from the main
executable's machine code. See [Gaps](#gaps) for what's still unknown.

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
| 98     | 12   | Six `u16`, one per track, meaning unknown. The values are 0–5, some with bit 6 (`0x40`) set, plus one `0x3F`. |
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
| `0x0000` | 8192   | Height map: 64 rows (world y) × 64 `u16` (world x). Each cell is one unit of the game's world; the ground there is value / 64 pixels high. |
| `0x2000` | 8192   | Unused, zero on every track. |
| `0x4000` | 8      | `u16` index of the last gate (so the track has one more gate than this), `u16` start x, `u16` start y, `u16` flags. |
| `0x4008` | 70 × 8 | Gate slots: `x1 y1 x2 y2`, world coordinates × 32 (× 512 in the game's own units). Slots after the last gate repeat the list from the start. |
| `0x4238` | 2144   | Unknown. 88 bytes of mostly doubled bytes, then what looks like leftover picture data. |
| `0x4A98` | 4 × 65536 | Four non-interleaved 1024 × 512 bitplanes, giving 16 colours. |

The executable replaces the start point and flags of two tracks with
fixed values: (844, 50, flags 1) for file 1's track 2 and (187, 119, flags 0)
for file 2's track 4. It isn't clear whether those track numbers count from
0 or 1.

### Track geometry

Gates are line segments across the road and run in race order; gate 0 is
the start/finish line. They're defined on an isometric ground plane rotated
45° to the picture.

The rest of this section uses this port's world units, which are 1/16 of
the game's own unit (the 16.16 fixed-point values the executable works in).
So a height-map cell is 16 × 16 units.

The executable projects a point `z` pixels above the ground plane like
this:

```
screen x = wx - wy + 512
screen y = (wx + wy) / 2 - 156 - z
```

The car sprite position uses 182 in place of 156, putting the sprite's top
left 26 pixels higher. With `z` set to the ground height, 98% of points
sampled along the gates land on road pixels across all 24 tracks.

**Walls.** The gates are also the control points of the road edges:
- **The curves:** routine `$24670` evaluates a uniform quadratic B-spline
  through the gates' first ends for one edge, and their second ends for
  the other. At parameter `i + f` that is
  `(1-f)²/2 · G[i] + (1/2 + f - f²) · G[i+1] + f²/2 · G[i+2]`.
- **Zones:** at set-up the game cuts the road into zones, two per gate,
  between consecutive cross-sections of the two curves. Each zone's four
  sides are tested against the car (routine `$25b2c`).
- **Collisions:** crossing a side edge is a wall hit; crossing the front
  or back moves the car on to the next or previous zone.
- **Fit:** points 5% in from either edge are on road pixels 95% of the
  time, and points 15% outside only 25% of the time. So the curves follow
  the painted road edge.

The stored start point is on the road for 14 of the 24 tracks read as screen
coordinates, and 10 read as world coordinates. The game seems to use it for
the starting camera position. This port forms the grid up behind gate 0.

### Road and scenery

Colours 8–15 are the driving surface on every track, so the fourth
bitplane is effectively a road mask. The game's physics don't read the
picture; the walls keep cars on the road.

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
- **Settings:** 16 `u16` values (see below). The caravan's values aren't
  driving values; two of them are −4.
- **Menu picture:** 512 bytes. A 64 × 32 picture in two bitplanes, plane 0
  (256 bytes) then plane 1.

Routine `$332a` copies the settings into each car, as 16.16 fixed point
with these shifts. Word `k` is the `k`th `u16`, counting from 0:

| Word | Car field | Value | Meaning |
|------|-----------|-------|---------|
| 0 | `$bc` | w / 64  | drag (computer drivers) |
| 1 | `$e2` | w / 512 | grip (computer drivers) |
| 2 | `$e6` | w / 16  | steering, 64ths of a turn per frame |
| 3 | `$c0` | −w / 64 | stored but never read |
| 4 | `$106` | w / 8  | gravity, pixels per frame² |
| 5 | `$b8` | w / 512 | power (computer drivers) |
| 6 | (towing) | w / 32 | tow hitch distance behind the car, in game units |
| 8 | `$bc` | w / 64  | drag (human drivers) |
| 9 | `$e2` | w / 512 | grip (human drivers) |
| 10 | `$b8` | w / 512 | power (human drivers) |

Words 7 and 11–15 aren't read. [Game logic](#game-logic) shows how drag,
grip and power work together. In short, top speed is
`power × drag / (1 − drag)` game units per frame. For the Camaro that is
271 of this port's units a second for human drivers and 186 for computer
ones.

### Handling presets

The race menu's handling option is CUSTOM (each car's own settings, as
above) or one of six presets that replace them for every car. The table
is at `$21dc6`, 8 bytes each. Bytes 0–2 are for human drivers, 3–5 for
computer drivers, and byte 6 is steering for both:

| Preset | Human | Computer | Steering |
|--------|-------|----------|----------|
| CLASSIC | 98 26 24 | 98 24 24 | 44 |
| GRUNTY | 92 40 30 | 108 33 25 | 60 |
| ZIPPY | 102 61 47 | 115 59 45 | 64 |
| RIPPY | 100 80 60 | 110 50 60 | 72 |
| SLIPPERY | 25 127 101 | 29 45 122 | 68 |
| PEDAL CAR | 117 23 17 | 104 17 10 | 30 |

For a preset `(a, t, g)`:
- drag = a/128 × 0.98 + 0.01
- p = t/16 × (1/drag − 1), and power = p/8
- grip = g/256 / p / drag / 8
- steering = byte 6 / 32, gravity = 0.5

So top speed is simply t/128 game units per frame.

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

## Game logic

`SKIDMARKS_CD³²` is one code hunk of 222,032 bytes, compiled from Blitz
Basic 2. Addresses below are offsets into it. All arithmetic is 16.16
fixed point (multiply at `$2ceda`, divide at `$2e122`).

### Car record

Each car is a 266-byte (`$10a`) record:

| Offset | Field |
|--------|-------|
| `$0`, `$4`, `$8` | x, y, height z |
| `$c`, `$10`, `$14` | velocity x, y, vertical |
| `$18`, `$1c` | this frame's velocity change |
| `$58`, `$5c` | heading (64 per turn) and turn rate |
| `$60`, `$64` | heading vector, from sine/cosine tables of 64 entries |
| `$68` | computer steering, −1, 0 or 1 |
| `$6c`, `$6e` | pitch and roll notch, 0–4 |
| `$76` | controller: 0–7 for joysticks and keyboards, 8 for computer |
| `$78` | sprite frame: roll × 160 + pitch × 32 + heading / 2 |
| `$7a` | airborne |
| `$7e` | state: negative while racing, 1 while warping |
| `$a6` | engine speed |
| `$aa` | forward speed |
| `$b8`, `$bc`, `$e2`, `$e6`, `$106` | handling (above) |
| `$d2`, `$d6` | links to the car towing this one, and the one it tows |
| `$ea` | zone (half-gate) progress |

### One frame

The race loop runs at 50 Hz. Each car gets steering (`$1c618`), drawing
(`$1cafa`), and moving (`$1982c`) in turn. While drawing, a car sprite
is skipped if the beam is already past a given line, so a frame always
fits in one field. Then `$247ac` integrates every car.

1. **Steering:**
   - Input is −1, 0 or 1, from the joystick or the computer driver.
   - turn = (turn + input × steering) / 2. Airborne cars can't steer, and
     their turn rate is limited to ±1.
2. **Height:**
   - vertical −= gravity.
   - The car looks two frames ahead: if `z + 2 × vertical` is still above
     the ground at `pos + 2 × velocity`, it's airborne.
   - Otherwise vertical = (ground there − z) / 2.
   - Pitch and roll each step one notch a frame towards
     `floor(rise + 2.5)`, clamped to 0–4. The rise is the ground height
     change over 0.2 units along world y (pitch) or world −x (roll).
   - heading += turn.
3. **Engine and grip**, skipped while airborne:
   - throttle is −1, 0 or 1. Half the vertical speed is taken off it
     going forwards (added going backwards), so hills slow cars down and
     speed them up.
   - target = (engine speed + throttle × power) × drag
   - change = grip × (heading vector × target − velocity)
   - engine speed += (target − engine speed) / 4
4. **Walls:**
   - The zone test runs on `pos + 4 × velocity + 10 × change`.
   - On a wall the velocity points back off it. The car is held at the
     contact point and loses 1.5× its velocity into the wall, so it
     bounces back at half speed.
   - Crossing a zone's front or back moves `$ea` on, and counts the laps.
5. **Integrate:** velocity += change, position += velocity, z += vertical.

A warping car (state 1) instead flies straight at a point on the track:
its velocity is the distance to it / 16.

### Computer drivers

Computer cars use words 0, 1 and 5 for handling rather than 8, 9 and 10.
They steer with the sign of the cross product between their heading and
the direction to a target point, as if holding a joystick hard over. The
targets come from a table of points built along the road. The driver
picks one side of the road or the other depending on which way the road
bends, and the throttle comes from fixed values.

## Executables

- **`SKIDMARKS_CD³²`** (237 KB): a single-hunk compiled Blitz Basic 2
  program. It includes IFF ILBM/8SVX/ANIM loaders and a tracker replay
  routine with note-period tables.
- **`skidmarks2`** (90 KB): a three-hunk program whose code is packed.

## Gaps

- **Game logic still to decode:**
  - car-to-car collisions
  - exactly how computer drivers pick target points and throttle
  - the warp rules
  - the towing physics, beyond the hitch distance
  - the race rules and scoring
- **Unknown data:**
  - the six per-track words after each track file's palette
  - the 2144 bytes after the gate slots in each track section
  - the colours of `ball.spr`, probably set by the executable

The engine's own editable formats, which `skids extract` converts all of
the above into, are described in [assets/README.md](../assets/README.md).
