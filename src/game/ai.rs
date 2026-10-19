//! Computer drivers.
//!
//! Each driver follows a line through the gates (at its own offset across
//! the track) by pure pursuit: aim at a point a speed-dependent distance
//! further along the line. Short look-ahead rays nudge the chosen heading
//! away from the grass, and a driver that stops moving backs out.

use super::car::{angle_diff, Car, Controls};
use crate::data::track::{Gate, Track};

const RAYS: i32 = 7;
const RAY_STEP: f32 = 0.13;

#[derive(Clone, Debug)]
pub struct Ai {
    /// Where across each gate this driver aims (0 = end a, 1 = end b).
    pub line: f32,
    /// Fraction of the car's top speed this driver uses.
    pub pace: f32,
    stuck_for: f32,
    blocked_for: f32,
    reversing: f32,
}

impl Ai {
    pub fn new(line: f32, pace: f32) -> Ai {
        Ai {
            line,
            pace,
            stuck_for: 0.0,
            blocked_for: 0.0,
            reversing: 0.0,
        }
    }

    fn point(&self, g: &Gate) -> (f32, f32) {
        (
            g.a.0 + (g.b.0 - g.a.0) * self.line,
            g.a.1 + (g.b.1 - g.a.1) * self.line,
        )
    }

    /// Point `dist` along the driving line from the car's projection onto it.
    fn pursue(&mut self, car: &Car, gates: &[Gate], next_gate: usize, dist: f32) -> (f32, f32) {
        let n = gates.len();
        // Only the stretch into the gate the lap counter needs next, or the
        // one after it (crossing counts with one gate skipped), so a car
        // knocked backwards turns round for the gate it missed.
        let seg = next_gate;
        let mut best = (f32::MAX, seg, 0.0f32);
        for k in 0..2 {
            let i = (seg + k) % n;
            let a = self.point(&gates[(i + n - 1) % n]);
            let b = self.point(&gates[i]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = (dx * dx + dy * dy).max(1e-3);
            let t = (((car.x - a.0) * dx + (car.y - a.1) * dy) / len2).clamp(0.0, 1.0);
            let d = (a.0 + dx * t - car.x).hypot(a.1 + dy * t - car.y);
            if d < best.0 - 1.0 {
                best = (d, i, t);
            }
        }
        let (_, mut i, t) = best;

        let mut a = self.point(&gates[(i + n - 1) % n]);
        let mut b = self.point(&gates[i]);
        let mut remaining = dist + (b.0 - a.0).hypot(b.1 - a.1) * t;
        for _ in 0..n {
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            if remaining <= len {
                let k = remaining / len.max(1e-3);
                return (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
            }
            remaining -= len;
            i = (i + 1) % n;
            a = b;
            b = self.point(&gates[i]);
        }
        b
    }

    pub fn drive(
        &mut self,
        car: &Car,
        track: &Track,
        next_gate: usize,
        dt: f32,
        racing: bool,
    ) -> Controls {
        if !racing {
            return Controls::default();
        }
        let gates = &track.gates;
        let speed = car.speed();
        let look = 34.0 + speed * 0.32;
        let target = self.pursue(car, gates, next_gate, look);
        let far = self.pursue(car, gates, next_gate, look * 2.2);

        let want = (target.1 - car.y).atan2(target.0 - car.x);

        if self.reversing > 0.0 {
            self.reversing -= dt;
            let diff = angle_diff(want, car.heading);
            return Controls {
                throttle: 0.0,
                brake: 1.0,
                steer: -diff.signum(),
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

        // Pick the heading closest to the target that keeps us on the road.
        let ray_len = 18.0 + speed * 0.25;
        let mut best = (f32::MAX, want);
        for k in -RAYS..=RAYS {
            let cand = want + k as f32 * RAY_STEP;
            let mut grass = 0.0;
            for step in 1..=4 {
                let d = ray_len * step as f32 / 4.0;
                if !track.on_road_world(car.x + cand.cos() * d, car.y + cand.sin() * d) {
                    grass += 1.0 / step as f32;
                }
            }
            let cost = (k as f32 * RAY_STEP).abs() + grass * 0.9;
            if cost < best.0 {
                best = (cost, cand);
            }
        }
        let diff = angle_diff(best.1, car.heading);
        let steer = (diff * 3.0).clamp(-1.0, 1.0);
        // Boxed in (nose in the scenery or against the edge): back out.
        if best.0 > 1.6 && speed < 30.0 {
            self.blocked_for += dt;
            if self.blocked_for > 0.5 {
                self.blocked_for = 0.0;
                self.reversing = 0.7;
            }
        } else {
            self.blocked_for = 0.0;
        }

        // Slow for what is coming: compare heading with the far probe.
        let bend = angle_diff((far.1 - car.y).atan2(far.0 - car.x), car.heading).abs();
        let corner_speed = car.spec.top * self.pace * (1.0 - (bend / 1.3).min(0.75));
        let (mut throttle, mut brake) = (1.0, 0.0);
        if speed > corner_speed {
            throttle = 0.0;
            if speed > corner_speed + 25.0 {
                brake = 0.7;
            }
        } else if diff.abs() > 0.8 {
            throttle = 0.6;
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
            throttle,
            brake,
            steer,
        }
    }
}
