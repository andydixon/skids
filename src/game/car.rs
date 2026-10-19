//! Vehicle handling.
//!
//! The retail executable is not available, so handling is a reconstruction:
//! an arcade model with separate forward and lateral friction, which gives
//! the long power-slides the series is known for. Physics runs on the
//! isometric ground plane (see `data::track::to_screen`).

use crate::data::track::{self, Track};
use crate::data::vehicle;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// Top speed on tarmac, world pixels per second.
    pub top: f32,
    pub accel: f32,
    pub brake: f32,
    /// Lateral grip: how quickly sideways motion is killed (1/s).
    pub grip: f32,
    /// Steering rate at speed (rad/s).
    pub turn: f32,
    pub mass: f32,
    /// Speed and grip multiplier off the road.
    pub offroad: f32,
}

pub fn spec_for(id: &str) -> Spec {
    let base = Spec {
        top: 200.0,
        accel: 170.0,
        brake: 320.0,
        grip: 5.0,
        turn: 3.6,
        mass: 1.0,
        offroad: 0.55,
    };
    match id {
        "f1" => Spec {
            top: 240.0,
            accel: 210.0,
            grip: 6.5,
            turn: 3.7,
            mass: 0.9,
            offroad: 0.45,
            ..base
        },
        "porsche" => Spec {
            top: 225.0,
            accel: 190.0,
            grip: 5.2,
            turn: 3.6,
            ..base
        },
        "mini" => Spec {
            top: 190.0,
            accel: 200.0,
            grip: 5.8,
            turn: 4.4,
            mass: 0.8,
            ..base
        },
        "vw" => Spec {
            top: 185.0,
            accel: 160.0,
            grip: 4.6,
            turn: 3.6,
            mass: 1.0,
            ..base
        },
        "midget" => Spec {
            top: 205.0,
            accel: 185.0,
            grip: 4.0,
            turn: 4.0,
            mass: 0.85,
            offroad: 0.6,
            ..base
        },
        "truck" => Spec {
            top: 195.0,
            accel: 140.0,
            grip: 4.2,
            turn: 3.0,
            mass: 1.8,
            offroad: 0.75,
            ..base
        },
        "police" => Spec {
            top: 215.0,
            accel: 180.0,
            grip: 5.0,
            turn: 3.5,
            mass: 1.2,
            ..base
        },
        "cow" => Spec {
            top: 170.0,
            accel: 150.0,
            grip: 7.0,
            turn: 4.6,
            mass: 1.4,
            offroad: 0.9,
            ..base
        },
        _ => base,
    }
}

pub const RADIUS: f32 = 11.0;
const SLOPE_GRAVITY: f32 = 110.0;
/// Cap on slope acceleration so the steep banking on some tracks cannot pin
/// a car against the outside of a bend.
const SLOPE_MAX: f32 = 45.0;

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
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// World angle, 0 = +x, increasing clockwise on screen.
    pub heading: f32,
    pub controls: Controls,
    /// Sideways speed this frame, used for skid marks and sound.
    pub slip: f32,
    pub on_road: bool,
    pub tilt: usize,
    /// Throttle/rpm proxy for the engine sound, 0..1.
    pub rpm: f32,
    /// Strength of the last collision, decays each frame.
    pub bump: f32,
}

impl Car {
    pub fn new(vehicle: usize, id: &str, team: usize, x: f32, y: f32, heading: f32) -> Car {
        Car {
            vehicle,
            spec: spec_for(id),
            team,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            heading,
            controls: Controls::default(),
            slip: 0.0,
            on_road: true,
            tilt: vehicle::LEVEL,
            rpm: 0.0,
            bump: 0.0,
        }
    }

    pub fn screen_pos(&self) -> (f32, f32) {
        track::to_screen(self.x, self.y)
    }

    pub fn speed(&self) -> f32 {
        self.vx.hypot(self.vy)
    }

    pub fn forward(&self) -> (f32, f32) {
        (self.heading.cos(), self.heading.sin())
    }

    pub fn step(&mut self, track: &Track, dt: f32, top_scale: f32) {
        self.on_road = track.on_road_world(self.x, self.y);
        let surf = if self.on_road { 1.0 } else { self.spec.offroad };
        let c = self.controls;
        let (fx, fy) = self.forward();
        let (rx, ry) = (-fy, fx);
        let mut vf = self.vx * fx + self.vy * fy;
        let mut vr = self.vx * rx + self.vy * ry;
        let top = self.spec.top * surf * top_scale;

        if c.throttle > 0.0 && vf < top {
            let headroom = (1.0 - (vf / top).max(0.0).powi(2)).max(0.05);
            vf += self.spec.accel * c.throttle * headroom * dt;
        }
        if c.brake > 0.0 {
            if vf > 4.0 {
                vf = (vf - self.spec.brake * c.brake * dt).max(0.0);
            } else if vf > -top * 0.35 {
                vf -= self.spec.accel * 0.6 * c.brake * dt;
            }
        }
        let drag = if self.on_road { 0.35 } else { 1.6 };
        vf -= vf * drag * dt;
        if c.throttle == 0.0 && c.brake == 0.0 {
            vf -= vf.signum() * (30.0 * dt).min(vf.abs());
        }

        // Sideways friction: grip drops as the slide gets faster, so once a
        // car breaks away it keeps sliding.
        let grip = self.spec.grip * surf.max(0.6) / (1.0 + vr.abs() / 120.0);
        vr *= (-grip * dt).exp();
        self.slip = vr.abs();

        let steer_auth = (vf.abs() / 45.0).min(1.0) * vf.signum();
        let turn = self.spec.turn * (1.0 - (vf.abs() / (top * 2.2)).min(0.35));
        self.heading = (self.heading + c.steer * turn * steer_auth * dt).rem_euclid(TAU);

        self.vx = fx * vf + rx * vr;
        self.vy = fy * vf + ry * vr;

        // Hills: the height map is sampled in screen space.
        let (gx, gy) = gradient(track, self.x, self.y);
        let (mut ax, mut ay) = (-gx * SLOPE_GRAVITY, -gy * SLOPE_GRAVITY);
        let a = ax.hypot(ay);
        if a > SLOPE_MAX {
            ax *= SLOPE_MAX / a;
            ay *= SLOPE_MAX / a;
        }
        self.vx += ax * dt;
        self.vy += ay * dt;

        let (ox, oy) = (self.x, self.y);
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        self.keep_in_bounds(ox, oy);

        // Body tilt from the slope along and across the car.
        let (fx, fy) = self.forward();
        let along = gx * fx + gy * fy;
        let across = gx * -fy + gy * fx;
        let q = |s: f32| (s / 0.035).round() as i32;
        self.tilt = vehicle::tilt_block(-q(along), q(across));

        let target_rpm = if c.throttle > 0.0 {
            0.35 + 0.65 * (vf.abs() / top).min(1.0)
        } else {
            (vf.abs() / top).min(1.0) * 0.7
        };
        self.rpm += (target_rpm - self.rpm) * (1.0 - (-6.0 * dt).exp());
        self.bump *= (-8.0 * dt).exp();
    }

    /// The picture edge is a wall. Slide along it by keeping whichever axis
    /// of the move stays inside, so cars never get pinned in a corner.
    fn keep_in_bounds(&mut self, ox: f32, oy: f32) {
        const MARGIN: f32 = 3.0;
        let inside = |x: f32, y: f32| {
            let (sx, sy) = track::to_screen(x, y);
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
    pub fn rear_wheels(&self) -> [(f32, f32); 2] {
        let (fx, fy) = self.forward();
        let (rx, ry) = (-fy, fx);
        let (bx, by) = (self.x - fx * 10.0, self.y - fy * 10.0);
        [
            track::to_screen(bx + rx * 6.0, by + ry * 6.0),
            track::to_screen(bx - rx * 6.0, by - ry * 6.0),
        ]
    }
}

/// Height change per world unit along world x and y.
fn gradient(track: &Track, wx: f32, wy: f32) -> (f32, f32) {
    let d = 6.0;
    let gx = (track.height_world(wx + d, wy) - track.height_world(wx - d, wy)) / (2.0 * d);
    let gy = (track.height_world(wx, wy + d) - track.height_world(wx, wy - d)) / (2.0 * d);
    (gx, gy)
}

/// Pushes overlapping cars apart and exchanges momentum.
pub fn collide(a: &mut Car, b: &mut Car) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d2 = dx * dx + dy * dy;
    let min = RADIUS * 2.0;
    if d2 >= min * min || d2 < 1e-6 {
        return;
    }
    let d = d2.sqrt();
    let (nx, ny) = (dx / d, dy / d);
    let overlap = min - d;
    let (ia, ib) = (1.0 / a.spec.mass, 1.0 / b.spec.mass);
    let push = overlap / (ia + ib);
    a.x -= nx * push * ia;
    a.y -= ny * push * ia;
    b.x += nx * push * ib;
    b.y += ny * push * ib;

    let vn = (b.vx - a.vx) * nx + (b.vy - a.vy) * ny;
    if vn < 0.0 {
        let j = -(1.0 + 0.45) * vn / (ia + ib);
        a.vx -= j * ia * nx;
        a.vy -= j * ia * ny;
        b.vx += j * ib * nx;
        b.vy += j * ib * ny;
        let hit = (-vn / 150.0).min(1.0);
        a.bump = a.bump.max(hit);
        b.bump = b.bump.max(hit);
        // A glancing knock spins the cars a little.
        let spin = (nx * a.forward().1 - ny * a.forward().0) * hit * 0.25;
        a.heading = (a.heading + spin).rem_euclid(TAU);
        b.heading = (b.heading - spin).rem_euclid(TAU);
    }
}

pub fn angle_diff(to: f32, from: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}
