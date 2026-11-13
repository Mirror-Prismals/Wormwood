//! Conservative spatial rejection for the original capsule/ellipsoid union.
//! Leaves evaluate the original formulas; no field samples or details are removed.
use super::{Chamber, Tunnel, V, capsule};
use crate::audio::{length, sub};
#[derive(Clone)]
struct Bound {
    min: V,
    max: V,
    ratio: f64,
    radius: f64,
}
impl Bound {
    fn lower(&self, p: V) -> f64 {
        let gap: V = std::array::from_fn(|i| (self.min[i] - p[i]).max(p[i] - self.max[i]).max(0.));
        let outside = length(gap);
        let bound = if outside > 0. {
            outside * self.ratio
        } else {
            -self.radius
        };
        // Outward allowance for floating-point bound arithmetic, never for leaf values.
        bound - 1e-9 * (1. + bound.abs())
    }
    fn union(a: &Self, b: &Self) -> Self {
        Self {
            min: std::array::from_fn(|i| a.min[i].min(b.min[i])),
            max: std::array::from_fn(|i| a.max[i].max(b.max[i])),
            ratio: a.ratio.min(b.ratio),
            radius: a.radius.max(b.radius),
        }
    }
}
struct Shape {
    bound: Bound,
    id: usize,
}
struct Node {
    bound: Bound,
    children: Option<[usize; 2]>,
    ids: Vec<usize>,
}
pub(super) struct VoidIndex {
    nodes: Vec<Node>,
}
impl VoidIndex {
    pub fn build(tunnels: &[Tunnel], chambers: &[Chamber]) -> Self {
        let mut shapes: Vec<_> = tunnels
            .iter()
            .enumerate()
            .map(|(id, t)| Shape {
                id,
                bound: Bound {
                    min: std::array::from_fn(|i| t.start[i].min(t.end[i]) - t.radius),
                    max: std::array::from_fn(|i| t.start[i].max(t.end[i]) + t.radius),
                    ratio: 1.,
                    radius: t.radius,
                },
            })
            .collect();
        shapes.extend(chambers.iter().enumerate().map(|(i, c)| {
            let lo = c.radii.into_iter().fold(f64::INFINITY, f64::min);
            let hi = c.radii.into_iter().fold(0f64, f64::max);
            Shape {
                id: tunnels.len() + i,
                bound: Bound {
                    min: std::array::from_fn(|i| c.center[i] - c.radii[i]),
                    max: std::array::from_fn(|i| c.center[i] + c.radii[i]),
                    ratio: lo / hi,
                    radius: hi,
                },
            }
        }));
        let mut tree = Self { nodes: Vec::new() };
        if !shapes.is_empty() {
            tree.add(&mut shapes);
        }
        tree
    }
    fn add(&mut self, shapes: &mut [Shape]) -> usize {
        let bound = shapes
            .iter()
            .skip(1)
            .fold(shapes[0].bound.clone(), |a, s| Bound::union(&a, &s.bound));
        let index = self.nodes.len();
        let axis = (0..3)
            .max_by(|&a, &b| {
                (bound.max[a] - bound.min[a]).total_cmp(&(bound.max[b] - bound.min[b]))
            })
            .unwrap();
        self.nodes.push(Node {
            bound,
            children: None,
            ids: Vec::new(),
        });
        if shapes.len() <= 4 {
            self.nodes[index].ids = shapes.iter().map(|s| s.id).collect();
        } else {
            shapes.sort_by(|a, b| {
                (a.bound.min[axis] + a.bound.max[axis])
                    .total_cmp(&(b.bound.min[axis] + b.bound.max[axis]))
            });
            let half = shapes.len() / 2;
            let (a, b) = shapes.split_at_mut(half);
            let l = self.add(a);
            let r = self.add(b);
            self.nodes[index].children = Some([l, r]);
        }
        index
    }
    pub fn distance(&self, p: V, tunnels: &[Tunnel], chambers: &[Chamber]) -> f64 {
        let mut best = f64::INFINITY;
        if !self.nodes.is_empty() {
            self.visit(0, p, tunnels, chambers, &mut best);
        }
        best
    }
    fn visit(&self, i: usize, p: V, tunnels: &[Tunnel], chambers: &[Chamber], best: &mut f64) {
        let node = &self.nodes[i];
        if node.bound.lower(p) >= *best {
            return;
        }
        if let Some([a, b]) = node.children {
            let (a, b) = if self.nodes[a].bound.lower(p) <= self.nodes[b].bound.lower(p) {
                (a, b)
            } else {
                (b, a)
            };
            self.visit(a, p, tunnels, chambers, best);
            self.visit(b, p, tunnels, chambers, best);
        } else {
            for &id in &node.ids {
                let d = if id < tunnels.len() {
                    let t = &tunnels[id];
                    capsule(p, t.start, t.end, t.radius)
                } else {
                    let c = &chambers[id - tunnels.len()];
                    let delta = sub(p, c.center);
                    let k0 = length(std::array::from_fn(|i| delta[i] / c.radii[i]));
                    let k1 = length(std::array::from_fn(|i| delta[i] / c.radii[i].powi(2)));
                    if k1 > 1e-12 {
                        k0 * (k0 - 1.) / k1
                    } else {
                        -c.radii.iter().copied().fold(f64::INFINITY, f64::min)
                    }
                };
                *best = best.min(d);
            }
        }
    }
}
