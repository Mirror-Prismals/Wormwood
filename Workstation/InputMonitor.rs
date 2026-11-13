//! Authored Cardinal traffic lights in the central timeline's right gutter.
//! Geometry and three material palettes are immutable; frames carry poses only.
//! Document mode chooses the authored lamp palette; lane picking uses this same placement.
use super::{
    arrangement, engraving,
    panel_layout::{self, Options, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::path::Path;

pub(crate) const BASE: usize = engraving::DENSITY_BASE + engraving::DENSITY_ROWS;
pub(crate) const ROWS: usize = 1_310_720;
pub(crate) const POSE_BASE: usize = 226;
// Bubble-note headers occupy 234..238; this group is independent of both panels.
pub(crate) const GROUP_BASE: usize = 238;
// Keep legacy diagnostic instances10..12 and plug-in instances13..28 stable.
// Live lane monitors use instances29..60 in otherwise unused frame storage.
pub(crate) const LANE_HEADER: usize = 247;
pub(crate) const LANE_POSE_BASE: usize = engraving::BASE + 368;
pub(crate) const MAX_LANES: usize = arrangement::MAX_VISIBLE_TRACKS;
const _: () = assert!(LANE_POSE_BASE + MAX_LANES * 2 <= engraving::BASE + 448);
const MATERIALS: usize = 39;
const PATH: &str = "Art/AudioWorkstation/InputMonitor";

/// Source: Cardinal DawLaneTimelineSystem::ComputeLaneLayout and
/// DawInputMonitoringSystem::SignalPlacement. Explicit diagnostic only:
/// these are three appearance specimens at the inherited 60 px lane defaults,
/// not document tracks (new Cardinal tracks instead start at 180 px).
struct PreviewLayout {
    column_left: f64,
    first_center_y: f64,
    clip_top: f64,
    clip_bottom: f64,
}

fn preview_layout(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<PreviewLayout, String> {
    let panels = panel_layout::visual_bounds(view, motion, options)?;
    let (left, right) = if options.swap_sides { (3, 2) } else { (2, 3) };
    let (top, bottom) = if options.swap_horizontal {
        (1, 0)
    } else {
        (0, 1)
    };
    let mut l = panels[left].x + panels[left].width;
    let mut r = panels[right].x;
    if r - l <= 1. {
        (l, r) = (0., view.width);
    }
    if view.canvas_scale >= 0.999 {
        l = l.clamp(0., view.width);
        r = r.clamp(l, view.width);
    }
    // Resolved geometry already accounts for closed, moving and snapped
    // panels; gating it by the animated slot would lag behind a snap model.
    let t = (panels[top].y + panels[top].height).max(0.);
    let b = panels[bottom].y.min(view.height).max(t + 1.);
    Ok(PreviewLayout {
        // 40 px margin + monitor 57 + meter 14 + fader 32; left handles 25.
        // Retain Cardinal's 200 px minimum lane width on narrow viewports.
        column_left: (r - 143.).max(l + 65. + 200.),
        first_center_y: t + 160. - 42.,
        clip_top: t + 160. - 42. - 30. - 15.,
        clip_bottom: b - 40.,
    })
}

pub(crate) fn signal_scale(lane_height: f64) -> f64 {
    (1.5 * ((lane_height - 4.) / 404.).clamp(0.025, 0.20)).min((lane_height - 4.).max(1.) / 358.)
}

/// Inverse of the source SignalPlacement/HitMode, in logical screen points.
/// The GLB origin is the source's (0,171), so lamp targets remain 65/175/285.
pub(crate) fn hit_mode(column_left:f64,center_y:f64,lane_height:f64,p:[f64;2])->Option<u8> {
    let s=signal_scale(lane_height);
    let x=column_left+arrangement::MONITOR_GUTTER_WIDTH*0.5+10.*s;
    let y=center_y-167.*s;
    let local=[(p[0]-x)/s,(p[1]-y)/s];
    if !(-96. ..=76.).contains(&local[0]) || !(12. ..=338.).contains(&local[1]) {return None;}
    Some((((local[1]-12.)/110.) as u8).min(2))
}

pub(crate) struct Kit {
    rows: usize,
    root: usize,
    end: usize,
    triangles: usize,
    material_bases: [usize; 3],
    min: [f64; 3],
    max: [f64; 3],
}

impl Kit {
    /// Visual lane furniture follows the arrangement's actual ordered rows.
    /// Off/Auto/In is the actual document mode, independent of record arming.
    pub(crate) fn append_lanes(
        &self,
        rows: &mut [[f32; 4]],
        view: Viewport,
        value: &Value,
    ) -> Result<(), String> {
        if rows.len() < LANE_POSE_BASE + MAX_LANES * 2 {
            return Err("input monitors need complete frame storage".into());
        }
        let mut packed = [[0.; 4]; MAX_LANES * 2];
        let mut group = [[0.; 4]; 2];
        let mut count = 0;
        if !value.is_null() {
            let canvas = &value["canvas"];
            let number = |key: &str, index: usize| -> Result<f64, String> {
                (if canvas.is_array() {
                    &canvas[index]
                } else {
                    &canvas[key]
                })
                .as_f64()
                .filter(|n| n.is_finite() && n.abs() <= 1e6)
                .ok_or_else(|| format!("invalid monitor canvas {key}"))
            };
            let (left, top, width, height) = (
                number("left", 0)?,
                number("top", 1)?,
                number("width", 2)?,
                number("height", 3)?,
            );
            let tracks = value["tracks"]
                .as_array()
                .ok_or("monitor tracks must be an array")?;
            let scroll = value["scroll_tracks"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("invalid monitor track scroll")?;
            if width > arrangement::BODY_OFFSET + arrangement::MONITOR_GUTTER_WIDTH
                && height > arrangement::RULER_HEIGHT
            {
                let mpp = 1.3 / view.height;
                let visible=arrangement::lane_count(tracks,scroll,height-arrangement::RULER_HEIGHT);
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for (index, track) in tracks.iter().enumerate().skip(scroll).take(visible) {
                    // Preserve CC lane spacing; it has no input-monitor signal.
                    if track["comp_parent"].is_string() || !matches!(track["kind"].as_str(), Some("audio" | "midi")) {
                        continue;
                    }
                    let lane_height=arrangement::lane_height(track);
                    let s=signal_scale(lane_height);let scale=s*mpp/0.0005;
                    let x=left+width-super::timeline_controls::reserve(super::timeline_controls::flags(value))-arrangement::MONITOR_GUTTER_WIDTH*0.5+10.*s;
                    let y=top+arrangement::RULER_HEIGHT+arrangement::lane_top(tracks,scroll,index)+lane_height*0.5+4.*s;
                    let at = count * 2;
                    packed[at] = [
                        ((x - view.width * 0.5) * mpp) as f32,
                        ((view.height * 0.5 - y) * mpp) as f32,
                        (-self.min[2] * scale + 0.0005) as f32,
                        scale as f32,
                    ];
                    packed[at + 1] = [
                        self.root as f32,
                        self.end as f32,
                        0.,
                        self.material_bases[track["input_monitor_mode"].as_u64().unwrap_or(if track["kind"]=="midi" {2}else{0}).min(2) as usize] as f32,
                    ];
                    for axis in 0..3 {
                        lo[axis] = lo[axis].min(packed[at][axis] as f64 + self.min[axis] * scale);
                        hi[axis] = hi[axis].max(packed[at][axis] as f64 + self.max[axis] * scale);
                    }
                    count += 1;
                }
                if count > 0 {
                    let clip_min = (view.height * 0.5 - top - height) * mpp;
                    let clip_max = (view.height * 0.5 - top - arrangement::RULER_HEIGHT) * mpp;
                    lo[1] = lo[1].max(clip_min);
                    hi[1] = hi[1].min(clip_max);
                    for axis in 0..3 {
                        group[0][axis] = ((lo[axis] + hi[axis]) * 0.5) as f32;
                        group[1][axis] = ((hi[axis] - lo[axis]).max(0.) * 0.5 + 1e-6) as f32;
                    }
                    group[0][3] = clip_min as f32;
                    group[1][3] = clip_max as f32;
                }
            }
        }
        rows[POSE_BASE..POSE_BASE + 6].fill([0.; 4]);
        rows[LANE_POSE_BASE..LANE_POSE_BASE + MAX_LANES * 2].copy_from_slice(&packed);
        rows[GROUP_BASE..GROUP_BASE + 2].copy_from_slice(&group);
        rows[LANE_HEADER] = [LANE_POSE_BASE as f32, count as f32, 0., 0.];
        Ok(())
    }

    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join(PATH).join("runtime.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let integer = |name: &str| -> Result<usize, String> {
            data[name]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| format!("invalid input monitor {name}"))
        };
        let kit = Self {
            rows: integer("rows")?,
            root: integer("root")?,
            end: integer("end")?,
            triangles: integer("triangles")?,
            material_bases: serde_json::from_value(data["material_bases"].clone())
                .map_err(|e| e.to_string())?,
            min: serde_json::from_value(data["min"].clone()).map_err(|e| e.to_string())?,
            max: serde_json::from_value(data["max"].clone()).map_err(|e| e.to_string())?,
        };
        if integer("schema")? != 1
            || integer("base")? != BASE
            || integer("capacity")? != ROWS
            || integer("material_count")? != MATERIALS
            || kit.rows > ROWS
            || kit.root != BASE + 3 * MATERIALS * 40
            || kit.end <= kit.root
            || kit.end > BASE + kit.rows
            || (kit.end - kit.root) % 3 != 0
            || integer("nodes")? != (kit.end - kit.root) / 3
            || integer("nodes")? > 131_072
            || kit.triangles != 128_450
            || kit.end + kit.triangles * 6 != BASE + kit.rows
            || kit.material_bases != [BASE, BASE + MATERIALS * 40, BASE + 2 * MATERIALS * 40]
            || kit.min.iter().chain(&kit.max).any(|v| !v.is_finite())
            || (0..3).any(|i| kit.min[i] >= kit.max[i])
        {
            return Err("invalid authored input monitor package; re-export the GLBs".into());
        }
        Ok(kit)
    }

    /// Populate the immutable extension once during native graphics preparation.
    pub(crate) fn read_geometry(&self, root: &Path) -> Result<Vec<[f32; 4]>, String> {
        let bytes =
            std::fs::read(root.join(PATH).join("runtime.bin")).map_err(|e| e.to_string())?;
        if bytes.len() != self.rows * 16 {
            return Err("input monitor geometry length disagrees with its manifest".into());
        }
        let rows: Vec<[f32; 4]> = bytes
            .chunks_exact(16)
            .map(|bytes| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                })
            })
            .collect();
        if rows.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite input monitor geometry".into());
        }
        let address = |v: f32| -> Result<usize, String> {
            if v < BASE as f32 || v > (BASE + self.rows) as f32 || v.fract() != 0. {
                return Err("invalid input monitor BVH address".into());
            }
            Ok(v as usize)
        };
        for node in (self.root..self.end).step_by(3) {
            let at = node - BASE;
            let escape = address(rows[at][3])?;
            let first = address(rows[at + 1][3])?;
            let count = rows[at + 2][0];
            if escape <= node
                || escape > self.end
                || (escape - self.root) % 3 != 0
                || !(0.0..=4.).contains(&count)
                || count.fract() != 0.
                || first < self.end
                || first + count as usize * 6 > BASE + self.rows
                || (first - self.end) % 6 != 0
                || (0..3).any(|i| rows[at][i] > rows[at + 1][i])
            {
                return Err("invalid input monitor BVH node".into());
            }
        }
        for triangle in (self.end - BASE..self.rows).step_by(6) {
            let material = rows[triangle][3];
            if !(0.0..MATERIALS as f32).contains(&material) || material.fract() != 0. {
                return Err("invalid input monitor triangle material".into());
            }
        }
        Ok(rows)
    }

    /// Decorative Off/Auto/In examples share the exact same geometry. Palette
    /// selection represents the chosen mode, not whether audio is flowing.
    pub(crate) fn append(
        &self,
        rows: &mut [[f32; 4]],
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<(), String> {
        if rows.len() < 256 || rows[12][0] != 10. {
            return Err("input monitors require the ten existing workstation controls".into());
        }
        let layout = preview_layout(view, motion, options)?;
        let mpp = 1.3 / view.height;
        let s = signal_scale(60.);
        // The existing Blender model uses .0005 m per Cardinal source unit.
        // Its pivot is at source Y=171; SignalPlacement subtracts 167*s.
        let scale = s * mpp / 0.0005;
        let x = layout.column_left + 28.5 + 10. * s;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for mode in 0..3 {
            let at = POSE_BASE + mode * 2;
            let y = layout.first_center_y + mode as f64 * 63. + 4. * s;
            rows[at] = [
                ((x - view.width * 0.5) * mpp) as f32,
                ((view.height * 0.5 - y) * mpp) as f32,
                (-self.min[2] * scale + 0.0005) as f32,
                scale as f32,
            ];
            rows[at + 1] = [
                self.root as f32,
                self.end as f32,
                0.,
                self.material_bases[mode] as f32,
            ];
            for axis in 0..3 {
                lo[axis] = lo[axis].min(rows[at][axis] as f64 + self.min[axis] * scale);
                hi[axis] = hi[axis].max(rows[at][axis] as f64 + self.max[axis] * scale);
            }
        }
        rows[12][0] = 13.;
        // Cardinal scissors lane furniture between its ruler and bottom margin.
        // Both the group box and individual triangle hits obey that clip.
        let clip_min = (view.height * 0.5 - layout.clip_bottom) * mpp;
        let clip_max = (view.height * 0.5 - layout.clip_top) * mpp;
        lo[1] = lo[1].max(clip_min);
        hi[1] = hi[1].min(clip_max);
        for axis in 0..3 {
            rows[GROUP_BASE][axis] = ((lo[axis] + hi[axis]) * 0.5) as f32;
            rows[GROUP_BASE + 1][axis] = ((hi[axis] - lo[axis]).max(0.) * 0.5 + 1e-6) as f32;
        }
        rows[GROUP_BASE][3] = clip_min as f32;
        rows[GROUP_BASE + 1][3] = clip_max as f32;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_hit_mode_selects_individual_lamps_at_compact_and_large_scales(){
        for height in [20.,64.,180.] {
            let s=signal_scale(height);let x=100.+arrangement::MONITOR_GUTTER_WIDTH*0.5+10.*s;
            let top=200.-167.*s;
            for mode in 0..3 {assert_eq!(hit_mode(100.,200.,height,[x,top+(65.+110.*mode as f64)*s]),Some(mode));}
            assert_eq!(hit_mode(100.,200.,height,[x-97.*s,200.]),None);
            assert_eq!(hit_mode(100.,200.,height,[x,top+339.*s]),None);
        }
    }
    #[test]
    fn workstation_input_monitors_follow_document_lanes_and_clear_without_timeline() {
        use serde_json::json;
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let view = Viewport {
            width: 1280.,
            height: 720.,
            canvas_scale: 1.,
        };
        let mut rows = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        let original = rows.clone();
        let mut timeline = json!({
            "canvas": {"left": 282., "top": 242.-(arrangement::RULER_HEIGHT-32.), "width": 734., "height": 254.+(arrangement::RULER_HEIGHT-32.)},
            "scroll_tracks": 0,
            "tracks": [{"id":"a","kind":"audio","lane_height":64.},{"id":"cc","kind":"cc"},
                {"id":"m","kind":"midi"},{"id":"offscreen","kind":"audio","lane_height":64.}]
        });
        kit.append_lanes(&mut rows, view, &timeline).unwrap();
        assert_eq!(rows[LANE_HEADER], [LANE_POSE_BASE as f32, 2., 0., 0.]);
        let mpp = 1.3 / view.height;
        let s = signal_scale(64.);
        for (packed_index, lane) in [0, 2].into_iter().enumerate() {
            let pose = rows[LANE_POSE_BASE + packed_index * 2];
            let x = pose[0] as f64 / mpp + view.width * 0.5;
            let y = view.height * 0.5 - pose[1] as f64 / mpp;
            assert!((x - (1016. - 28.5 + 10. * s)).abs() < 0.0001);
            assert!((y - (306. + lane as f64 * 64. + 4. * s)).abs() < 0.0001);
            assert_eq!(rows[LANE_POSE_BASE + packed_index * 2 + 1][3], kit.material_bases[if lane==2{2}else{0}] as f32);
        }
        for i in 0..rows.len() {
            if i != LANE_HEADER
                && !(POSE_BASE..POSE_BASE + 6).contains(&i)
                && !(GROUP_BASE..GROUP_BASE + 2).contains(&i)
                && !(LANE_POSE_BASE..LANE_POSE_BASE + MAX_LANES * 2).contains(&i)
            {
                assert_eq!(rows[i], original[i], "unrelated frame row {i}");
            }
        }
        // Scrolling keeps a CC gap while compacting only the GPU pose records.
        timeline["scroll_tracks"] = json!(1);
        kit.append_lanes(&mut rows, view, &timeline).unwrap();
        let y = view.height * 0.5 - rows[LANE_POSE_BASE][1] as f64 / mpp;
        assert!((y - (370. + 4. * s)).abs() < 0.0001);
        // Drag preview order, supplied by Arrangement, is the drawing order.
        timeline["tracks"].as_array_mut().unwrap().swap(1, 2);
        kit.append_lanes(&mut rows, view, &timeline).unwrap();
        let y = view.height * 0.5 - rows[LANE_POSE_BASE][1] as f64 / mpp;
        assert!((y - (306. + 4. * s)).abs() < 0.0001);
        let valid = rows.clone();
        timeline["canvas"]["width"] = json!("invalid");
        assert!(kit.append_lanes(&mut rows, view, &timeline).is_err());
        assert_eq!(
            rows, valid,
            "invalid packet must not partly mutate the frame"
        );
        kit.append_lanes(&mut rows, view, &Value::Null).unwrap();
        assert_eq!(rows[LANE_HEADER][1], 0.);
        assert_eq!(rows[GROUP_BASE..GROUP_BASE + 2], [[0.; 4]; 2]);
        assert!(
            rows[LANE_POSE_BASE..LANE_POSE_BASE + MAX_LANES * 2]
                .iter()
                .all(|r| *r == [0.; 4])
        );
        timeline["tracks"] = json!([]);
        timeline["canvas"]["width"] = json!(734.);
        kit.append_lanes(&mut rows, view, &timeline).unwrap();
        assert_eq!(rows[LANE_HEADER][1], 0., "empty document has no specimens");
        timeline["canvas"]["width"] = json!(100.);
        kit.append_lanes(&mut rows, view, &timeline).unwrap();
        assert_eq!(rows[LANE_HEADER][1], 0., "covered canvas has no monitors");
    }

    #[test]
    fn workstation_input_monitor_preserves_geometry_and_exclusive_lamp_palettes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let kit = Kit::load(root).unwrap();
        let rows = kit.read_geometry(root).unwrap();
        // Each exported state emits only its selected color; no all-dark Off.
        for mode in 0..3 {
            let mut lit = 0;
            for material in 0..MATERIALS {
                let at = kit.material_bases[mode] - BASE + material * 40;
                assert_eq!(rows[at][3], 0., "opaque authored signal");
                let rgb = &rows[at + 5][..3];
                if rgb.iter().any(|v| *v > 0.) {
                    lit += 1;
                    if mode == 2 {
                        assert!(rgb[1] > rgb[0] && rgb[1] > rgb[2]);
                    } else {
                        assert!(rgb[0] > rgb[1] && rgb[1] > rgb[2]);
                    }
                }
            }
            assert_eq!(lit, 8, "eight authored brightness levels in one lamp");
        }
    }

    #[test]
    fn workstation_input_monitors_use_cardinal_timeline_column() {
        let view = Viewport {
            width: 1280.,
            height: 720.,
            canvas_scale: 1.,
        };
        let mut motion = super::super::panel_motion::State::default();
        // Independently calculated from the preserved C++ defaults. These
        // absolute pixel references catch accidentally using a panel specimen.
        let closed = preview_layout(view, &motion.snapshot(), Options::default()).unwrap();
        assert_eq!(closed.column_left, 1137.);
        assert_eq!(closed.first_center_y, 118.);
        assert_eq!([closed.clip_top, closed.clip_bottom], [73., 680.]);
        motion.set_all_open(true);
        motion.advance(1.);
        let open = preview_layout(view, &motion.snapshot(), Options::default()).unwrap();
        assert!((open.column_left - 881.).abs() < 1e-6);
        assert!((open.first_center_y - 352.).abs() < 1e-6);
        assert!((open.clip_top - 307.).abs() < 1e-6);
        assert!((open.clip_bottom - 464.).abs() < 1e-6);
        assert!((signal_scale(60.) - 56. / 358.).abs() < 1e-12);
        assert!((signal_scale(180.) - 0.3).abs() < 1e-12);

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let kit = Kit::load(root).unwrap();
        let mut rows = vec![[0.; 4]; 256];
        rows[12][0] = 10.;
        kit.append(&mut rows, view, &motion.snapshot(), Options::default())
            .unwrap();
        let mpp = 1.3 / view.height;
        for i in 0..3 {
            let pose = rows[POSE_BASE + 2 * i];
            let screen_x = pose[0] as f64 / mpp + view.width * 0.5;
            let screen_y = view.height * 0.5 - pose[1] as f64 / mpp;
            assert!((screen_x - (881. + 28.5 + 10. * 56. / 358.)).abs() < 0.0001);
            assert!((screen_y - (352. + i as f64 * 63. + 4. * 56. / 358.)).abs() < 0.0001);
        }
        // Third specimen crosses the bottom content clip. Its pose stays in
        // its row; clipping, rather than compressing the rows, hides the rest.
        let third = rows[POSE_BASE + 4];
        assert!(third[1] < rows[GROUP_BASE][3]);
        assert!(third[1] + kit.max[1] as f32 * third[3] > rows[GROUP_BASE][3]);
        let box_bottom = rows[GROUP_BASE][1] - rows[GROUP_BASE + 1][1];
        assert!((box_bottom - rows[GROUP_BASE][3]).abs() < 0.00001);
    }

    #[test]
    fn workstation_input_monitors_are_independent_of_mixer_and_preserve_headers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let kit = Kit::load(root).unwrap();
        let controls = super::super::decorative_controls::Kit::load(root).unwrap();
        let edison = super::super::edison_demo::Kit::load(root).unwrap();
        for open in [true, false] {
            let mut motion = super::super::panel_motion::State::default();
            motion.set_all_open(true);
            motion.advance(1.);
            motion.set_all_open(open);
            for dt in [0., 0.08, 1.] {
                motion.advance(dt);
                for (width, height) in [(1280., 720.), (390., 844.)] {
                    for canvas_scale in [1., 0.5] {
                        for swap_sides in [false, true] {
                            for swap_horizontal in [false, true] {
                                let view = Viewport {
                                    width,
                                    height,
                                    canvas_scale,
                                };
                                let options = Options {
                                    swap_sides,
                                    swap_horizontal,
                                    ..Options::default()
                                };
                                let snapshot = motion.snapshot();
                                let mut rows =
                                    vec![[0.; 4]; super::super::decorative_controls::BASE];
                                controls
                                    .append(&mut rows, view, &snapshot, options)
                                    .unwrap();
                                rows.resize(engraving::BASE + engraving::ROWS, [0.; 4]);
                                rows[234..238].fill([123.; 4]);
                                let original = rows.clone();
                                kit.append(&mut rows, view, &snapshot, options).unwrap();
                                assert_eq!(rows.len(), original.len(), "immutable geometry");
                                for i in 0..rows.len() {
                                    if i != 12
                                        && !(POSE_BASE..POSE_BASE + 6).contains(&i)
                                        && !(GROUP_BASE..GROUP_BASE + 2).contains(&i)
                                    {
                                        assert_eq!(rows[i], original[i], "unrelated row {i}");
                                    }
                                }
                                let poses = rows[POSE_BASE..POSE_BASE + 6].to_vec();
                                let group = rows[GROUP_BASE..GROUP_BASE + 2].to_vec();
                                for i in 0..3 {
                                    let pose = rows[POSE_BASE + i * 2];
                                    assert_eq!(pose[0], poses[0][0], "one vertical column");
                                    assert_eq!(pose[3], poses[0][3]);
                                    assert!(
                                        (poses[0][1] as f64
                                            - pose[1] as f64
                                            - i as f64 * 63. * 1.3 / height)
                                            .abs()
                                            < 1e-6
                                    );
                                    assert!(pose[2] + kit.min[2] as f32 * pose[3] > 0.);
                                }
                                edison
                                    .append(&mut rows, view, &snapshot, options, &Value::Null)
                                    .unwrap();
                                assert_eq!(
                                    rows[POSE_BASE..POSE_BASE + 6],
                                    poses,
                                    "bottom mixer fitting must never move timeline monitors"
                                );
                                assert_eq!(rows[GROUP_BASE..GROUP_BASE + 2], group);
                                assert_eq!(rows[234..238], [[123.; 4]; 4], "bubble headers");
                            }
                        }
                    }
                }
            }
        }
    }
}
