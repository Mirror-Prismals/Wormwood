//! Exact-input damage planning for the built-in direct-studio workstation.
//!
//! This module never decides whether GPU resources are unchanged. The caller
//! must advance resource_epoch for changes outside the complete optical packet
//! (shader/pipeline, vertices, bindings, auxiliary uniforms, or buffer writes),
//! and set reusable=false unless the direct-studio ruby path is selected with
//! no pending bake/resolve, procedural input, or auxiliary rendering.
//! A committed snapshot must describe pixels actually rendered into the retained
//! target. Preparing or uploading a frame must never commit its snapshot.

const PLAYHEAD: usize = 252;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Context {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub packet_rows: usize,
    pub resource_epoch: u64,
    pub reusable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let end_x = (self.x + self.width).max(other.x + other.width);
        let end_y = (self.y + self.height).max(other.y + other.height);
        Self { x, y, width: end_x - x, height: end_y - y }
    }

    pub(super) fn pixels(self) -> u64 { u64::from(self.width) * u64::from(self.height) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Damage {
    Full,
    Rect(Rect),
    Unchanged,
}

struct Snapshot {
    context: Context,
    rows: Vec<[f32; 4]>,
}

#[derive(Default)]
pub(super) struct Tracker {
    completed: Option<Snapshot>,
}

impl Tracker {
    pub(super) fn invalidate(&mut self) { self.completed = None; }

    pub(super) fn plan(&self, rows: &[[f32; 4]], context: Context) -> Damage {
        let Some(previous) = &self.completed else { return Damage::Full; };
        if previous.context != context || !supported(rows, context) {
            return Damage::Full;
        }
        let mut marker_changed = false;
        for (at, (old, new)) in previous.rows.iter().zip(rows).enumerate() {
            if old.map(f32::to_bits) != new.map(f32::to_bits) {
                if at != PLAYHEAD { return Damage::Full; }
                marker_changed = true;
            }
        }
        if !marker_changed { return Damage::Unchanged; }
        let old = marker_rect(&previous.rows, context);
        let new = marker_rect(rows, context);
        match (old, new) {
            (Ok(Some(a)), Ok(Some(b))) => Damage::Rect(a.union(b)),
            (Ok(Some(rect)), Ok(None)) | (Ok(None), Ok(Some(rect))) => Damage::Rect(rect),
            (Ok(None), Ok(None)) => Damage::Unchanged,
            _ => Damage::Full,
        }
    }

    /// Whole-image composition covers every gem shader family. Unlike the
    /// playhead certificate it exempts no packet lane or shader dependency.
    pub(super) fn plan_complete(&self, rows: &[[f32; 4]], context: Context) -> Damage {
        let Some(previous) = &self.completed else { return Damage::Full; };
        if previous.context == context && supported_complete(rows, context)
            && previous.rows.len() == rows.len()
            && previous.rows.iter().zip(rows).all(|(a,b)| a.map(f32::to_bits)==b.map(f32::to_bits)) {
            Damage::Unchanged
        } else { Damage::Full }
    }
    pub(super) fn completed_complete(&mut self, rows: &[[f32; 4]], context: Context) {
        self.completed = supported_complete(rows, context).then(|| Snapshot { context, rows: rows.to_vec() });
    }

    /// Call only after GPU completion of the render/copy commands for these inputs.
    /// The caller may supply reusable=true after a full draw has finished its
    /// ordered cache passes, provided the recorded epoch describes that output.
    pub(super) fn completed(&mut self, rows: &[[f32; 4]], context: Context) {
        self.completed = supported(rows, context).then(|| Snapshot {
            context,
            rows: rows.to_vec(),
        });
    }
}

pub(super) fn supported_complete(rows: &[[f32; 4]], c: Context) -> bool {
    c.reusable && c.format != 0 && c.packet_rows >= 256 && rows.len() == c.packet_rows
        && (1..=16_384).contains(&c.width) && (1..=16_384).contains(&c.height)
        && rows.iter().flatten().all(|v| v.is_finite())
        && rows[8][0] == c.width as f32 && rows[8][1] == c.height as f32
}

pub(super) fn supported(rows: &[[f32; 4]], c: Context) -> bool {
    c.reusable && c.format != 0 && c.packet_rows >= 256 && rows.len() == c.packet_rows
        && (1..=16_384).contains(&c.width) && (1..=16_384).contains(&c.height)
        && rows.iter().flatten().all(|v| v.is_finite())
        && rows[0][2] == 2.0 && rows[1][3] == 0.0
        && rows[2][..3] == [1.0, 0.0, 0.0] && rows[3][..3] == [0.0, 1.0, 0.0]
        && rows[4][..3] == [0.0, 0.0, -1.0]
        && rows[8][0] == c.width as f32 && rows[8][1] == c.height as f32
        && rows[245][0] == 0.0 && rows[245][2] == 0.0
        && (1e-6..=1e6).contains(&rows[2][3]) && (1e-6..=1e6).contains(&rows[3][3])
        && rows[1][..3].iter().all(|v| v.abs() <= 1e6) && rows[1][2] > 0.016
}

fn marker_rect(rows: &[[f32; 4]], c: Context) -> Result<Option<Rect>, ()> {
    let marker = rows[PLAYHEAD];
    if marker[3] <= 0.5 { return Ok(None); }
    let pixel = rows[253][0];
    if pixel <= 0.0 || pixel > 1e6 || marker[1] > marker[2]
        || marker[..3].iter().any(|v| v.abs() > 1e6) {
        return Err(());
    }
    // Match the shader's float32 construction of its marker box, including
    // rounded centre/extents, rather than assuming y/z are its exact endpoints.
    let center_y = (marker[1] + marker[2]) * 0.5;
    let half_y = (marker[2] - marker[1]) * 0.5;
    let half_x = pixel * 0.75;
    let bounds = [marker[0] - half_x, center_y - half_y,
                  marker[0] + half_x, center_y + half_y];
    let camera = rows[1];
    let half = [rows[2][3], rows[3][3]];
    // gemInverseRay uses 1e8 for the zero XY directions. Include its tiny
    // parallel-slab extension and a deliberately generous float32 roundoff
    // enclosure for sample construction, box subtraction and projection.
    let slab = (f64::from(camera[2]).abs() + 1.0) * 1e-8;
    let margin: [f64; 2] = std::array::from_fn(|axis| {
        let magnitude = 1.0 + f64::from(camera[axis]).abs() + f64::from(half[axis])
            + f64::from(bounds[axis]).abs().max(f64::from(bounds[axis + 2]).abs());
        64.0 * f64::from(f32::EPSILON) * magnitude + slab
    });
    let project_x = |world: f64| (1.0 + (world - f64::from(camera[0])) / f64::from(half[0]))
        * 0.5 * f64::from(c.width);
    let project_y = |world: f64| (1.0 - (world - f64::from(camera[1])) / f64::from(half[1]))
        * 0.5 * f64::from(c.height);
    // Keep the original full viewport. Scissors restrict only invocation
    // coverage. Two extra pixels enclose both original stratified samples.
    let x0 = (project_x(f64::from(bounds[0]) - margin[0]).floor() - 2.0)
        .clamp(0.0, f64::from(c.width)) as u32;
    let x1 = (project_x(f64::from(bounds[2]) + margin[0]).ceil() + 2.0)
        .clamp(0.0, f64::from(c.width)) as u32;
    let y0 = (project_y(f64::from(bounds[3]) + margin[1]).floor() - 2.0)
        .clamp(0.0, f64::from(c.height)) as u32;
    let y1 = (project_y(f64::from(bounds[1]) - margin[1]).ceil() + 2.0)
        .clamp(0.0, f64::from(c.height)) as u32;
    Ok((x1 > x0 && y1 > y0).then_some(Rect { x: x0, y: y0,
        width: x1.saturating_sub(x0), height: y1.saturating_sub(y0) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<[f32; 4]>, Context) {
        let mut rows = vec![[0.0; 4]; 260];
        rows[0][2] = 2.0;
        rows[1] = [0.0, 0.0, 3.0, 0.0];
        rows[2] = [1.0, 0.0, 0.0, 1.04];
        rows[3] = [0.0, 1.0, 0.0, 0.65];
        rows[4] = [0.0, 0.0, -1.0, 1.0];
        rows[8] = [1920.0, 1200.0, 16.0, 64.0];
        rows[252] = [0.0, -0.4, 0.4, 1.0];
        rows[253][0] = 1.3 / 1200.0;
        let context = Context { width: 1920, height: 1200, format: 23,
            packet_rows: rows.len(), resource_epoch: 1, reusable: true };
        (rows, context)
    }

    #[test]
    fn complete_composition_reuses_all_shader_families_but_no_changed_input() {
        let (mut rows, context)=fixture();
        // Perspective and custom/piano/sphere/bubble paths may all retain an
        // identical final image, without borrowing the playhead certificate.
        rows[1][3]=1.;rows[234][0]=1.;rows[240][0]=1.;rows[245][0]=1.;
        let mut tracker=Tracker::default();
        assert_eq!(tracker.plan_complete(&rows,context),Damage::Full);
        tracker.completed_complete(&rows,context);
        assert_eq!(tracker.plan_complete(&rows,context),Damage::Unchanged);
        for at in 0..rows.len(){for lane in 0..4 {
            let mut changed=rows.clone();changed[at][lane]=f32::from_bits(rows[at][lane].to_bits()^1);
            assert_eq!(tracker.plan_complete(&changed,context),Damage::Full,"row {at} lane {lane}");
        }}
        assert_eq!(tracker.plan_complete(&rows,Context{resource_epoch:2,..context}),Damage::Full);
        assert_eq!(tracker.plan_complete(&rows,Context{reusable:false,..context}),Damage::Full);
        tracker.invalidate();assert_eq!(tracker.plan_complete(&rows,context),Damage::Full);
    }

    #[test]
    fn only_completed_pixels_are_reused() {
        let (rows, context) = fixture();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.plan(&rows, context), Damage::Full);
        tracker.completed(&rows, context);
        assert_eq!(tracker.plan(&rows, context), Damage::Unchanged);
        let mut pending = rows.clone(); pending[252][0] += 0.01;
        assert!(matches!(tracker.plan(&pending, context), Damage::Rect(_)));
        assert_eq!(tracker.plan(&rows, context), Damage::Unchanged);
        tracker.invalidate();
        assert_eq!(tracker.plan(&rows, context), Damage::Full);
    }

    #[test]
    fn nonmarker_bits_and_external_state_force_full_redraw() {
        let (rows, context) = fixture();
        let mut tracker = Tracker::default(); tracker.completed(&rows, context);
        for at in [0, 1, 2, 5, 8, 32, 214, 245, 253, 256, 259] {
            let mut changed = rows.clone(); changed[at][0] += 0.01;
            assert_eq!(tracker.plan(&changed, context), Damage::Full, "row {at}");
        }
        let mut negative_zero = rows.clone(); negative_zero[259][3] = -0.0;
        assert_eq!(tracker.plan(&negative_zero, context), Damage::Full);
        for changed in [Context { resource_epoch: 2, ..context },
                        Context { reusable: false, ..context },
                        Context { format: 24, ..context },
                        Context { width: 1919, ..context }] {
            assert_eq!(tracker.plan(&rows, changed), Damage::Full);
        }
        assert_eq!(tracker.plan(&rows[..259], context), Damage::Full);
    }

    #[test]
    fn disappearance_clears_old_pixels_and_hidden_motion_needs_no_shading() {
        let (mut rows, context) = fixture();
        let old = marker_rect(&rows, context).unwrap().unwrap();
        let mut tracker = Tracker::default(); tracker.completed(&rows, context);
        rows[252][3] = 0.0;
        assert_eq!(tracker.plan(&rows, context), Damage::Rect(old));
        tracker.completed(&rows, context);
        rows[252][0] = 0.4;
        assert_eq!(tracker.plan(&rows, context), Damage::Unchanged);
        rows[252][3] = 1.0;
        assert_eq!(tracker.plan(&rows, context), Damage::Rect(marker_rect(&rows, context).unwrap().unwrap()));
    }

    // Independent float32 copy of the original marker slab test, at the exact
    // two shader sample positions. This checks coverage, not a mirrored formula
    // for the planner's projected rectangle.
    fn shader_hits(rows: &[[f32; 4]], c: Context, x: u32, y: u32, sample: usize) -> bool {
        let xi = [[0.375_f32, 0.125_f32], [0.875, 0.875]][sample];
        let screen = [(x as f32 + 0.5) * 2.0 / c.width as f32 - 1.0,
                      1.0 - (y as f32 + 0.5) * 2.0 / c.height as f32];
        let origin = [rows[1][0] + (screen[0] + (xi[0] - 0.5) * 2.0 / c.width as f32) * rows[2][3],
                      rows[1][1] + (screen[1] + (xi[1] - 0.5) * 2.0 / c.height as f32) * rows[3][3], rows[1][2]];
        let m = rows[252];
        if m[3] <= 0.5 { return false; }
        let center = [m[0], (m[1] + m[2]) * 0.5, 0.015];
        let extent = [rows[253][0] * 0.75, (m[2] - m[1]) * 0.5, 0.0005];
        let mut near = f32::NEG_INFINITY; let mut far = f32::INFINITY;
        for axis in 0..3 {
            let inverse = if axis == 2 { -1.0 } else { 1e8 };
            let a = (center[axis] - extent[axis] - origin[axis]) * inverse;
            let b = (center[axis] + extent[axis] - origin[axis]) * inverse;
            near = near.max(a.min(b)); far = far.min(a.max(b));
        }
        near > 1e-7 && far >= near
    }

    #[test]
    fn rectangle_encloses_actual_shader_samples_at_edges_and_fractional_positions() {
        let (mut rows, mut c) = fixture();
        c.width = 192; c.height = 120; rows[8][0] = 192.0; rows[8][1] = 120.0;
        for x in [-1.041, -1.04, -1.039, -0.123456, 0.0, 0.123456, 1.039, 1.04, 1.041] {
            rows[252] = [x, -0.649, 0.651, 1.0];
            let rect = marker_rect(&rows, c).unwrap();
            for y in 0..c.height { for px in 0..c.width { for sample in 0..2 {
                if shader_hits(&rows, c, px, y, sample) {
                    let r = rect.expect("visible sample omitted");
                    assert!(px >= r.x && px < r.x + r.width && y >= r.y && y < r.y + r.height,
                        "sample {px},{y}:{sample} omitted at marker {x}");
                }
            }}}
        }
    }

    #[test]
    fn native_playhead_motion_remains_small_and_invalid_inputs_decline() {
        let (mut rows, context) = fixture();
        let mut tracker = Tracker::default(); tracker.completed(&rows, context);
        rows[252][0] += 1.3 / 1200.0;
        let Damage::Rect(rect) = tracker.plan(&rows, context) else { panic!("missing damage"); };
        assert!(rect.pixels() < u64::from(context.width) * u64::from(context.height) / 100);
        for (at, lane, value) in [(252, 0, f32::NAN), (252, 1, 0.5), (253, 0, -1.0),
                                  (1, 3, 1.0), (245, 2, 1.0)] {
            let mut invalid = rows.clone(); invalid[at][lane] = value;
            assert_eq!(tracker.plan(&invalid, context), Damage::Full);
        }
    }
}
