//! World-anchored surface material sampled from the detailed ground mesh.
//! Coarse exterior geometry must retain the dirt visible in the detailed mesh.
use super::{Point, surface_net, underground};
use crate::physics::matter::world_field;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

type Face = [(Point, [f32; 3]); 3];
#[derive(Default)]
struct Cache {
    source: serde_json::Value,
    tiles: HashMap<[i64; 2], Arc<Vec<Face>>>,
    generation: u64,
    order: VecDeque<[i64; 2]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distant_cover_retains_detailed_surface_dirt_and_grass() {
        let data = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let field = underground::Field::parse(&data).unwrap().unwrap();
        let sampler = Sampler::new(&field).unwrap();
        let mesh = field.mesh(underground::world([0., 2., 0.]), &[]).unwrap();
        let mut dirt = 0;
        let mut grass = 0;
        for vertex in mesh.iter().flatten() {
            let p = [vertex[0], vertex[1], vertex[2]];
            if p[0].abs() > 12. || p[2].abs() > 12. {
                continue;
            }
            let h = super::super::height(normal(p));
            let depth = h - underground::local(p)[1];
            if depth.abs() > 0.5 {
                continue;
            }
            let kind = world_field::substance(p);
            if !matches!(
                kind,
                world_field::Substance::Dirt | world_field::Substance::Turf
            ) {
                continue;
            }
            let far = underground::world([p[0], h, p[2]]);
            let actual = sampler
                .color(far)
                .expect("detailed roof must have distant cover");
            let expected = kind.color_at(p);
            let error = actual
                .iter()
                .zip(expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(error < 0.015, "{p:?}: {actual:?} differs from {expected:?}");
            if kind == world_field::Substance::Dirt {
                dirt += 1;
            } else {
                grass += 1;
            }
        }
        assert!(
            dirt > 0 && grass > 0,
            "must exercise both intentional materials: {dirt}/{grass}"
        );
    }
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }
static SHARED: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
fn shared() -> &'static Mutex<Cache> {
    SHARED.get_or_init(|| Mutex::new(Cache::default()))
}
impl Cache {
    fn select(&mut self, source: &serde_json::Value) {
        if self.source != *source {
            self.tiles.clear();
            self.order.clear();
            self.source = source.clone();
            self.generation = self.generation.wrapping_add(1);
        }
    }
    fn insert(&mut self, key: [i64; 2], faces: Arc<Vec<Face>>) -> Arc<Vec<Face>> {
        if let Some(old) = self.tiles.get(&key) {
            return old.clone();
        }
        if self.tiles.len() >= 1024 {
            if let Some(old) = self.order.pop_front() {
                self.tiles.remove(&old);
            }
        }
        self.tiles.insert(key, faces.clone());
        self.order.push_back(key);
        faces
    }
}

fn normal(p: Point) -> Point {
    let r = p.iter().map(|v| v * v).sum::<f64>().sqrt();
    p.map(|v| v / r)
}

/// The same 3 m sampling phase and edge-intersection average as the detailed
/// surface net. Only its upper band is needed, not the underground chambers.
fn tile(field: &underground::Field, key: [i64; 2]) -> Vec<Face> {
    let x0 = key[0] as f64 * 24. - 6.;
    let z0 = key[1] as f64 * 24. - 6.;
    let mut heights = [[0.; 13]; 13];
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for z in 0..13 {
        for x in 0..13 {
            let h = super::height(normal(underground::world([
                x0 + x as f64 * 3.,
                0.,
                z0 + z as f64 * 3.,
            ])));
            heights[z][x] = h;
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    let y0 = ((lo + 78.) / 3.).floor() * 3. - 78. - 3.;
    let ny = ((hi - y0) / 3.).ceil() as usize + 1;
    if ny > 32 {
        return Vec::new();
    }
    let Ok(mesh) = surface_net::extract([x0, y0, z0], [12, ny, 12], 3., |p| {
        let x = ((p[0] - x0) / 3.).round() as usize;
        let z = ((p[2] - z0) / 3.).round() as usize;
        field.distance(underground::world(p), heights[z][x])
    }) else {
        return Vec::new();
    };
    // Shared vertices occur in up to six faces. Classify and light their
    // material once per tile, rather than repeating geological queries per face.
    let vertices: Vec<_> = mesh
        .vertices
        .iter()
        .map(|p| {
            let p = underground::world(*p);
            let kind = world_field::substance(p);
            let color = if kind == world_field::Substance::Turf {
                let up = normal(p);
                let n = super::surface_normal(up);
                if n.iter().zip(up).map(|(a, b)| a * b).sum::<f64>().abs() < 0.65 {
                    world_field::ROOT_COLOR
                } else {
                    kind.color_at(p)
                }
            } else {
                kind.color_at(p)
            };
            (p, color)
        })
        .collect();
    mesh.triangles
        .iter()
        .filter_map(|face| {
            let face = face.map(|i| vertices[i as usize]);
            let center = std::array::from_fn(|k| (face[0].0[k] + face[1].0[k] + face[2].0[k]) / 3.);
            // Underground walls and actual openings are not a surface paint layer.
            let h = super::height(normal(center));
            ((underground::local(center)[1] - h).abs() <= 0.75).then_some(face)
        })
        .collect()
}

pub(crate) struct Sampler<'a> {
    field: &'a underground::Field,
    source: serde_json::Value,
    generation: Cell<u64>,
    shared: bool,
}
impl<'a> Sampler<'a> {
    pub(crate) fn new(field: &'a underground::Field) -> Option<Self> {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let shared = *ENABLED
            .get_or_init(|| std::env::var("WORMWOOD_SHARED_SURFACE_COVER").as_deref() == Ok("1"));
        Self::with_mode(field, shared)
    }
    fn with_mode(field: &'a underground::Field, use_shared: bool) -> Option<Self> {
        let source = field.snapshot();
        if source["meshing"] != "surface_net" {
            return None;
        }
        let generation = if use_shared {
            let mut cache = shared().lock().unwrap();
            cache.select(&source);
            cache.generation
        } else {
            CACHE.with(|c| {
                let mut c = c.borrow_mut();
                c.select(&source);
                c.generation
            })
        };
        Some(Self {
            field,
            source,
            generation: Cell::new(generation),
            shared: use_shared,
        })
    }
    pub(crate) fn color(&self, point: Point) -> Option<[f32; 3]> {
        // Southern original-surface color is defined by its authored biome.
        // Do not reconstruct the old town cave net for every offshore vertex.
        // Geometry and excavation remain owned by their existing fields.
        if !super::nightworld::active(point) && super::regions::southern(normal(point)).is_some() {
            return Some(world_field::substance(point).color_at(point));
        }
        let field = self.field;
        if point[1] <= 0. || !field.affected(point, 0.) {
            return None;
        }
        let h = super::height(normal(point));
        // Match the vertical range of the detailed outdoor net.
        if !(-74. ..15.).contains(&h) {
            return None;
        }
        let key = [
            (point[0] / 24.).floor() as i64,
            (point[2] / 24.).floor() as i64,
        ];
        let faces = if self.shared {
            let existing = {
                let mut cache = shared().lock().unwrap();
                if cache.generation != self.generation.get() {
                    cache.select(&self.source);
                    self.generation.set(cache.generation);
                }
                cache.tiles.get(&key).cloned()
            };
            if let Some(faces) = existing {
                faces
            } else {
                // Expensive extraction is outside the lock. A result is cached
                // only if its source is still current; old jobs cannot publish
                // tiles into a newer world's cache.
                let faces = Arc::new(tile(field, key));
                let mut cache = shared().lock().unwrap();
                if cache.generation == self.generation.get() {
                    cache.insert(key, faces)
                } else {
                    faces
                }
            }
        } else {
            CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                if let Some(faces) = cache.tiles.get(&key) {
                    faces.clone()
                } else {
                    cache.insert(key, Arc::new(tile(field, key)))
                }
            })
        };
        sample_faces(&faces, point)
    }
}
fn sample_faces(faces: &[Face], point: Point) -> Option<[f32; 3]> {
    for face in faces {
        let [(a, ca), (b, cb), (c, cc)] = *face;
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-10 {
            continue;
        }
        let u = ((b[2] - c[2]) * (point[0] - c[0]) + (c[0] - b[0]) * (point[2] - c[2])) / d;
        let v = ((c[2] - a[2]) * (point[0] - c[0]) + (a[0] - c[0]) * (point[2] - c[2])) / d;
        let w = 1. - u - v;
        if u >= -1e-7 && v >= -1e-7 && w >= -1e-7 {
            return Some(std::array::from_fn(|k| {
                (ca[k] as f64 * u + cb[k] as f64 * v + cc[k] as f64 * w) as f32
            }));
        }
    }
    None
}

#[cfg(test)]
mod shared_cover_tests {
    use super::*;
    #[test]
    fn shared_cover_matches_private_cache_across_threads_and_profile_changes() {
        let profile: serde_json::Value =
            serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let a = underground::Field::parse(&profile).unwrap().unwrap();
        let mut other = profile;
        other["organic_noise"]["field"] = serde_json::json!("value_noise");
        let b = underground::Field::parse(&other).unwrap().unwrap();
        let points: Vec<_> = (-2..=2)
            .flat_map(|x| {
                (-2..=2).map(move |z| {
                    let p = underground::world([x as f64 * 3., 0., z as f64 * 3.]);
                    underground::world([p[0], super::super::height(normal(p)), p[2]])
                })
            })
            .collect();
        let aa = Sampler::with_mode(&a, false).unwrap();
        let ea: Vec<_> = points.iter().map(|&p| aa.color(p)).collect();
        let bb = Sampler::with_mode(&b, false).unwrap();
        let eb: Vec<_> = points.iter().map(|&p| bb.color(p)).collect();
        std::thread::scope(|scope| {
            let fa = &a;
            let fb = &b;
            let ps = &points;
            let ea = &ea;
            let eb = &eb;
            let one = scope.spawn(move || {
                for _ in 0..3 {
                    let s = Sampler::with_mode(fa, true).unwrap();
                    for (&p, &e) in ps.iter().zip(ea) {
                        assert_eq!(s.color(p), e);
                    }
                }
            });
            let two = scope.spawn(move || {
                for _ in 0..3 {
                    let s = Sampler::with_mode(fb, true).unwrap();
                    for (&p, &e) in ps.iter().zip(eb) {
                        assert_eq!(s.color(p), e);
                    }
                }
            });
            one.join().unwrap();
            two.join().unwrap();
        });
        assert!(shared().lock().unwrap().tiles.len() <= 1024);
    }
}
