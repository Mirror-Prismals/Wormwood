//! Cardinal tie/segment Hermite geometry, represented as continuous surfaces.
//! Rendering and collision consume the same indexed trough mesh.
use super::math::*;
#[derive(Clone, Debug)]
pub(crate) struct Tie {
    pub id: u64,
    pub anchor: V,
    pub position: V,
    pub normal: V,
    pub forward: V,
    pub radius: f64,
}
#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub id: u64,
    pub start: u64,
    pub end: u64,
    pub material: u32,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pose {
    pub position: V,
    pub right: V,
    pub up: V,
    pub forward: V,
    pub radius: f64,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vertex {
    pub position: V,
    pub normal: V,
    pub uv: V2,
    pub material: u32,
}
#[derive(Default, Clone, Debug)]
pub(crate) struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
}
impl Mesh {
    fn quad(&mut self, p: [V; 4], normal: V, uv: [[f64; 2]; 4], material: u32) {
        let first = self.vertices.len() as u32;
        self.vertices.extend((0..4).map(|i| Vertex {
            position: p[i],
            normal,
            uv: uv[i],
            material,
        }));
        self.triangles
            .extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub min_radius: f64,
    pub max_radius: f64,
    pub radius_step: f64,
    pub lift: f64,
    pub sides: usize,
    pub steps_per_meter: f64,
    pub wall_height: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            min_radius: 0.35,
            max_radius: 1.65,
            radius_step: 0.15,
            lift: 0.04,
            sides: 8,
            steps_per_meter: 4.,
            wall_height: 0.,
        }
    }
}
pub(crate) struct State {
    pub settings: Settings,
    pub ties: Vec<Tie>,
    pub segments: Vec<Segment>,
    pub selected: Option<u64>,
    pub radius: f64,
    pub material: u32,
    pub revision: u64,
    next_tie: u64,
    next_segment: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            ties: Vec::new(),
            segments: Vec::new(),
            selected: None,
            radius: 0.65,
            material: 528,
            revision: 0,
            next_tie: 1,
            next_segment: 1,
        }
    }
}
fn hermite(a: &Tie, b: &Tie, t: f64) -> (V, V) {
    let distance = len(sub(a.position, b.position)).max(1.);
    let m0 = mul(a.forward, distance);
    let m1 = mul(b.forward, distance);
    let t2 = t * t;
    let t3 = t2 * t;
    let p = add(
        add(
            mul(a.position, 2. * t3 - 3. * t2 + 1.),
            mul(m0, t3 - 2. * t2 + t),
        ),
        add(mul(b.position, -2. * t3 + 3. * t2), mul(m1, t3 - t2)),
    );
    let tangent = add(
        add(
            mul(a.position, 6. * t2 - 6. * t),
            mul(m0, 3. * t2 - 4. * t + 1.),
        ),
        add(
            mul(b.position, -6. * t2 + 6. * t),
            mul(m1, 3. * t2 - 2. * t),
        ),
    );
    (p, tangent)
}
pub(crate) fn evaluate(a: &Tie, b: &Tie, t: f64) -> Pose {
    let t = t.clamp(0., 1.);
    let (position, tangent) = hermite(a, b, t);
    let forward = safe_unit(tangent, safe_unit(sub(b.position, a.position), a.forward));
    let suggested = safe_unit(mix(a.normal, b.normal, t), a.normal);
    let projected = sub(suggested, mul(forward, dot(suggested, forward)));
    let fallback = if forward[1].abs() < 0.8 {
        [0., 1., 0.]
    } else {
        [1., 0., 0.]
    };
    let up = safe_unit(
        projected,
        unit(sub(fallback, mul(forward, dot(fallback, forward)))),
    );
    let right = unit(cross(up, forward));
    let up = unit(cross(forward, right));
    Pose {
        position,
        right,
        up,
        forward,
        radius: a.radius * (1. - t) + b.radius * t,
    }
}
impl State {
    pub fn configure(&mut self, data: &serde_json::Value) -> Result<(), String> {
        let ties = data["ties"]
            .as_array()
            .filter(|a| a.len() <= 1024)
            .ok_or("bounded slide ties required")?;
        let segments = data["segments"]
            .as_array()
            .filter(|a| a.len() <= 1024)
            .ok_or("bounded slide segments required")?;
        let mut next = Self::default();
        next.settings = self.settings.clone();
        for row in ties {
            let id = row["id"]
                .as_u64()
                .filter(|n| *n > 0 && *n < u64::MAX)
                .ok_or("slide tie id required")?;
            if next.ties.iter().any(|t| t.id == id) {
                return Err("duplicate slide tie".into());
            }
            let anchor = super::types::vector(&row["anchor"])?;
            let normal = super::types::vector(&row["normal"])?;
            let forward = super::types::vector(&row["forward"])?;
            if len(normal) < 1e-9 || len(forward) < 1e-9 {
                return Err("zero slide frame direction".into());
            }
            let radius = super::types::number(
                row,
                "radius",
                0.65,
                next.settings.min_radius,
                next.settings.max_radius,
            )?;
            next.ties.push(Tie {
                id,
                anchor,
                normal: unit(normal),
                forward: unit(forward),
                position: add(anchor, mul(unit(normal), radius + next.settings.lift)),
                radius,
            });
            next.next_tie = next.next_tie.max(id + 1);
        }
        for row in segments {
            let start = row["start"].as_u64().ok_or("slide start required")?;
            let end = row["end"].as_u64().ok_or("slide end required")?;
            if next
                .segments
                .iter()
                .any(|s| s.start == start || s.end == end)
            {
                return Err("slide branching requires explicit separate course".into());
            }
            let id = next.connect(start, end)?;
            let material = row["material"].as_u64().unwrap_or(528);
            if material != 528 && material != 529 {
                return Err("unsupported slide material".into());
            }
            next.segments
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .material = material as u32;
        }
        *self = next;
        Ok(())
    }
    pub fn course(&self) -> Result<Vec<Pose>, String> {
        self.course_from(None)
    }
    pub fn course_from(&self, start: Option<u64>) -> Result<Vec<Pose>, String> {
        let Some(first) = (if let Some(start) = start {
            Some(
                self.segments
                    .iter()
                    .find(|s| s.start == start)
                    .ok_or("configured source tie has no outgoing course")?,
            )
        } else {
            self.segments
                .iter()
                .find(|s| !self.segments.iter().any(|other| other.end == s.start))
                .or(self.segments.first())
        }) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut next = Some(first);
        let mut seen = std::collections::HashSet::new();
        while let Some(segment) = next {
            if !seen.insert(segment.id) {
                return Err("floodgate course cannot be a closed cycle".into());
            }
            let mut poses = self.segment_poses(segment)?;
            if !out.is_empty() {
                poses.remove(0);
            }
            out.extend(poses);
            next = self.segments.iter().find(|s| s.start == segment.end);
        }
        Ok(out)
    }
    pub fn place(
        &mut self,
        anchor: V,
        normal: V,
        heading: V,
        new_chain: bool,
    ) -> Result<u64, String> {
        if !finite(anchor)
            || !finite(normal)
            || !finite(heading)
            || len(normal) < 1e-8
            || len(heading) < 1e-8
        {
            return Err("invalid continuous slide attachment".into());
        }
        let normal = unit(normal);
        let radius = self
            .radius
            .clamp(self.settings.min_radius, self.settings.max_radius);
        let position = add(anchor, mul(normal, radius + self.settings.lift));
        let existing = self
            .ties
            .iter()
            .find(|t| len(sub(t.anchor, anchor)) < 0.1)
            .map(|t| t.id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_tie;
            self.next_tie += 1;
            id
        });
        let tie = Tie {
            id,
            anchor,
            position,
            normal,
            forward: unit(heading),
            radius,
        };
        if let Some(previous) = self.ties.iter_mut().find(|t| t.id == id) {
            *previous = tie;
        } else {
            self.ties.push(tie);
        }
        if !new_chain {
            if let Some(previous) = self.selected {
                if previous != id {
                    self.connect(previous, id)?;
                }
            }
        }
        self.selected = Some(id);
        self.revision += 1;
        Ok(id)
    }
    pub fn connect(&mut self, start: u64, end: u64) -> Result<u64, String> {
        if start == end
            || !self.ties.iter().any(|t| t.id == start)
            || !self.ties.iter().any(|t| t.id == end)
        {
            return Err("invalid waterslide connection".into());
        }
        self.segments.retain(|s| s.start != start && s.end != end);
        let id = self.next_segment;
        self.next_segment += 1;
        self.segments.push(Segment {
            id,
            start,
            end,
            material: self.material,
        });
        self.revision += 1;
        Ok(id)
    }
    pub fn adjust_radius(&mut self, steps: f64) {
        self.radius = (self.radius + steps * self.settings.radius_step)
            .clamp(self.settings.min_radius, self.settings.max_radius);
        if let Some(tie) = self.ties.iter_mut().find(|t| Some(t.id) == self.selected) {
            tie.radius = self.radius;
            tie.position = add(
                tie.anchor,
                mul(tie.normal, self.radius + self.settings.lift),
            );
        }
        self.revision += 1;
    }
    pub fn clear(&mut self, all: bool) {
        self.selected = None;
        if all {
            self.ties.clear();
            self.segments.clear();
            self.next_tie = 1;
            self.next_segment = 1;
        }
        self.revision += 1;
    }
    pub fn segment_poses(&self, segment: &Segment) -> Result<Vec<Pose>, String> {
        let a = self
            .ties
            .iter()
            .find(|t| t.id == segment.start)
            .ok_or("missing slide start")?;
        let b = self
            .ties
            .iter()
            .find(|t| t.id == segment.end)
            .ok_or("missing slide end")?;
        let count = (len(sub(a.position, b.position)) * self.settings.steps_per_meter.max(1.))
            .ceil()
            .clamp(8., 192.) as usize;
        Ok((0..=count)
            .map(|i| evaluate(a, b, i as f64 / count as f64))
            .collect())
    }
    pub fn mesh(&self) -> Result<Mesh, String> {
        let mut out = Mesh::default();
        let sides = self.settings.sides.clamp(8, 48);
        for segment in &self.segments {
            let poses = self.segment_poses(segment)?;
            let mut distance = 0.;
            for pair in poses.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let next = distance + len(sub(a.position, b.position));
                for side in sides / 2..sides {
                    let t0 = side as f64 / sides as f64;
                    let t1 = (side + 1) as f64 / sides as f64;
                    let angle0 = t0 * std::f64::consts::TAU;
                    let angle1 = t1 * std::f64::consts::TAU;
                    let radial =
                        |p: Pose, t: f64| unit(add(mul(p.right, t.cos()), mul(p.up, t.sin())));
                    let (ra0, ra1, rb0, rb1) = (
                        radial(a, angle0),
                        radial(a, angle1),
                        radial(b, angle0),
                        radial(b, angle1),
                    );
                    out.quad(
                        [
                            add(a.position, mul(ra0, a.radius)),
                            add(b.position, mul(rb0, b.radius)),
                            add(b.position, mul(rb1, b.radius)),
                            add(a.position, mul(ra1, a.radius)),
                        ],
                        unit(add(add(ra0, ra1), add(rb0, rb1))),
                        [[t0, distance], [t0, next], [t1, next], [t1, distance]],
                        segment.material,
                    );
                }
                let h = self.settings.wall_height.max(0.);
                if h > 0. {
                    for sign in [-1., 1.] {
                        let pa = add(a.position, mul(a.right, sign * a.radius));
                        let pb = add(b.position, mul(b.right, sign * b.radius));
                        out.quad(
                            [pa, pb, add(pb, mul(b.up, h)), add(pa, mul(a.up, h))],
                            mul(unit(add(a.right, b.right)), -sign),
                            [[0., distance], [0., next], [1., next], [1., distance]],
                            segment.material,
                        );
                    }
                }
                distance = next;
            }
        }
        Ok(out)
    }
}
