//! Cached direct-light visibility for the workstation's authored studio floor.
//!
//! The input is the first 256 optical-frame rows described by
//! `Procedures/Shaders/workstation.shared.glsl`. This computes the same approximate
//! single-path glass shadow as `gemShadow`, using the actual convex halfspaces,
//! Snell refraction at entry, interior Beer absorption, and entry/exit Fresnel.
//! It is not a caustic solver: the refracted path is not reconnected to the emitter.
//!
//! Each light receives a separate RGB visibility field, normalized by its
//! unoccluded irradiance. The shader retains its view-dependent floor BRDF and
//! authored roughness; averaging visibility does approximate the correlation of
//! that BRDF with different points on an area emitter. Values are scene-linear
//! transmission, never radiance or display-transformed color.
//!
//! Rebuild only when panel geometry/material, the floor, or lights change. This
//! allocating, bounded CPU computation belongs to presentation, not audio.

pub const EDGE: usize = 64;
pub const ROWS: usize = 3 * EDGE * EDGE;

const INPUT_ROWS: usize = 256;
const MARGIN_M: f32 = 0.75;
const EPSILON: f32 = 0.000_001;
const PARALLEL_EPSILON: f32 = 0.000_000_1;
const UNOBSTRUCTED: [f32; 4] = [1., 1., 1., 1.];
type V3 = [f32; 3];

// All sixteen samples at one floor node share the exact same ray origin.
// Prepare a panel only after its original AABB test admits a connection.
type EntryNumerators = [Option<[f32; 26]>; 4];

#[derive(PartialEq, Eq)]
enum CacheKey {
    Unobstructed,
    Scene([[u32; 4]; INPUT_ROWS]),
}

/// One bounded field per presentation session. Compare exact input bits, not a
/// lossy hash, while ignoring framebuffer, animation and control-only rows.
pub(crate) struct Cache {
    key: Option<CacheKey>,
    field: Vec<[f32; 4]>,
    lights: [LightCache; 3],
    sample_reuse: bool,
    #[cfg(test)]
    rebuilds: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self::with_sample_reuse(std::env::var("WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE").as_deref() == Ok("1"))
    }
}

impl Cache {
    pub(crate) fn with_sample_reuse(enabled: bool) -> Self {
        Self { key: None, field: Vec::new(), lights: Default::default(), sample_reuse: enabled,
            #[cfg(test)] rebuilds: 0 }
    }

    pub(crate) fn field(&mut self, rows: &[[f32; 4]]) -> &[[f32; 4]] {
        let key = cache_key(rows);
        if self.key.as_ref() != Some(&key) {
            if self.sample_reuse { self.rebuild(rows, &key); }
            else { self.field = build(rows); }
            self.key = Some(key);
            #[cfg(test)]
            { self.rebuilds += 1; }
        }
        &self.field
    }

    fn rebuild(&mut self, rows: &[[f32; 4]], key: &CacheKey) {
        let CacheKey::Scene(scene) = key else {
            self.field.resize(ROWS, UNOBSTRUCTED);
            self.field.fill(UNOBSTRUCTED);
            return;
        };
        // Recomputing all factors is more expensive than the original pass.
        // Use that pass during startup and changes to shared ray geometry;
        // initialize factors only after the connections stop changing.
        let stable_connections = match &self.key {
            Some(CacheKey::Scene(previous)) => (0..3).all(|i| connection_key(previous, i) == connection_key(scene, i)),
            _ => false,
        };
        if !stable_connections {
            for (i, cache) in self.lights.iter_mut().enumerate() {
                if cache.shared.as_ref() != Some(&connection_key(scene, i)) {
                    cache.shared = None;
                    cache.panels = Default::default();
                }
            }
            self.field = build(rows);
            return;
        }
        self.field.resize(ROWS, UNOBSTRUCTED);
        self.field.fill(UNOBSTRUCTED);
        let Some(panels) = panels(rows) else { return; };
        let bounds = domain(rows);
        let floor_z = rows[5][3];
        std::thread::scope(|scope| {
            for ((index, grid), cache) in self.field.chunks_mut(EDGE * EDGE).enumerate().zip(&mut self.lights) {
                if index as f32 >= rows[9][0] { break; }
                let Some(light) = light(rows, index) else { continue; };
                let dirty = cache.prepare(scene, index);
                let panels = &panels;
                scope.spawn(move || build_light_cached(grid, panels, &light, bounds, floor_z, cache, dirty));
            }
        });
    }
}

/// Four independent optical factors per connection, in their original panel
/// order. Their 2-bit modes distinguish a skipped panel, a multiplication, and
/// the original early-zero branch. Storage is bounded at 9.64 MB for 3 lights.
#[derive(Default)]
struct LightCache {
    shared: Option<[[u32; 4]; 6]>,
    panels: [Option<[[u32; 4]; 32]>; 4],
    factors: Vec<[V3; 4]>,
    modes: Vec<u8>,
}

impl LightCache {
    fn prepare(&mut self, scene: &[[u32; 4]; INPUT_ROWS], light: usize) -> u8 {
        let shared = connection_key(scene, light);
        let changed = self.shared.as_ref() != Some(&shared);
        let mut dirty = 0;
        for i in 0..4 {
            let mut panel = [[0; 4]; 32];
            panel.copy_from_slice(&scene[32 + 40 * i..64 + 40 * i]);
            if changed || self.panels[i].as_ref() != Some(&panel) { dirty |= 1 << i; }
            self.panels[i] = Some(panel);
        }
        self.shared = Some(shared);
        self.factors.resize(EDGE * EDGE * 16, [[1.; 3]; 4]);
        self.modes.resize(EDGE * EDGE * 16, 0);
        dirty
    }
}

fn connection_key(scene: &[[u32; 4]; INPUT_ROWS], light: usize) -> [[u32; 4]; 6] {
    let at = 16 + light * 5;
    [scene[2], scene[5], scene[at], scene[at + 1], scene[at + 2], scene[at + 3]]
}

fn cache_key(rows: &[[f32; 4]]) -> CacheKey {
    // build() validates the whole header, including otherwise unused words.
    // A nonfinite control/header value must still select its unit-field fallback.
    if rows.len() < INPUT_ROWS || rows[..INPUT_ROWS].iter().flatten().any(|v| !v.is_finite()) {
        return CacheKey::Unobstructed;
    }
    let Some(panels) = panels(rows) else { return CacheKey::Unobstructed; };
    if panels.iter().all(Option::is_none) { return CacheKey::Unobstructed; }
    let mut key = [[0; 4]; INPUT_ROWS];
    key[2] = domain(rows).map(f32::to_bits);
    key[5][3] = rows[5][3].to_bits();
    for (index, panel) in panels.iter().enumerate() {
        let Some(panel) = panel else { continue; };
        let base = 32 + 40 * index;
        key[base] = [1, panel.ior.to_bits(), 0, panel.transmission.to_bits()];
        for (offset, values) in [(1, panel.sigma), (2, panel.center), (3, panel.extent)] {
            for axis in 0..3 { key[base + offset][axis] = values[axis].to_bits(); }
        }
        for (target, plane) in key[base + 6..base + 32].iter_mut().zip(&panel.planes) {
            *target = plane.map(f32::to_bits);
        }
    }
    for index in 0..3 {
        if index as f32 >= rows[9][0] { break; }
        let base = 16 + 5 * index;
        if rows[base + 1][3] <= 0. || rows[base + 2][3] <= 0. { continue; }
        // The field normalizes visibility by unoccluded irradiance. Emission
        // RGB and the unused position/normal W words do not affect its result.
        key[base][3] = 1;
        for axis in 0..3 {
            key[base][axis] = rows[base][axis].to_bits();
            key[base + 3][axis] = rows[base + 3][axis].to_bits();
        }
        key[base + 1] = rows[base + 1].map(f32::to_bits);
        key[base + 2] = rows[base + 2].map(f32::to_bits);
    }
    CacheKey::Scene(key)
}

// Four-point Gauss-Legendre on each emitter axis: 16 weighted connections.
const GAUSS: [(f32, f32); 4] = [
    (-0.861_136_3, 0.347_854_85),
    (-0.339_981_05, 0.652_145_15),
    (0.339_981_05, 0.652_145_15),
    (0.861_136_3, 0.347_854_85),
];

#[derive(Clone, Copy)]
struct Panel {
    center: V3,
    extent: V3,
    planes: [[f32; 4]; 26],
    sigma: V3,
    ior: f32,
    transmission: f32,
}

#[derive(Clone, Copy)]
struct LightSample {
    position: V3,
    weight: f32,
}

struct Light {
    normal: V3,
    samples: [LightSample; 16],
}

#[derive(Clone, Copy)]
struct Hit {
    distance: f32,
    normal: V3,
}

/// `[min_x, min_y, span_x, span_y]`, in world metres on the horizontal floor.
/// Camera rows 2/3 supply the orthographic half extents. An additional .75 m
/// surrounds each side; outside this domain the shader must use its analytic
/// shadow fallback, not clamp to a border sample. Invalid extents use .65 m.
pub fn domain(rows: &[[f32; 4]]) -> [f32; 4] {
    fn expanded_half(rows: &[[f32; 4]], index: usize) -> f32 {
        let half = rows.get(index).map_or(0.65, |row| row[3]);
        let half = if half.is_finite() && half > 0. && half < f32::MAX * 0.25 {
            half
        } else {
            0.65
        };
        half + MARGIN_M
    }
    let x = expanded_half(rows, 2);
    let y = expanded_half(rows, 3);
    [-x, -y, 2. * x, 2. * y]
}

/// Three consecutive `EDGE * EDGE` fields, one per light, with X varying fastest.
/// Nodes include both domain endpoints: `min + span * index / (EDGE - 1)`.
/// RGB is finite transmission in [0,1]; alpha is 1. Missing/invalid input produces
/// an unobstructed field. The caller validates the optical frame before upload.
pub fn build(rows: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut field = vec![UNOBSTRUCTED; ROWS];
    if rows.len() < INPUT_ROWS || rows[..INPUT_ROWS].iter().flatten().any(|v| !v.is_finite()) {
        return field;
    }
    let panels = match panels(rows) {
        Some(panels) => panels,
        None => return field,
    };
    if panels.iter().all(Option::is_none) {
        return field;
    }
    let bounds = domain(rows);
    let floor_z = rows[5][3];
    // The three fields are independent. Scoped workers borrow their own output
    // slice and the immutable decoded panels, without a lock or intermediate copy.
    std::thread::scope(|scope| {
        for (light_index, grid) in field.chunks_mut(EDGE * EDGE).enumerate() {
            if light_index as f32 >= rows[9][0] {
                break;
            }
            let Some(light) = light(rows, light_index) else {
                continue;
            };
            let panels = &panels;
            scope.spawn(move || build_light(grid, panels, &light, bounds, floor_z));
        }
    });
    field
}

fn build_light(
    grid: &mut [[f32; 4]],
    panels: &[Option<Panel>; 4],
    light: &Light,
    bounds: [f32; 4],
    floor_z: f32,
) {
    let [min_x, min_y, span_x, span_y] = bounds;
    let dx = span_x / (EDGE - 1) as f32;
    let dy = span_y / (EDGE - 1) as f32;
    for y in 0..EDGE {
        for x in 0..EDGE {
            let point = [min_x + dx * x as f32, min_y + dy * y as f32, floor_z];
            let origin = [point[0], point[1], point[2] + EPSILON];
            let mut total_weight = 0.;
            let mut transmitted = [0.; 3];
            let mut entry_numerators: EntryNumerators = [None; 4];
            for sample in &light.samples {
                let to_light = sub(sample.position, point);
                let distance_squared = dot(to_light, to_light);
                if distance_squared <= EPSILON * EPSILON || !distance_squared.is_finite() {
                    continue;
                }
                let distance = distance_squared.sqrt();
                let ray = mul(to_light, distance.recip());
                let nl = ray[2].max(0.);
                let emitter_cosine = (-dot(light.normal, ray)).max(0.);
                let weight = sample.weight * nl * emitter_cosine / distance_squared;
                if weight <= 0. || !weight.is_finite() {
                    continue;
                }
                let visibility = shadow_with_entries(panels, origin, ray, distance, Some(&mut entry_numerators));
                for channel in 0..3 {
                    transmitted[channel] += weight * visibility[channel];
                }
                total_weight += weight;
            }
            if total_weight > 0. && total_weight.is_finite() {
                let at = y * EDGE + x;
                for channel in 0..3 {
                    grid[at][channel] = (transmitted[channel] / total_weight).clamp(0., 1.);
                }
            }
        }
    }
}

fn build_light_cached(
    grid: &mut [[f32; 4]],
    panels: &[Option<Panel>; 4],
    light: &Light,
    bounds: [f32; 4],
    floor_z: f32,
    cache: &mut LightCache,
    dirty: u8,
) {
    let [min_x, min_y, span_x, span_y] = bounds;
    let dx = span_x / (EDGE - 1) as f32;
    let dy = span_y / (EDGE - 1) as f32;
    for y in 0..EDGE {
        for x in 0..EDGE {
            let point = [min_x + dx * x as f32, min_y + dy * y as f32, floor_z];
            let origin = [point[0], point[1], point[2] + EPSILON];
            let mut total_weight = 0.;
            let mut transmitted = [0.; 3];
            let mut entry_numerators: EntryNumerators = [None; 4];
            for (sample_index, sample) in light.samples.iter().enumerate() {
                // Preserve the original normalized ray, validity branches,
                // weights and accumulation order; only panel factors are reused.
                let to_light = sub(sample.position, point);
                let distance_squared = dot(to_light, to_light);
                if distance_squared <= EPSILON * EPSILON || !distance_squared.is_finite() { continue; }
                let distance = distance_squared.sqrt();
                let ray = mul(to_light, distance.recip());
                let nl = ray[2].max(0.);
                let emitter_cosine = (-dot(light.normal, ray)).max(0.);
                let weight = sample.weight * nl * emitter_cosine / distance_squared;
                if weight <= 0. || !weight.is_finite() { continue; }
                let at = (y * EDGE + x) * 16 + sample_index;
                let factors = &mut cache.factors[at];
                let modes = &mut cache.modes[at];
                if dirty != 0 {
                    let inverse_ray = ray.map(|v| if v.abs() < 1e-8 { 0. } else { v.recip() });
                    for (i, panel) in panels.iter().enumerate() {
                        if dirty & (1 << i) == 0 { continue; }
                        let (factor, mode) = panel.as_ref().map_or(([1.; 3], 0), |panel| {
                            panel_factor(panel, origin, ray, inverse_ray, distance, &mut entry_numerators[i])
                        });
                        factors[i] = factor;
                        *modes = (*modes & !(3 << (2 * i))) | (mode << (2 * i));
                    }
                }
                let mut visibility = [1.; 3];
                for (i, factor) in factors.iter().enumerate() {
                    match (*modes >> (2 * i)) & 3 {
                        1 => { for channel in 0..3 { visibility[channel] *= factor[channel]; } }
                        2 => { visibility = [0.; 3]; break; }
                        _ => {}
                    }
                }
                for channel in 0..3 { transmitted[channel] += weight * visibility[channel]; }
                total_weight += weight;
            }
            if total_weight > 0. && total_weight.is_finite() {
                let at = y * EDGE + x;
                for channel in 0..3 { grid[at][channel] = (transmitted[channel] / total_weight).clamp(0., 1.); }
            }
        }
    }
}

// Same one-panel operations as shadow(), before its ordered multiplication.
fn panel_factor(panel: &Panel, origin: V3, ray: V3, inverse_ray: V3, distance: f32,
    entry_numerators: &mut Option<[f32; 26]>) -> (V3, u8)
{
    if !box_hit(panel, origin, ray, inverse_ray, distance) { return ([1.; 3], 0); }
    let numerators = entry_numerators.get_or_insert_with(|| prepare_entry(panel, origin));
    let Some(entry) = intersect_prepared::<true>(panel, origin, ray, false, distance, numerators) else { return ([1.; 3], 0); };
    let ratio = panel.ior.recip();
    let cosine = dot(ray, entry.normal);
    let k = 1. - ratio * ratio * (1. - cosine * cosine);
    if k < 0. { return ([0.; 3], 2); }
    let interior_ray = sub(mul(ray, ratio), mul(entry.normal, ratio * cosine + k.sqrt()));
    let start = add(add(origin, mul(ray, entry.distance)), mul(interior_ray, EPSILON));
    let Some(exit) = intersect(panel, start, interior_ray, true, f32::MAX) else { return ([1.; 3], 0); };
    let entry_f = fresnel(cosine, 1., panel.ior);
    let exit_f = fresnel(dot(interior_ray, exit.normal), panel.ior, 1.);
    let surface = (1. - entry_f) * (1. - exit_f) * panel.transmission;
    (panel.sigma.map(|sigma| (-sigma * exit.distance).exp() * surface), 1)
}

fn panels(rows: &[[f32; 4]]) -> Option<[Option<Panel>; 4]> {
    let mut result = [None; 4];
    for (index, target) in result.iter_mut().enumerate() {
        if index as f32 >= rows[0][1] {
            break;
        }
        let base = 32 + 40 * index;
        if rows[base][0] < 0.5 {
            continue;
        }
        let ior = rows[base][1];
        let extent = xyz(rows[base + 3]);
        if ior < 1. || extent.iter().any(|v| *v <= 0.) {
            return None;
        }
        let mut planes = [[0.; 4]; 26];
        planes.copy_from_slice(&rows[base + 6..base + 32]);
        // Snell and Fresnel require the frame's unit outward plane normals.
        if planes
            .iter()
            .any(|plane| (dot(xyz(*plane), xyz(*plane)) - 1.).abs() > 0.001)
        {
            return None;
        }
        *target = Some(Panel {
            center: xyz(rows[base + 2]),
            extent,
            planes,
            sigma: xyz(rows[base + 1]).map(|v| v.max(0.)),
            ior,
            transmission: rows[base][3].clamp(0., 1.),
        });
    }
    Some(result)
}

fn light(rows: &[[f32; 4]], index: usize) -> Option<Light> {
    let base = 16 + 5 * index;
    let u = rows[base + 1];
    let v = rows[base + 2];
    if u[3] <= 0. || v[3] <= 0. {
        return None;
    }
    let position = xyz(rows[base]);
    let mut samples = [LightSample {
        position: [0.; 3],
        weight: 0.,
    }; 16];
    for (y, &(gy, wy)) in GAUSS.iter().enumerate() {
        for (x, &(gx, wx)) in GAUSS.iter().enumerate() {
            samples[y * 4 + x] = LightSample {
                position: add(
                    add(position, mul(xyz(u), u[3] * gx)),
                    mul(xyz(v), v[3] * gy),
                ),
                weight: wx * wy,
            };
        }
    }
    Some(Light {
        normal: xyz(rows[base + 3]),
        samples,
    })
}

#[cfg(test)]
fn shadow(panels: &[Option<Panel>; 4], origin: V3, ray: V3, distance: f32) -> V3 {
    shadow_with_entries(panels, origin, ray, distance, None)
}

fn prepare_entry(panel: &Panel, origin: V3) -> [f32; 26] {
    panel.planes.map(|plane| plane[3] - dot(xyz(plane), origin))
}

fn shadow_with_entries(panels: &[Option<Panel>; 4], origin: V3, ray: V3, distance: f32,
    mut entries: Option<&mut EntryNumerators>) -> V3
{
    let mut visibility = [1.; 3];
    let inverse_ray = ray.map(|v| if v.abs() < 1e-8 { 0. } else { v.recip() });
    for (index, panel) in panels.iter().enumerate() {
        let Some(panel) = panel else { continue; };
        if !box_hit(panel, origin, ray, inverse_ray, distance) {
            continue;
        }
        let entry = if let Some(entries) = entries.as_deref_mut() {
            let numerators = entries[index].get_or_insert_with(|| prepare_entry(panel, origin));
            intersect_prepared::<true>(panel, origin, ray, false, distance, numerators)
        } else {
            intersect(panel, origin, ray, false, distance)
        };
        let Some(entry) = entry else { continue; };
        let ratio = panel.ior.recip();
        let cosine = dot(ray, entry.normal);
        let k = 1. - ratio * ratio * (1. - cosine * cosine);
        if k < 0. {
            return [0.; 3];
        }
        let interior_ray = sub(
            mul(ray, ratio),
            mul(entry.normal, ratio * cosine + k.sqrt()),
        );
        let start = add(
            add(origin, mul(ray, entry.distance)),
            mul(interior_ray, EPSILON),
        );
        let Some(exit) = intersect(panel, start, interior_ray, true, f32::MAX) else {
            continue;
        };
        let entry_f = fresnel(cosine, 1., panel.ior);
        // Use the refracted INTERIOR direction. The air ray can falsely yield TIR.
        let exit_f = fresnel(dot(interior_ray, exit.normal), panel.ior, 1.);
        let surface = (1. - entry_f) * (1. - exit_f) * panel.transmission;
        for channel in 0..3 {
            visibility[channel] *= (-panel.sigma[channel] * exit.distance).exp() * surface;
        }
    }
    visibility
}

fn box_hit(panel: &Panel, origin: V3, ray: V3, inverse_ray: V3, limit: f32) -> bool {
    let mut near: f32 = 0.;
    let mut far = limit;
    for axis in 0..3 {
        let low = panel.center[axis] - panel.extent[axis];
        let high = panel.center[axis] + panel.extent[axis];
        if ray[axis].abs() < 1e-8 {
            if origin[axis] < low || origin[axis] > high {
                return false;
            }
        } else {
            let a = (low - origin[axis]) * inverse_ray[axis];
            let b = (high - origin[axis]) * inverse_ray[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if far < near {
                return false;
            }
        }
    }
    far >= near
}

fn intersect(panel: &Panel, origin: V3, ray: V3, inside: bool, limit: f32) -> Option<Hit> {
    intersect_prepared::<false>(panel, origin, ray, inside, limit, &[0.; 26])
}

// PREPARED selects only the origin-dependent numerator. The quotient, plane
// order, strict near/far comparisons, normals and rejection rules stay original.
// Exit origins depend on each refracted ray and always use PREPARED=false.
fn intersect_prepared<const PREPARED: bool>(panel: &Panel, origin: V3, ray: V3,
    inside: bool, limit: f32, numerators: &[f32; 26]) -> Option<Hit>
{
    let mut near = -1e20;
    let mut far = 1e20;
    let mut entry_normal = [0.; 3];
    let mut exit_normal = [0.; 3];
    for (index, plane) in panel.planes.iter().enumerate() {
        let normal = xyz(*plane);
        let denominator = dot(normal, ray);
        let distance = if PREPARED { numerators[index] } else { plane[3] - dot(normal, origin) };
        if denominator.abs() < PARALLEL_EPSILON {
            if distance < -PARALLEL_EPSILON {
                return None;
            }
        } else {
            let t = distance / denominator;
            if denominator < 0. && t > near {
                near = t;
                entry_normal = normal;
            }
            if denominator > 0. && t < far {
                far = t;
                exit_normal = normal;
            }
            if far < near {
                return None;
            }
        }
    }
    if far <= PARALLEL_EPSILON {
        return None;
    }
    if inside {
        Some(Hit {
            distance: far,
            normal: exit_normal,
        })
    } else if near >= PARALLEL_EPSILON && near < limit {
        Some(Hit {
            distance: near,
            normal: entry_normal,
        })
    } else {
        None
    }
}

fn fresnel(cosine: f32, eta_i: f32, eta_t: f32) -> f32 {
    let ci = cosine.abs().clamp(0., 1.);
    let ratio = eta_i / eta_t;
    let sin_t_squared = if ci == 1. {
        0.
    } else {
        ratio * ratio * (1. - ci * ci)
    };
    if sin_t_squared >= 1. {
        return 1.;
    }
    let ct = (1. - sin_t_squared).max(0.).sqrt();
    let rs = (eta_i * ci - eta_t * ct) / (eta_i * ci + eta_t * ct).max(PARALLEL_EPSILON);
    let rp = (eta_t * ci - eta_i * ct) / (eta_t * ci + eta_i * ct).max(PARALLEL_EPSILON);
    0.5 * (rs * rs + rp * rp)
}

fn xyz(v: [f32; 4]) -> V3 {
    [v[0], v[1], v[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, factor: f32) -> V3 {
    a.map(|v| v * factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slab(ior: f32, sigma: V3) -> Panel {
        let six = [
            [1., 0., 0., 4.],
            [-1., 0., 0., 4.],
            [0., 1., 0., 4.],
            [0., -1., 0., 4.],
            [0., 0., 1., 0.5],
            [0., 0., -1., 0.5],
        ];
        Panel {
            center: [0.; 3],
            extent: [4., 4., 0.5],
            planes: std::array::from_fn(|i| six[i % six.len()]),
            sigma,
            ior,
            transmission: 1.,
        }
    }

    #[test]
    fn workstation_floor_entry_numerators_are_lazy_and_exact_across_sample_directions() {
        let panels = [Some(slab(1.766, [0.2, 0.8, 2.])), None,
            Some(slab(1.45, [0.1, 0.3, 0.7])), None];
        let mut entries: EntryNumerators = [None; 4];
        let missed = shadow_with_entries(&panels, [5.,0.,-2.], [0.,0.,1.], 4., Some(&mut entries));
        assert_eq!(missed, [1.; 3]);
        assert!(entries.iter().all(Option::is_none), "AABB misses must not prepare planes");
        let origin = [0.,0.,-2.];
        for ray in [[0.,0.,1.], [0.6,0.,0.8], [-0.6,0.,0.8], [0.,0.6,0.8], [1.,0.,0.]] {
            let reference = shadow(&panels, origin, ray, 5.);
            let cached = shadow_with_entries(&panels, origin, ray, 5., Some(&mut entries));
            assert_eq!(reference.map(f32::to_bits), cached.map(f32::to_bits));
        }
        assert!(entries[1].is_none() && entries[3].is_none());
        for i in [0, 2] {
            let values = entries[i].unwrap();
            assert_eq!(values.map(f32::to_bits), prepare_entry(panels[i].as_ref().unwrap(), origin).map(f32::to_bits));
        }
    }

    #[test]
    fn workstation_floor_prepared_entry_matches_original_clipping_and_ties() {
        let panel = slab(1.766, [1.,2.,3.]);
        // Repeated slab planes deliberately exercise strict-comparison ties.
        for origin in [[0.,0.,-2.], [4.,0.,-2.], [5.,0.,-2.], [0.,0.,0.]] {
            let numerators = prepare_entry(&panel, origin);
            for ray in [[0.,0.,1.], [0.6,0.,0.8], [0.,0.,-1.], [1.,0.,0.], [0.,1e-8,1.]] {
                for limit in [0.5, 4., f32::MAX] {
                    let before = intersect(&panel,origin,ray,false,limit);
                    let after = intersect_prepared::<true>(&panel,origin,ray,false,limit,&numerators);
                    match (before,after) {
                        (None,None) => {},
                        (Some(a),Some(b)) => {
                            assert_eq!(a.distance.to_bits(),b.distance.to_bits());
                            assert_eq!(a.normal.map(f32::to_bits),b.normal.map(f32::to_bits));
                        },
                        _ => panic!("prepared entry changed clipping result"),
                    }
                }
            }
        }
    }

    #[test]
    fn workstation_normal_transmission_matches_beer_lambert_and_fresnel() {
        let ior = 1.766;
        let panel = slab(ior, [1., 2., 3.]);
        let result = shadow(
            &[Some(panel), None, None, None],
            [0., 0., -2.],
            [0., 0., 1.],
            4.,
        );
        let f = ((ior - 1.) / (ior + 1.)).powi(2);
        for channel in 0..3 {
            let expected = (-panel.sigma[channel]).exp() * (1. - f).powi(2);
            assert!((result[channel] - expected).abs() < 2e-6);
        }
    }

    #[test]
    fn workstation_oblique_entry_refracts_before_exit_fresnel() {
        let ior = 1.766;
        let panel = slab(ior, [0.; 3]);
        // Air cosine .8 would produce false total internal reflection if reused
        // at exit. Parallel glass faces instead have equal entry/exit reflectance.
        assert_eq!(fresnel(0.8, ior, 1.), 1.);
        let result = shadow(
            &[Some(panel), None, None, None],
            [0., 0., -2.],
            [0.6, 0., 0.8],
            5.,
        );
        let expected = (1. - fresnel(0.8, 1., ior)).powi(2);
        assert!(expected > 0.5);
        assert!((result[0] - expected).abs() < 1e-6);
    }

    #[test]
    fn workstation_missed_or_behind_emitter_glass_does_not_occlude() {
        let panels = [Some(slab(1.766, [10.; 3])), None, None, None];
        assert_eq!(shadow(&panels, [5., 0., -2.], [0., 0., 1.], 4.), [1.; 3]);
        assert_eq!(shadow(&panels, [0., 0., -2.], [0., 0., 1.], 1.), [1.; 3]);
    }

    #[test]
    fn workstation_empty_and_malformed_inputs_produce_finite_unit_field() {
        let mut rows = [[0.; 4]; INPUT_ROWS];
        for input in [&[][..], &rows[..]] {
            let values = build(input);
            assert_eq!(values.len(), ROWS);
            assert!(values.iter().all(|v| *v == UNOBSTRUCTED));
        }
        rows[32][0] = f32::NAN;
        assert!(build(&rows).iter().all(|v| *v == UNOBSTRUCTED));
    }

    fn optical_fixture() -> [[f32; 4]; INPUT_ROWS] {
        let mut rows = [[0.; 4]; INPUT_ROWS];
        rows[0][1] = 1.;
        rows[2][3] = 0.75;
        rows[3][3] = 0.75;
        rows[5][3] = -2.;
        rows[9][0] = 1.;
        rows[16] = [0., 0., 2., 0.];
        rows[17] = [1., 0., 0., 0.2];
        rows[18] = [0., 1., 0., 0.2];
        rows[19] = [0., 0., -1., 0.];
        let panel = slab(1.766, [0.2, 0.8, 2.]);
        rows[32] = [1., panel.ior, 0., 1.];
        rows[33] = [0.2, 0.8, 2., 0.];
        rows[35] = [4., 4., 0.5, 0.];
        rows[38..64].copy_from_slice(&panel.planes);
        rows
    }

    #[test]
    fn workstation_area_light_field_retains_colored_absorption_and_bounds() {
        let rows = optical_fixture();
        let field = build(&rows);
        assert!(
            field
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v))
        );
        let center = field[(EDGE / 2) * EDGE + EDGE / 2];
        assert!(center[0] < 1. && center[0] > center[1] && center[1] > center[2]);
        assert!(field[EDGE * EDGE..].iter().all(|v| *v == UNOBSTRUCTED));
    }

    fn bits(field: &[[f32; 4]]) -> Vec<[u32; 4]> {
        field.iter().map(|row| row.map(f32::to_bits)).collect()
    }

    #[test]
    fn workstation_floor_panel_cache_keys_separate_panel_and_light_dependencies() {
        let original = optical_fixture();
        let CacheKey::Scene(scene) = cache_key(&original) else { panic!("fixture scene"); };
        let mut caches: [LightCache; 3] = Default::default();
        for (i, cache) in caches.iter_mut().enumerate() {
            assert_eq!(cache.prepare(&scene, i), 0b1111);
            assert_eq!(cache.prepare(&scene, i), 0);
        }
        let mut moved = scene;
        moved[34][0] = 0.25f32.to_bits();
        for (i, cache) in caches.iter_mut().enumerate() { assert_eq!(cache.prepare(&moved, i), 1); }
        moved[33][0] = 0.5f32.to_bits();
        for (i, cache) in caches.iter_mut().enumerate() { assert_eq!(cache.prepare(&moved, i), 1); }
        moved[16][0] = 0.3f32.to_bits();
        assert_eq!(caches[0].prepare(&moved, 0), 0b1111);
        assert_eq!(caches[1].prepare(&moved, 1), 0);
        assert_eq!(caches[2].prepare(&moved, 2), 0);
        moved[5][3] = (-1.5f32).to_bits();
        for (i, cache) in caches.iter_mut().enumerate() { assert_eq!(cache.prepare(&moved, i), 0b1111); }
        let bytes: usize = caches.iter().map(|c| c.factors.len() * std::mem::size_of::<[V3; 4]>() + c.modes.len()).sum();
        assert_eq!(bytes, 3 * EDGE * EDGE * 16 * 49);
    }

    #[test]
    fn workstation_floor_sample_cache_is_explicit_and_default_path_allocates_no_sample_storage() {
        let mut rows = optical_fixture();
        let mut off = Cache::with_sample_reuse(false);
        let mut on = Cache::with_sample_reuse(true);
        assert_eq!(bits(off.field(&rows)), bits(on.field(&rows)));
        assert!(on.lights.iter().all(|l| l.factors.is_empty()));
        rows[33][0] += 0.1;
        assert_eq!(bits(off.field(&rows)), bits(on.field(&rows)));
        assert!(off.lights.iter().all(|l| l.factors.is_empty() && l.modes.is_empty()));
        assert!(on.lights[0].factors.len() == EDGE * EDGE * 16);
    }

    #[test]
    fn workstation_floor_sample_cache_uses_original_rebuild_while_connections_change() {
        let mut rows = optical_fixture();
        let mut cache = Cache::with_sample_reuse(true);
        for _ in 0..3 {
            assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
            assert!(cache.lights.iter().all(|l| l.factors.is_empty()));
            rows[5][3] -= 0.1;
        }
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        rows[33][0] += 0.1;
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        assert!(cache.lights[0].shared.is_some());
        rows[5][3] -= 0.1;
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        assert!(cache.lights.iter().all(|l| l.shared.is_none()));
        rows[33][0] += 0.1;
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        assert!(cache.lights[0].shared.is_some());
    }

    #[test]
    fn workstation_floor_cache_reuses_exact_field_for_nonlighting_updates() {
        let mut rows = optical_fixture();
        let expected = bits(&build(&rows));
        let mut cache = Cache::with_sample_reuse(true);
        assert_eq!(bits(cache.field(&rows)), expected);
        // Framebuffer, time/control headers, floor color, radiance and panel
        // surface roughness change the rendered image, not this visibility field.
        for (row, word) in [(8, 0), (8, 1), (249, 3), (192, 0), (6, 0), (20, 0), (32, 2)] {
            rows[row][word] += 0.125;
        }
        assert_eq!(bits(&build(&rows)), expected);
        assert_eq!(bits(cache.field(&rows)), expected);
        assert_eq!(cache.rebuilds, 1);
    }

    #[test]
    fn workstation_floor_cache_invalidates_domain_floor_material_geometry_and_lights() {
        let original = optical_fixture();
        let mut cache = Cache::with_sample_reuse(true);
        cache.field(&original);
        for (i, (row, word)) in [(2, 3), (5, 3), (33, 0), (38, 3), (17, 3)].into_iter().enumerate() {
            let mut rows = original;
            rows[row][word] += 0.125;
            assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
            assert_eq!(cache.rebuilds, i + 2);
        }
        // Check all consumed component classes, including enable/count changes,
        // independently of whether a particular scene happens to be occluded.
        let key = cache_key(&original);
        for (row, word) in [(0, 1), (3, 3), (9, 0), (16, 0), (17, 0), (18, 0),
            (18, 3), (19, 0), (32, 0), (32, 1), (32, 3), (34, 0), (35, 0), (38, 0)] {
            let mut rows = original;
            rows[row][word] = if (row, word) == (0, 1) || (row, word) == (9, 0)
                || (row, word) == (32, 0) {0.} else if (row, word) == (32, 3) {0.875}
                else {rows[row][word] + 0.125};
            assert!(cache_key(&rows) != key, "missing input dependency {row}:{word}");
        }
    }

    #[test]
    fn workstation_floor_cache_preserves_malformed_and_custom_style_fallbacks() {
        let mut rows = optical_fixture();
        let mut cache = Cache::with_sample_reuse(true);
        let original = bits(cache.field(&rows));
        rows[249][3] = f32::NAN;
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        assert!(cache.field(&rows).iter().all(|v| *v == UNOBSTRUCTED));
        assert_eq!(bits(cache.field(&[])), bits(&build(&[])));
        assert_eq!(cache.rebuilds, 2);
        rows = optical_fixture();
        rows[32][0] = 0.; // Custom geometry replaces the convex gem.
        rows[32][1] = 500.;
        rows[38] = [2.; 4]; // Custom metadata is not a convex plane.
        assert_eq!(bits(cache.field(&rows)), bits(&build(&rows)));
        assert_eq!(cache.rebuilds, 2);
        assert_eq!(bits(cache.field(&optical_fixture())), original);
        assert_eq!(cache.rebuilds, 3);
    }

    #[test]
    fn workstation_domain_includes_view_and_finite_margin() {
        let mut rows = [[0.; 4]; INPUT_ROWS];
        rows[2][3] = 1.;
        rows[3][3] = 0.5;
        assert_eq!(domain(&rows), [-1.75, -1.25, 3.5, 2.5]);
        rows[2][3] = f32::INFINITY;
        rows[3][3] = f32::MAX;
        assert!(domain(&rows).iter().all(|v| v.is_finite()));
    }
}
