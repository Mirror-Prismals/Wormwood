//! Local resolution changes preserve mass and momentum. Coarsening is a
//! numerical operation, never restoration of failed material connections.
use super::*;
impl World {
    pub fn adapt(&mut self) {
        // A sand grain is a persistent physical grain. Subdividing one into
        // overlapping spheres changes its packing and injects motion. Solid
        // rock/wood now have their own conserved material cells as well.
        if self.particles.first().is_none_or(|p| p.kind != Kind::Dirt) {
            return;
        }
        let mut candidates: Vec<_> = self
            .particles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.active && !p.anchored && p.level < 2)
            .map(|(i, p)| (i, self.tool.distance(p.p)))
            .filter(|(i, d)| {
                let p = &self.particles[*i];
                // Refine the tool's local process zone. Gravity, rigid travel
                // and bulk settling alone must not subdivide the whole bed.
                // Detached matter keeps its existing resolution until settled;
                // the existing damage/momentum checks still govern coarsening.
                *d < p.radius * 2.25 && *d > p.radius * 1.45
            })
            .collect();
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (id, _) in candidates.into_iter().take(3) {
            if self.active_count() + 7 > MAX_ACTIVE || self.particles.len() + 8 > 4096 {
                break;
            }
            self.split(id);
        }
        for family in 0..self.families.len() {
            if self.can_merge(family) {
                self.merge(family);
                break;
            }
        }
    }
    fn split(&mut self, id: usize) {
        let parent = self.particles[id].clone();
        if !parent.active {
            return;
        }
        let children = std::array::from_fn(|i| self.particles.len() + i);
        let mut model = vec![parent.rest];
        let mut moved = vec![parent.p];
        for b in self
            .bonds
            .iter()
            .filter(|b| b.active && (b.a == id || b.b == id))
            .take(16)
        {
            let other = if b.a == id { b.b } else { b.a };
            model.push(self.particles[other].rest);
            moved.push(self.particles[other].p);
        }
        let axes = super::super::rigid_fit::fit(&model, &moved)
            .map(|(_, axes)| axes.map(|v| v.map(|x| x as f32)))
            .unwrap_or([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        // Opposite offsets cancel exactly; each sample carries 1/8 the mass
        // and volume. Spin supplies the affine velocity contribution.
        for i in 0..8 {
            let off = [
                if i & 1 == 0 { -1. } else { 1. },
                if i & 2 == 0 { -1. } else { 1. },
                if i & 4 == 0 { -1. } else { 1. },
            ]
            .map(|x| x * parent.radius * 0.5);
            let mut child = parent.clone();
            let world_off = (0..3).fold([0.; 3], |v, k| add(v, scale(axes[k], off[k])));
            child.p = add(parent.p, world_off);
            child.rest = add(parent.rest, off);
            child.v = add(parent.v, cross(parent.spin, world_off));
            child.mass = parent.mass / 8.;
            child.radius = parent.radius * 0.5;
            child.level += 1;
            child.quiet = 0.;
            child.activity = 0.;
            self.particles.push(child);
        }
        let incident: Vec<_> = self
            .bonds
            .iter()
            .filter(|b| b.active && (b.a == id || b.b == id))
            .cloned()
            .collect();
        for b in &mut self.bonds {
            if b.a == id || b.b == id {
                b.active = false;
            }
        }
        self.particles[id].active = false;
        if parent.kind.law().cohesion > 0. {
            for a in 0..8 {
                for b in a + 1..8 {
                    if length(sub(
                        self.particles[children[a]].p,
                        self.particles[children[b]].p,
                    )) < parent.radius * 1.5
                    {
                        self.bond(
                            children[a],
                            children[b],
                            parent.radius.powi(2) * 0.20,
                            1.,
                            0.,
                        );
                    }
                }
            }
            for old in incident {
                let other = if old.a == id { old.b } else { old.a };
                let ratio = old.rest / length(sub(parent.p, self.particles[other].p)).max(1e-6);
                let mut near = children.to_vec();
                near.sort_by(|a, b| {
                    length(sub(self.particles[*a].p, self.particles[other].p))
                        .total_cmp(&length(sub(self.particles[*b].p, self.particles[other].p)))
                });
                // Partition interface area and inherit strain/damage. Refining
                // does not strengthen a connection or reset its plastic state.
                for child in near.into_iter().take(4) {
                    self.bond(child, other, old.area / 4., ratio, old.damage);
                }
            }
        }
        self.garden.remap_split(id, children, &self.particles);
        self.families.push(Family {
            parent: id,
            children,
        });
        self.splits += 1;
        self.topology_dirty = true;
    }
    fn can_merge(&self, f: usize) -> bool {
        let family = &self.families[f];
        let parent = &self.particles[family.parent];
        if parent.active {
            return false;
        }
        if family
            .children
            .iter()
            .any(|i| !self.particles[*i].active || self.particles[*i].quiet < 1.8)
        {
            return false;
        }
        let mass: f32 = family
            .children
            .iter()
            .map(|i| self.particles[*i].mass)
            .sum();
        let c = scale(
            family.children.iter().fold([0.; 3], |v, i| {
                add(v, scale(self.particles[*i].p, self.particles[*i].mass))
            }),
            1. / mass,
        );
        if self.tool.distance(c) < parent.radius * 6. {
            return false;
        }
        if family
            .children
            .iter()
            .any(|i| length(sub(self.particles[*i].p, c)) > parent.radius * 0.94)
        {
            return false;
        }
        if self.bonds.iter().any(|b| {
            (family.children.contains(&b.a) || family.children.contains(&b.b)) && b.damage > 0.02
        }) {
            return false;
        }
        // Only simplify the interior; preserve fine free surfaces and separated
        // grains. This also avoids filling a freshly opened cavity on merge.
        self.particles
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                p.active
                    && !family.children.contains(i)
                    && length(sub(p.p, c)) < parent.radius * 2.8
            })
            .count()
            >= 10
    }
    fn merge(&mut self, f: usize) {
        let Family { parent, children } = self.families[f].clone();
        let mass: f32 = children.iter().map(|i| self.particles[*i].mass).sum();
        let center = scale(
            children.iter().fold([0.; 3], |v, i| {
                add(v, scale(self.particles[*i].p, self.particles[*i].mass))
            }),
            1. / mass,
        );
        let velocity = scale(
            children.iter().fold([0.; 3], |v, i| {
                add(v, scale(self.particles[*i].v, self.particles[*i].mass))
            }),
            1. / mass,
        );
        let angular = children.iter().fold([0.; 3], |v, i| {
            let p = &self.particles[*i];
            add(
                v,
                add(
                    scale(cross(sub(p.p, center), sub(p.v, velocity)), p.mass),
                    scale(p.spin, 0.4 * p.mass * p.radius * p.radius),
                ),
            )
        });
        let mut connections = BTreeMap::<usize, (f32, f32, f32)>::new();
        for b in &mut self.bonds {
            if !b.active {
                continue;
            }
            let a = children.contains(&b.a);
            let c = children.contains(&b.b);
            if !a && !c {
                continue;
            }
            b.active = false;
            if a && c {
                continue;
            }
            let other = if a { b.b } else { b.a };
            let ratio =
                b.rest / length(sub(self.particles[b.a].p, self.particles[b.b].p)).max(1e-6);
            let e = connections.entry(other).or_default();
            e.0 += b.area;
            e.1 += ratio * b.area;
            e.2 = e.2.max(b.damage);
        }
        for i in children {
            self.particles[i].active = false;
        }
        let p = &mut self.particles[parent];
        p.active = true;
        p.p = center;
        p.v = velocity;
        p.mass = mass;
        p.spin = scale(angular, 1. / (0.4 * mass * p.radius * p.radius));
        p.quiet = 0.;
        for (other, (area, ratio, damage)) in connections {
            self.bond(parent, other, area, ratio / area, damage);
        }
        self.garden.remap_merge(parent, children, &self.particles);
        self.merges += 1;
        self.topology_dirty = true;
    }
}
