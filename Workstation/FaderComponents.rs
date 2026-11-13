//! Original GLB component poses. All authored triangles/materials are retained.
//! Source-defined controller rectangles drive the actual stationary/slider
//! transforms, so clicking a knob and seeing it use exactly the same geometry.
use serde_json::Value;
use std::path::Path;
pub(crate) const BASE: usize = 43_152;
pub(crate) const ROWS: usize = 512;
pub(crate) const EDITOR_BASE: usize = 40_992;
pub(crate) const EDITOR_ROWS: usize = 2160;
pub(crate) const MAX_TRACKS: usize = 64;
pub(crate) const STRIDE: usize = 6;
pub(crate) const HEADER: usize = 8;
pub(crate) const CAP_EDGE:usize=128;
pub(crate) const CAP_BASE:usize=super::track_io_scene::BASE+super::track_io_scene::ROWS;
pub(crate) const MATERIAL_FIRST: i32 = 800_000;
pub(crate) const MATERIAL_END: i32 = 853_760;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Component {
    pub root: usize,
    pub end: usize,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
pub(crate) struct Kit {
    stationary: Component,
    slider: Component,
    meter: Component,
    preview_y: f64,
    left: [usize; 2],
    right: [usize; 2],
}
fn n(v: &Value, key: &str) -> Result<usize, String> {
    v[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("invalid mixer {key}"))
}
fn component(v: &Value, used: usize) -> Result<Component, String> {
    let a = Component {
        root: n(v, "root")?,
        end: n(v, "end")?,
        min: serde_json::from_value(v["min"].clone()).map_err(|e| e.to_string())?,
        max: serde_json::from_value(v["max"].clone()).map_err(|e| e.to_string())?,
    };
    if a.root < 12544
        || a.end <= a.root
        || (a.end - a.root) % 3 != 0
        || a.end + n(v, "triangles")? * 6 > 12544 + used
        || (0..3).any(|i| !a.min[i].is_finite() || !a.max[i].is_finite() || a.min[i] >= a.max[i])
    {
        return Err("invalid mixer component bounds".into());
    }
    Ok(a)
}
impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("Art/AudioWorkstation/DecorativeControls/controls.json");
        let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let used = n(&v, "rows")?;
        if n(&v, "base")? != 12544 || 12544 + used > EDITOR_BASE {
            return Err("mixer components overlap editor dynamic rows".into());
        }
        let assets = v["assets"].as_array().ok_or("mixer assets required")?;
        let f = assets
            .iter()
            .find(|a| a["name"] == "GemFader_Ivory")
            .ok_or("missing authored fader")?;
        let m = assets
            .iter()
            .find(|a| a["name"] == "GemMeter_Ivory")
            .ok_or("missing authored meter")?;
        let motion = &f["motion_components"];
        let preview_y = motion["preview_y_m"]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("missing slider origin")?;
        let materials = m["live_spectrum"]["materials"]
            .as_array()
            .ok_or("missing meter material roles")?;
        let range = |channel: &str| -> Result<[usize; 2], String> {
            let mut a = materials
                .iter()
                .filter(|m| m["channel"] == channel)
                .map(|m| n(m, "material"))
                .collect::<Result<Vec<_>, _>>()?;
            a.sort();
            a.dedup();
            if a.is_empty() || a.len() != a[a.len() - 1] - a[0] + 1 || a[a.len() - 1] >= 256 {
                return Err("meter palette must have explicit contiguous material roles".into());
            }
            Ok([a[0], a[a.len() - 1]])
        };
        Ok(Self {
            stationary: component(&motion["stationary"], used)?,
            slider: component(&motion["slider"], used)?,
            meter: component(m, used)?,
            preview_y,
            left: range("L")?,
            right: range("R")?,
        })
    }
    /// view is logical point width/height. depth is resolved logical bottom depth.
    /// Caller must append after static controls, before retained content key.
    pub(crate) fn append(
        &self,
        rows: &mut [[f32; 4]],
        view: [f64; 2],
        depth: f64,
        p: &Value,
    ) -> Result<(), String> {
        if rows.len() < BASE + ROWS {
            return Err("mixer needs complete dynamic frame".into());
        }
        rows[BASE..BASE + ROWS].fill([0.; 4]);
        if p["page"] != 0 {
            return Ok(());
        }
        if view.iter().any(|v| !v.is_finite() || *v <= 0.) || !depth.is_finite() {
            return Err("invalid mixer viewport".into());
        }
        let mpp = 1.3 / view[1];
        let to_world = |r: [f64; 4]| {
            [
                (r[0] + r[2] * 0.5 - view[0] * 0.5) * mpp,
                (view[1] * 0.5 - r[1] - r[3] * 0.5) * mpp,
                r[2] * mpp,
                r[3] * mpp,
            ]
        };
        let outer = rect(&p["panel"])?;
        let panel = if p["content"].is_array() {
            rect(&p["content"])?
        } else {
            [
                outer[0] + 118.,
                outer[1] + 36.,
                (outer[2] - 158.).max(0.),
                (outer[3] - 60.).max(0.),
            ]
        };
        let clip = [
            (panel[0] - view[0] * 0.5) * mpp,
            (view[1] * 0.5 - panel[1] - panel[3]) * mpp,
            (panel[0] + panel[2] - view[0] * 0.5) * mpp,
            (view[1] * 0.5 - panel[1]) * mpp,
        ];
        let mut records = Vec::new();
        let faders = p["faders"].as_array().ok_or("mixer faders required")?;
        if faders.len() > MAX_TRACKS {
            return Err("mixer track count exceeds document limit".into());
        }
        for f in faders {
            let h = rect(&f["housing"])?;
            let k = rect(&f["knob"])?;
            let l = rect(&f["meters"][0])?;
            let r = rect(&f["meters"][1])?;
            if !intersects(h, panel) && !intersects(l, panel) && !intersects(r, panel) {
                continue;
            }
            let spectrum = [l[0], l[1], r[0] + r[2] - l[0], l[3]];
            let z = mpp * 1000.;
            records.push([
                to_world(h),
                to_world(k),
                to_world(spectrum),
                [depth, z, level(&f["levels"][0])?, level(&f["levels"][1])?],
                clip,
                [1., f["normalized"].as_f64().unwrap_or(0.), 0., 0.],
            ]);
        }
        let masters = p["master_meters"]
            .as_array()
            .ok_or("master meters required")?;
        if masters.len() > 64 {
            return Err("mixer bus meter capacity exceeded".into());
        }
        for master in masters {
            let r = rect(&master["rect"])?;
            if !intersects(r, panel) {
                continue;
            }
            let peak = level(&master["level"])?;
            records.push([
                [0.; 4],
                [0.; 4],
                to_world(r),
                [depth, mpp * 1000., peak, peak],
                clip,
                // Layout labels identify mix buses independently of bus order/count.
                [2., 0., if master["label"].as_str().unwrap_or("").starts_with("MX") { 2. } else { 1. }, 0.],
            ]);
        }
        if HEADER + records.len() * STRIDE > ROWS {
            return Err("mixer dynamic packet capacity exceeded".into());
        }
        rows[BASE] = [
            records.len() as f32,
            12544.,
            self.stationary.min[2] as f32,
            1.,
        ];
        rows[BASE + 1] = [
            self.stationary.root as f32,
            self.stationary.end as f32,
            self.slider.root as f32,
            self.slider.end as f32,
        ];
        rows[BASE + 2] = [
            self.meter.root as f32,
            self.meter.end as f32,
            self.preview_y as f32,
            self.meter.min[2] as f32,
        ];
        rows[BASE + 6] = [if p["fader_drag"]==true {1.} else {0.},CAP_BASE as f32,0.,0.];
        rows[BASE + 5] = [
            self.left[0] as f32,
            self.left[1] as f32,
            self.right[0] as f32,
            self.right[1] as f32,
        ];
        for (i, r) in records.iter().enumerate() {
            for (j, row) in r.iter().enumerate() {
                rows[BASE + HEADER + i * STRIDE + j] = row.map(|v| v as f32);
            }
        }
        // Bounds are derived from the real component root boxes and transforms.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for i in 0..records.len() {
            for part in 0..3 {
                if let Some((a, t, s)) = self.pose(&records[i], part) {
                    for axis in 0..3 {
                        lo[axis] = lo[axis].min(t[axis] + a.min[axis] * s[axis]);
                        hi[axis] = hi[axis].max(t[axis] + a.max[axis] * s[axis]);
                    }
                }
            }
        }
        if !records.is_empty() {
            rows[BASE + 3] = [lo[0] as f32, lo[1] as f32, lo[2] as f32, 0.];
            rows[BASE + 4] = [hi[0] as f32, hi[1] as f32, hi[2] as f32, 0.];
        }
        Ok(())
    }
    fn pose(&self, r: &[[f64; 4]; 6], part: usize) -> Option<(Component, [f64; 3], [f64; 3])> {
        if r[5][0] == 2. && part < 2 {
            return None;
        }
        let rect = r[part];
        if rect[2] <= 0. || rect[3] <= 0. {
            return None;
        }
        let a = [self.stationary, self.slider, self.meter][part];
        let w = [0.04, 0.025, 0.01][part];
        let h = [0.18, 0.025, 0.156][part];
        let s = [rect[2] / w, rect[3] / h, r[3][1]];
        let mut t = [
            rect[0],
            rect[1],
            r[3][0]
                - if part == 2 {
                    self.meter.min[2]
                } else {
                    self.stationary.min[2]
                } * s[2],
        ];
        if part == 1 {
            t[1] -= self.preview_y * s[1];
        }
        Some((a, t, s))
    }
}
fn rect(v: &Value) -> Result<[f64; 4], String> {
    let a: [f64; 4] = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    if a.iter().any(|v| !v.is_finite()) || a[2] < 0. || a[3] < 0. {
        return Err("invalid mixer rectangle".into());
    }
    Ok(a)
}
fn level(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0., 1.))
        .ok_or_else(|| "invalid peak meter level".into())
}
fn intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[0] + b[2] && a[0] + a[2] > b[0] && a[1] < b[1] + b[3] && a[1] + a[3] > b[1]
}
