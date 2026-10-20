//! Computer drivers, as decoded from the executable's move routine.
//!
//! At set-up the game lays a path of nodes along the middle of the road,
//! 2 game units (32 world units) apart. Each node also has a point on
//! either side, 1 game unit in from the wall. Each frame a computer
//! driver:
//!
//! 1. predicts where it will be in four frames, and finds the first node
//!    that point hasn't reached;
//! 2. aims at the middle of the road four nodes on, at full throttle;
//! 3. but if the predicted point is beyond the line joining the side
//!    points of that node and the one before (running wide on one side),
//!    aims at the other side's point two nodes on, at 70% throttle;
//! 4. steers like a joystick: full lock towards the target, from where it
//!    is now.
//!
//! The cars' computer handling (`vehicle::Handling`) is what lets them take
//! the bends at that. On top of the original, a driver that stops moving
//! backs out.

use super::car::{angle_diff, Car, Controls, DT};
use crate::data::track::Gate;

/// Nodes ahead for the usual target, and for the target across the road.
const AHEAD: usize = 4;
const ACROSS_AHEAD: usize = 2;
/// Throttle while cutting across the road.
const ACROSS_THROTTLE: f32 = 0.7;
/// Distance between nodes, and from each wall to the side points.
const NODE_SPACING: f32 = 32.0;
const SIDE_INSET: f32 = 16.0;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub centre: (f32, f32),
    /// The side points, near edge `a` and near edge `b` of the road.
    pub side_a: (f32, f32),
    pub side_b: (f32, f32),
}

/// The computer drivers' path round the road.
#[derive(Clone, Debug, Default)]
pub struct Path {
    pub nodes: Vec<Node>,
    /// For each road cross-section (`Track::edges`), the first node at or
    /// after it.
    first: Vec<usize>,
}

impl Path {
    pub fn new(edges: &[Gate]) -> Path {
        let m = edges.len();
        let mut path = Path::default();
        if m < 3 {
            return path;
        }
        let mut first = vec![usize::MAX; m];
        // Distance along the middle of the road to the next node.
        let mut to_next = 0.0f32;
        for k in 0..m {
            let (g, h) = (edges[k], edges[(k + 1) % m]);
            let (p, q) = (g.mid(), h.mid());
            let len = (q.0 - p.0).hypot(q.1 - p.1);
            let mut at = to_next;
            while at < len {
                let f = at / len.max(1e-3);
                let lerp =
                    |u: (f32, f32), v: (f32, f32)| (u.0 + (v.0 - u.0) * f, u.1 + (v.1 - u.1) * f);
                let (a, b) = (lerp(g.a, h.a), lerp(g.b, h.b));
                let width = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
                let inset = SIDE_INSET.min(width * 0.5);
                let (ux, uy) = ((b.0 - a.0) / width, (b.1 - a.1) / width);
                if first[k] == usize::MAX {
                    first[k] = path.nodes.len();
                }
                path.nodes.push(Node {
                    centre: lerp(p, q),
                    side_a: (a.0 + ux * inset, a.1 + uy * inset),
                    side_b: (b.0 - ux * inset, b.1 - uy * inset),
                });
                at += NODE_SPACING;
            }
            to_next = at - len;
        }
        // Sections without a node of their own point at the next one.
        let mut next = 0;
        for k in (0..m).rev() {
            if first[k] == usize::MAX {
                first[k] = next;
            } else {
                next = first[k];
            }
        }
        path.first = first;
        path
    }

    fn node(&self, i: usize) -> Node {
        self.nodes[i % self.nodes.len()]
    }

    /// Whether `p` has reached node `i`: it is past the line across the
    /// road there.
    fn reached(&self, i: usize, p: (f32, f32)) -> bool {
        let (here, next) = (self.node(i), self.node(i + 1));
        let (fx, fy) = (next.centre.0 - here.centre.0, next.centre.1 - here.centre.1);
        (p.0 - here.centre.0) * fx + (p.1 - here.centre.1) * fy > 0.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Ai {
    stuck_for: f32,
    reversing: f32,
}

impl Ai {
    pub fn new() -> Ai {
        Ai::default()
    }

    /// Where the driver aims, and how much throttle it uses.
    pub fn target(&self, car: &Car, path: &Path) -> ((f32, f32), f32) {
        let p = (car.x + 4.0 * car.vx * DT, car.y + 4.0 * car.vy * DT);
        let n = path.nodes.len();
        // Start from the car's own place on the road (offset by a lap so
        // stepping back can't underflow), then find the first node the
        // predicted point hasn't reached.
        let mut i = path.first.get(car.section).copied().unwrap_or(0) + n;
        for _ in 0..2 {
            if path.reached(i - 1, p) {
                break;
            }
            i -= 1;
        }
        for _ in 0..8 {
            if !path.reached(i, p) {
                break;
            }
            i += 1;
        }
        let (prev, here) = (path.node(i - 1), path.node(i));
        let across = path.node(i + ACROSS_AHEAD);
        let mut aim = (path.node(i + AHEAD).centre, 1.0);
        if beyond(prev.side_a, here.side_a, here.side_b, p) {
            aim = (across.side_b, ACROSS_THROTTLE);
        }
        if beyond(prev.side_b, here.side_b, here.side_a, p) {
            aim = (across.side_a, ACROSS_THROTTLE);
        }
        aim
    }

    pub fn drive(&mut self, car: &Car, path: &Path, dt: f32, racing: bool) -> Controls {
        if !racing || path.nodes.len() < AHEAD + 2 {
            return Controls::default();
        }
        let ((tx, ty), throttle) = self.target(car, path);
        let diff = angle_diff((ty - car.y).atan2(tx - car.x), car.heading);
        let steer = if diff == 0.0 { 0.0 } else { diff.signum() };

        let speed = car.speed();
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
        Controls {
            throttle,
            brake: 0.0,
            steer,
        }
    }
}

/// Whether `p` is on the far side of the line `a`-`b` from `inside`.
fn beyond(a: (f32, f32), b: (f32, f32), inside: (f32, f32), p: (f32, f32)) -> bool {
    let side = |q: (f32, f32)| (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
    side(p) * side(inside) < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_nodes_are_evenly_spaced_and_inset() {
        // A straight road 40 units wide along x, as a loop of sections.
        let edges: Vec<Gate> = (0..40)
            .map(|i| Gate {
                a: (i as f32 * 10.0, 0.0),
                b: (i as f32 * 10.0, 40.0),
            })
            .collect();
        let path = Path::new(&edges[..20]);
        let n = &path.nodes;
        assert!(n.len() >= 5);
        let d = (n[1].centre.0 - n[0].centre.0).hypot(n[1].centre.1 - n[0].centre.1);
        assert!((d - NODE_SPACING).abs() < 1e-3);
        assert_eq!(n[0].side_a, (0.0, 16.0));
        assert_eq!(n[0].side_b, (0.0, 24.0));
    }
}
