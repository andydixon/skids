//! Computer drivers.
//!
//! As in the executable, a computer car steers like a joystick: full lock
//! towards a target point ahead, or straight on when it is dead ahead, and
//! drives flat out. The cars' computer handling (`vehicle::Handling`) is
//! what makes them take bends at that. The target is a point a little way
//! along the road (`Track::edges`) at the driver's own line across it. A
//! driver that stops moving backs out.

use super::car::{angle_diff, Car, Controls};
use crate::data::track::Track;

/// Heading error, in radians, below which the driver doesn't steer.
const DEAD_AHEAD: f32 = 0.06;

#[derive(Clone, Debug)]
pub struct Ai {
    /// Where across the road this driver aims (0 = one edge, 1 = the other).
    pub line: f32,
    /// Fraction of full throttle this driver uses.
    pub pace: f32,
    stuck_for: f32,
    reversing: f32,
}

impl Ai {
    pub fn new(line: f32, pace: f32) -> Ai {
        Ai {
            line,
            pace,
            stuck_for: 0.0,
            reversing: 0.0,
        }
    }

    /// The point `dist` along the road from the car's cross-section, at
    /// this driver's line.
    pub fn target(&self, car: &Car, track: &Track, dist: f32) -> (f32, f32) {
        let e = &track.edges;
        let m = e.len();
        let at = |k: usize| {
            let g = e[k % m];
            (
                g.a.0 + (g.b.0 - g.a.0) * self.line,
                g.a.1 + (g.b.1 - g.a.1) * self.line,
            )
        };
        let mut k = car.section + 1;
        let mut p = at(k);
        let mut left = dist - (p.0 - car.x).hypot(p.1 - car.y);
        while left > 0.0 && k < car.section + m {
            let q = at(k + 1);
            let len = (q.0 - p.0).hypot(q.1 - p.1);
            if len >= left {
                let f = left / len;
                return (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f);
            }
            left -= len;
            p = q;
            k += 1;
        }
        p
    }

    pub fn drive(&mut self, car: &Car, track: &Track, dt: f32, racing: bool) -> Controls {
        if !racing || track.edges.len() < 3 {
            return Controls::default();
        }
        let speed = car.speed();
        let look = 30.0 + speed * 0.2;
        let (tx, ty) = self.target(car, track, look);
        let diff = angle_diff((ty - car.y).atan2(tx - car.x), car.heading);
        let steer = if diff.abs() < DEAD_AHEAD {
            0.0
        } else {
            diff.signum()
        };

        if self.reversing > 0.0 {
            self.reversing -= dt;
            return Controls {
                throttle: 0.0,
                brake: 1.0,
                steer: -steer,
            };
        }
        if speed < 10.0 {
            self.stuck_for += dt;
            if self.stuck_for > 1.0 {
                self.stuck_for = 0.0;
                self.reversing = 0.8;
            }
        } else {
            self.stuck_for = 0.0;
        }
        if diff.abs() > 2.3 && speed < 35.0 {
            // Facing the wrong way: reverse-turn round.
            return Controls {
                throttle: 0.0,
                brake: 1.0,
                steer: -steer,
            };
        }
        Controls {
            throttle: self.pace,
            brake: 0.0,
            steer,
        }
    }
}
