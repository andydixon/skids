//! Vehicle handling, as decoded from the executable.
//!
//! The original runs one update per 50 Hz frame in its own units, where one
//! world unit is a height-map cell (`track::CELL` of ours). Each frame:
//!
//! - **Steering:** spin = (spin + steer * turn) / 2, then the heading
//!   turns by spin (64ths of a turn). Steering is digital, as the
//!   joystick is.
//! - **Height:** vertical speed loses `gravity`. If the car would still be
//!   above the ground two frames ahead it's airborne; otherwise its
//!   vertical speed is half the climb to that point. Body tilt steps one
//!   notch a frame towards the slope under the car.
//! - **Engine and grip:** see `vehicle::Handling`. Climbing takes half the
//!   vertical speed off the throttle; descending adds it.
//! - **Move:** velocity is added to the position. Airborne cars keep
//!   their velocity and engine speed.
//! - **Walls:** the road edges (`Track::edges`) are walls. A car that
//!   reaches one loses the part of its velocity into the wall and bounces
//!   back with half of it.
//! - **Other cars:** see `bump`.
//!
//! Human and computer drivers use separate handling from the vehicle's
//! `.set` values.

pub use super::DT;
use crate::data::track::{self, Gate, Track, CELL};
use crate::data::vehicle::{self, Handling, Spec};
use std::f32::consts::{PI, TAU};

/// How close the car's centre may come to a wall, in world units.
const WALL_MARGIN: f32 = 4.0;
/// The executable samples the slope 0.2 cells from the car.
const TILT_PROBE: f32 = 0.2 * CELL;

#[derive(Clone, Copy, Default, Debug)]
pub struct Controls {
    pub throttle: f32,
    pub brake: f32,
    /// -1 = left, 1 = right.
    pub steer: f32,
}

#[derive(Clone, Debug)]
pub struct Car {
    pub vehicle: usize,
    pub spec: Spec,
    pub team: usize,
    /// Uses the vehicle's computer-driver handling.
    pub computer: bool,
    pub x: f32,
    pub y: f32,
    /// Height above the ground plane, pixels.
    pub z: f32,
    /// World units per second.
    pub vx: f32,
    pub vy: f32,
    /// Pixels per frame.
    pub vz: f32,
    /// World angle, 0 = +x, increasing clockwise on screen.
    pub heading: f32,
    /// Turn per frame, 64ths of a turn.
    pub spin: f32,
    /// The engine's speed, in the original's units per frame.
    pub drive: f32,
    pub airborne: bool,
    pub controls: Controls,
    /// Sideways speed this frame, used for skid marks and sound.
    pub slip: f32,
    pub on_road: bool,
    /// Tilt notches 0-4 (2 = level): pitch follows the slope along world y
    /// and roll the slope along world x, as in the executable.
    pub pitch: i32,
    pub roll: i32,
    pub tilt: usize,
    /// Throttle/rpm proxy for the engine sound, 0..1.
    pub rpm: f32,
    /// Strength of the last collision, decays each frame.
    pub bump: f32,
    /// The road cross-section (`Track::edges`) the car is past.
    pub section: usize,
}

impl Car {
    pub fn new(vehicle: usize, spec: Spec, team: usize, x: f32, y: f32, heading: f32) -> Car {
        Car {
            vehicle,
            spec,
            team,
            computer: false,
            x,
            y,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            heading,
            spin: 0.0,
            drive: 0.0,
            airborne: false,
            controls: Controls::default(),
            slip: 0.0,
            on_road: true,
            pitch: 2,
            roll: 2,
            tilt: vehicle::LEVEL,
            rpm: 0.0,
            bump: 0.0,
            section: 0,
        }
    }

    /// Puts the car on the ground at its position, stopped, and finds its
    /// place along the road.
    pub fn place(&mut self, track: &Track) {
        self.z = track.ground(self.x, self.y);
        self.vx = 0.0;
        self.vy = 0.0;
        self.vz = 0.0;
        self.spin = 0.0;
        self.drive = 0.0;
        self.airborne = false;
        self.section = nearest_section(&track.edges, (self.x, self.y));
    }

    pub fn screen_pos(&self) -> (f32, f32) {
        track::project(self.x, self.y, self.z)
    }

    pub fn speed(&self) -> f32 {
        self.vx.hypot(self.vy)
    }

    pub fn forward(&self) -> (f32, f32) {
        (self.heading.cos(), self.heading.sin())
    }

    pub fn handling(&self) -> Handling {
        if self.computer {
            self.spec.computer
        } else {
            self.spec.player
        }
    }

    /// Top speed on the flat, world units per second.
    pub fn top_speed(&self) -> f32 {
        let h = self.handling();
        if h.drag >= 1.0 {
            return f32::INFINITY;
        }
        h.power * h.drag / (1.0 - h.drag) * CELL / DT
    }

    /// One frame. `pace` scales the throttle (finished cars cruise).
    pub fn step(&mut self, track: &Track, pace: f32) {
        let c = self.controls;
        let h = self.handling();
        // Velocity in world units per frame.
        let (mut vx, mut vy) = (self.vx * DT, self.vy * DT);

        if self.airborne {
            self.spin = self.spin.clamp(-1.0, 1.0);
        } else {
            let steer = c.steer.clamp(-1.0, 1.0);
            self.spin = (self.spin + steer * self.spec.turn) * 0.5;
        }

        self.vz -= self.spec.gravity;
        let (ax, ay) = (self.x + 2.0 * vx, self.y + 2.0 * vy);
        let ground = track.ground(ax, ay);
        if self.z + 2.0 * self.vz > ground {
            self.airborne = true;
        } else {
            self.airborne = false;
            self.vz = (ground - self.z) * 0.5;
            let notch = |probe: f32| ((probe - ground + 2.5).floor() as i32).clamp(0, 4);
            let pitch = notch(track.ground(ax, ay + TILT_PROBE));
            let roll = notch(track.ground(ax - TILT_PROBE, ay));
            self.pitch += (pitch - self.pitch).signum();
            self.roll += (roll - self.roll).signum();
        }
        self.tilt = vehicle::tilt_block(self.pitch - 2, self.roll - 2);
        self.heading = (self.heading + self.spin * TAU / 64.0).rem_euclid(TAU);

        let (fx, fy) = self.forward();
        if !self.airborne {
            // The original's units: world cells per frame.
            let (cx, cy) = (vx / CELL, vy / CELL);
            let along = cx * fx + cy * fy;
            let mut throttle = (c.throttle - c.brake).clamp(-1.0, 1.0) * pace;
            throttle += if along < 0.0 { self.vz } else { -self.vz } * 0.5;
            let target = (self.drive + throttle * h.power) * h.drag;
            let (sx, sy) = (h.grip * (fx * target - cx), h.grip * (fy * target - cy));
            self.drive += (target - self.drive) * 0.25;
            vx += sx * CELL;
            vy += sy * CELL;
        }

        let (ox, oy) = (self.x, self.y);
        self.x += vx;
        self.y += vy;
        self.z += self.vz;
        self.vx = vx / DT;
        self.vy = vy / DT;
        self.walls(track);
        self.keep_in_bounds(track, ox, oy);

        self.on_road = track.on_road_world(self.x, self.y);
        self.slip = (self.vy * fx - self.vx * fy).abs();
        let top = self.top_speed().max(1.0);
        let fwd = (self.vx * fx + self.vy * fy).abs();
        let target_rpm = if c.throttle > 0.0 {
            0.35 + 0.65 * (fwd / top).min(1.0)
        } else {
            (fwd / top).min(1.0) * 0.7
        };
        self.rpm += (target_rpm - self.rpm) * (1.0 - (-6.0 * DT).exp());
        self.bump *= (-8.0 * DT).exp();
    }

    /// Keeps the car between the road edges of the section it is in, and
    /// moves `section` along as the car crosses from one to the next.
    fn walls(&mut self, track: &Track) {
        let e = &track.edges;
        let m = e.len();
        if m < 3 {
            return;
        }
        let p = (self.x, self.y);
        for _ in 0..m {
            let next = (self.section + 1) % m;
            if !past(e, next, p) {
                break;
            }
            self.section = next;
        }
        for _ in 0..m {
            if past(e, self.section, p) {
                break;
            }
            self.section = (self.section + m - 1) % m;
        }
        let (s0, s1) = (e[self.section], e[(self.section + 1) % m]);
        for (p0, p1, inside) in [(s0.a, s1.a, s0.b), (s0.b, s1.b, s0.a)] {
            let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
            let len = dx.hypot(dy);
            if len < 1e-3 {
                continue;
            }
            // Unit normal pointing into the road.
            let (mut nx, mut ny) = (-dy / len, dx / len);
            if nx * (inside.0 - p0.0) + ny * (inside.1 - p0.1) < 0.0 {
                nx = -nx;
                ny = -ny;
            }
            let dist = nx * (self.x - p0.0) + ny * (self.y - p0.1);
            if dist >= WALL_MARGIN {
                continue;
            }
            self.x += nx * (WALL_MARGIN - dist);
            self.y += ny * (WALL_MARGIN - dist);
            let vn = self.vx * nx + self.vy * ny;
            if vn < 0.0 {
                self.vx -= 1.5 * vn * nx;
                self.vy -= 1.5 * vn * ny;
                self.bump = self.bump.max((-vn / 200.0).min(0.6));
            }
        }
    }

    /// The picture edge is a wall too, for tracks whose road edges run off
    /// it. Slide along it by keeping whichever axis of the move stays
    /// inside, so cars never get pinned in a corner.
    fn keep_in_bounds(&mut self, track: &Track, ox: f32, oy: f32) {
        const MARGIN: f32 = 3.0;
        let inside = |x: f32, y: f32| {
            let (sx, sy) = track.to_screen(x, y);
            track::on_screen(sx, sy, MARGIN)
        };
        if inside(self.x, self.y) {
            return;
        }
        let hit = self.speed() / 200.0;
        if inside(self.x, oy) {
            self.y = oy;
            self.vy *= -0.3;
        } else if inside(ox, self.y) {
            self.x = ox;
            self.vx *= -0.3;
        } else {
            self.x = ox;
            self.y = oy;
            self.vx *= -0.3;
            self.vy *= -0.3;
        }
        self.bump = self.bump.max(hit.min(0.6));
    }

    /// Rear wheel contact points in screen space, for skid marks.
    pub fn rear_wheels(&self, track: &Track) -> [(f32, f32); 2] {
        let (fx, fy) = self.forward();
        let (rx, ry) = (-fy, fx);
        let (bx, by) = (self.x - fx * 10.0, self.y - fy * 10.0);
        [
            track.to_screen(bx + rx * 6.0, by + ry * 6.0),
            track.to_screen(bx - rx * 6.0, by - ry * 6.0),
        ]
    }
}

/// Whether `p` is past cross-section `k`, going round the road.
fn past(e: &[Gate], k: usize, p: (f32, f32)) -> bool {
    let m = e.len();
    let (a, b) = (e[k].a, e[k].b);
    let side = |q: (f32, f32)| (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
    side(p) * side(e[(k + 1) % m].mid()) > 0.0
}

/// The cross-section just behind the point closest to `p`.
pub fn nearest_section(e: &[Gate], p: (f32, f32)) -> usize {
    let mut best = (f32::MAX, 0);
    for (k, g) in e.iter().enumerate() {
        let (mx, my) = g.mid();
        let d = (mx - p.0).hypot(my - p.1);
        if d < best.0 {
            best = (d, k);
        }
    }
    let k = best.1;
    let m = e.len().max(1);
    if past(e, k, p) || e.len() < 3 {
        k
    } else {
        (k + m - 1) % m
    }
}

/// Something a car can run into: another car or a caravan.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub heading: f32,
}

impl Car {
    pub fn body(&self) -> Body {
        Body {
            x: self.x,
            y: self.y,
            z: self.z,
            vx: self.vx,
            vy: self.vy,
            heading: self.heading,
        }
    }
}

/// Half-width and half-length of the box that `bump` tests points against,
/// world units. The executable uses the same box for every vehicle.
const TARGET: (f32, f32) = (6.0, 12.0);

/// The executable's collision (`$25e64`) between car `a` and `b`.
///
/// Six points on `a` are tested against `b`'s box: its four corners and a
/// point on each side, 0.58 of the way out. Each point inside the box
/// pushes the two apart along that point's offset from `a`'s centre, by
/// k = 64 |relative speed|^2 + 20 (game units); the side points push 2.5
/// times as hard. Each corner inside also spins `a` so that corner swings
/// away. Returns the velocity change for `b` (world units per second);
/// `a` has already received the opposite.
pub fn bump(a: &mut Car, b: &Body) -> Option<(f32, f32)> {
    // Cars more than two cells apart, or at different heights (on and
    // under a bridge), can't touch.
    if (a.x - b.x).abs() > 2.0 * CELL || (a.y - b.y).abs() > 2.0 * CELL || (a.z - b.z).abs() >= 17.0
    {
        return None;
    }
    let (dvx, dvy) = ((a.vx - b.vx) * DT / CELL, (a.vy - b.vy) * DT / CELL);
    let k = 64.0 * (dvx * dvx + dvy * dvy) + 20.0;
    let (fx, fy) = a.forward();
    let (ux, uy) = (-fy, fx);
    let (bfx, bfy) = (b.heading.cos(), b.heading.sin());
    let (l, w) = (a.spec.half_length, a.spec.half_width);
    let side = w * 0.58;
    // (along, across, push factor)
    let points = [
        (l, w, 1.0),
        (l, -w, 1.0),
        (-l, -w, 1.0),
        (-l, w, 1.0),
        (0.0, side, 2.5),
        (0.0, -side, 2.5),
    ];
    let (mut ix, mut iy, mut turn, mut hit) = (0.0, 0.0, 0.0, false);
    for (along, across, factor) in points {
        let (px, py) = (fx * along + ux * across, fy * along + uy * across);
        let (rx, ry) = (a.x + px - b.x, a.y + py - b.y);
        let b_along = rx * bfx + ry * bfy;
        let b_across = ry * bfx - rx * bfy;
        if b_across.abs() >= TARGET.0 || b_along.abs() >= TARGET.1 {
            continue;
        }
        hit = true;
        let push = k * factor / 256.0 / DT;
        ix += px * push;
        iy += py * push;
        if factor == 1.0 {
            turn -= (along * across).signum() * k / 16.0;
        }
    }
    if !hit {
        return None;
    }
    a.vx -= ix;
    a.vy -= iy;
    a.spin += turn;
    a.heading = (a.heading + 2.0 * turn * TAU / 64.0).rem_euclid(TAU);
    let knock = (dvx.hypot(dvy) * CELL / DT / 150.0).clamp(0.2, 1.0);
    a.bump = a.bump.max(knock);
    Some((ix, iy))
}

pub fn angle_diff(to: f32, from: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}
