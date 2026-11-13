//! Authored Edison hardware as Cardinal-style selected-track plugin inserts.
//! Immutable mesh data is uploaded once; only its pose enters changing frames.
use super::{
    input_monitor,
    panel_layout::{Options, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::path::Path;

pub(crate) const BASE: usize = input_monitor::BASE + input_monitor::ROWS;
pub(crate) const ROWS: usize = 32768;
// Keep the old socket and bubble immutable addresses stable.
pub(crate) const PLUG_BASE: usize = super::timeline_scene::BASE + super::timeline_scene::ROWS;
pub(crate) const PLUG_ROWS: usize = 262144;
const POSE: usize = super::engraving::BASE + 448;

const DIRECTORY: &str = "Art/AudioWorkstation/EdisonHardware";

pub(crate) struct Kit {
    prefix: &'static str,
    plugged: Option<Box<Kit>>,
    schema: usize,
    base: usize,
    capacity: usize,
    rows: usize,
    material_count: usize,
    root: usize,
    end: usize,
    node_count: usize,
    triangles: usize,
    min: [f64; 3],
    max: [f64; 3],
    navigation: Option<([usize; 3], [[f64; 3]; 2])>,
    stock:Option<std::sync::Arc<super::stock_eq_gems::Kit>>,
}

impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let mut kit = Self::load_packet(root, "runtime", BASE, ROWS, 4096)?;
        kit.plugged = Some(Box::new(Self::load_packet(
            root,
            "connected",
            PLUG_BASE,
            PLUG_ROWS,
            16384,
        )?));
        kit.navigation = Some(super::timeline_scene::Kit::load(root)?.mini_control());
        kit.stock=Some(super::stock_eq_gems::Kit::load(root)?);
        Ok(kit)
    }
    fn load_packet(
        root: &Path,
        prefix: &'static str,
        base: usize,
        capacity: usize,
        nodes: usize,
    ) -> Result<Self, String> {
        let path = root.join(DIRECTORY).join(format!("{prefix}.json"));

        let data: Value = serde_json::from_slice(
            &std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        let integer = |key: &str| -> Result<usize, String> {
            data[key]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("invalid Edison {key}"))
        };
        let kit = Self {
            prefix,
            plugged: None,
            schema: integer("schema")?,
            base: integer("base")?,
            capacity: integer("capacity")?,
            rows: integer("rows")?,
            material_count: integer("material_count")?,
            root: integer("root")?,
            end: integer("end")?,
            node_count: integer("node_count")?,
            triangles: integer("triangles")?,
            min: serde_json::from_value(data["min"].clone()).map_err(|e| e.to_string())?,
            max: serde_json::from_value(data["max"].clone()).map_err(|e| e.to_string())?,
            navigation: None,
            stock:None,
        };
        // Bound quantities before address arithmetic; the packet has no external
        // buffers, animation, or mutable audio state.
        if kit.schema != 1
            || data["rotation_degrees_xyz"] != serde_json::json!([0, 0, 0])
            || kit.base != base
            || kit.capacity != capacity
            || kit.rows > capacity
            || !(1..256).contains(&kit.material_count)
            || !(1..=nodes).contains(&kit.node_count)
            || kit.triangles > capacity / 6
            || kit.root != base + kit.material_count * 40
            || kit.end != kit.root + kit.node_count * 3
            || kit.end + kit.triangles * 6 != base + kit.rows
            || kit.min.iter().chain(&kit.max).any(|v| !v.is_finite())
            || (0..3).any(|i| kit.min[i] >= kit.max[i])
        {
            return Err("invalid Edison demo packet; re-export EdisonSocket.glb".into());
        }
        Ok(kit)
    }

    pub(crate) fn read_geometry(&self, root: &Path) -> Result<Vec<[f32; 4]>, String> {
        let bytes = std::fs::read(root.join(DIRECTORY).join(format!("{}.bin", self.prefix)))
            .map_err(|e| e.to_string())?;
        if bytes.len() != self.rows * 16 {
            return Err("Edison packet length mismatch".into());
        }
        let rows: Vec<[f32; 4]> = bytes
            .chunks_exact(16)
            .map(|word| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(word[i * 4..i * 4 + 4].try_into().unwrap())
                })
            })
            .collect();
        if rows.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite Edison geometry".into());
        }
        let integer = |v: f32| v >= 0. && v <= (self.base + self.rows) as f32 && v.fract() == 0.;
        for node in (self.root..self.end).step_by(3) {
            let at = node - self.base;
            let (escape, first, count) = (rows[at][3], rows[at + 1][3], rows[at + 2][0]);
            if !integer(escape)
                || !integer(first)
                || !integer(count)
                || count > 4.
                || escape as usize <= node
                || escape as usize > self.end
                || (escape as usize - self.root) % 3 != 0
                || (count == 0. && (escape as usize) < node + 9)
                || (count > 0. && escape as usize != node + 3)
                || (first as usize) < self.end
                || (first as usize - self.end) % 6 != 0
                || first as usize + count as usize * 6 > self.base + self.rows
                || (0..3).any(|i| rows[at][i] > rows[at + 1][i])
            {
                return Err("invalid Edison BVH address or bounds".into());
            }
        }
        for at in (self.end - self.base..self.rows).step_by(6) {
            if !integer(rows[at][3]) || rows[at][3] >= self.material_count as f32 {
                return Err("invalid Edison triangle material".into());
            }
        }
        Ok(rows)
    }

    pub(crate) fn read_connected(&self, root: &Path) -> Result<Vec<[f32; 4]>, String> {
        self.plugged
            .as_ref()
            .ok_or("missing Edison plug")?
            .read_geometry(root)
    }
    pub(crate) fn append(
        &self,
        rows: &mut [[f32; 4]],
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        presentation: &Value,
    ) -> Result<(), String> {
        if rows.len() < POSE + 64 || rows[12][0] != 13. {
            return Err("Edison inserts require the complete workstation frame".into());
        }
        rows[232] = [POSE as f32, 0., 0., 0.];
        rows[POSE..POSE + 64].fill([0.; 4]);
        if presentation["hide_mixer"] == true {for instance in 4..10 {rows[192+2*instance][3]=0.;}}
        if presentation["navigation"] != true && presentation["enabled"] != true {
            return Ok(());
        }
        let inserts = presentation["inserts"]
            .as_array()
            .ok_or("plugin insert list required")?;
        let enabled = presentation["enabled"] == true;
        let scroll = presentation["scroll"]
            .as_f64()
            .filter(|v| v.is_finite())
            .unwrap_or(0.);
        let layout = super::plugin_inserts::layout(view, motion, options)?;
        let pre=super::panel_layout::preamp_rect(view,motion,options)?;
        let unit=1.3/view.height;
        rows[POSE+60]=[((pre.x+pre.width*0.5-view.width*0.5)*unit) as f32,((view.height*0.5-pre.y-pre.height*0.5)*unit) as f32,(pre.width*unit*0.5) as f32,(pre.height*unit*0.5) as f32];

        let slots = if enabled && presentation["slots"].is_array() {
            presentation["slots"].as_array().unwrap().iter().filter_map(|s|Some(super::plugin_inserts::Slot{rect:serde_json::from_value(s["rect"].clone()).ok()?,index:inserts.iter().position(|p|p["id"]==s["id"])?})).take(16).collect()
        } else if enabled { layout.chain_slots(inserts,scroll) } else {Vec::new()};
        // The bottom FX page replaces the unconnected fader specimens. Keep
        // their instance IDs stable for the existing renderer/material ABI.
        if enabled {
            for instance in 4..10 {
                rows[192 + 2 * instance][3] = 0.;
            }
        }
        let mpp = 1.3 / view.height;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        if !enabled {
            for axis in 0..3 {
                lo[axis] = (rows[15][axis] - rows[31][axis]) as f64;
                hi[axis] = (rows[15][axis] + rows[31][axis]) as f64;
            }
        }
        let world = |x: f64, y: f64| [(x - view.width * 0.5) * mpp, (view.height * 0.5 - y) * mpp];
        let clip_lo = world(layout.viewport[0], layout.viewport[1] + layout.viewport[3]);
        let clip_hi = world(layout.viewport[0] + layout.viewport[2], layout.viewport[1]);
        rows[POSE + 40] = [
            clip_lo[0] as f32,
            clip_lo[1] as f32,
            clip_hi[0] as f32,
            clip_hi[1] as f32,
        ];
        let (nav, bounds) = self.navigation.ok_or("missing insert navigation mesh")?;
        let [min, max] = bounds;
        for (i, r) in layout.navigation.iter().enumerate() {
            // Cardinal's lower pair selects tracks on the inserts page only.
            if presentation["navigation"] != true || (!enabled && i >= 2) || !layout.navigation_visible(*r) {
                continue;
            }
            let scale = (r[2] * mpp / (max[0] - min[0])).min(r[3] * mpp / (max[1] - min[1]));
            let center = world(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
            let pressed = presentation["nav_pressed"].as_u64() == Some(i as u64);
            let pose = [
                center[0],
                center[1],
                layout.depth - min[2] * scale + 0.0005 - if pressed { mpp } else { 0. },
            ];
            rows[POSE + 32 + 2 * i] =
                [pose[0] as f32, pose[1] as f32, pose[2] as f32, scale as f32];
            rows[POSE + 33 + 2 * i] = [
                nav[0] as f32,
                nav[1] as f32,
                [60., 62., 123., 125.][i],
                nav[2] as f32,
            ];
            for axis in 0..3 {
                lo[axis] = lo[axis].min(pose[axis] + min[axis] * scale);
                hi[axis] = hi[axis].max(pose[axis] + max[axis] * scale);
            }
        }
        for (i, original) in slots.iter().enumerate() {
            let mut slot=super::plugin_inserts::Slot{rect:original.rect,index:original.index};
            let animated=presentation["slots"].as_array().and_then(|a|a.iter().find(|s|s["id"]==inserts[slot.index]["id"]));
            let lift=animated.and_then(|s|s["lift"].as_f64()).unwrap_or(0.);
            slot.rect[1]-=lift;
            let clip=animated.and_then(|s|serde_json::from_value::<[f64;4]>(s["clip"].clone()).ok()).unwrap_or(layout.viewport);
            let low=world(clip[0],clip[1]+clip[3]);let high=world(clip[0]+clip[2],clip[1]);
            rows[POSE+44+i]=[low[0] as f32,low[1] as f32,high[0] as f32,high[1] as f32];
            let insert=&inserts[slot.index];
            if insert["stock"]==true {
                let kit=self.stock.as_ref().ok_or("missing stock gem geometry")?;
                let v=kit.variant(presentation["selected"]==insert["id"],insert["bypassed"]==true,
                    presentation["stock_bypass_pressed"]==insert["id"],presentation["stock_remove_pressed"]==insert["id"]);
                let scale=slot.rect[2]/230.*mpp;
                let xy=world(slot.rect[0]+slot.rect[2]*0.5,slot.rect[1]+slot.rect[3]*0.5);
                let pose=[xy[0],xy[1],layout.depth-v.min[2]*scale+0.0005+lift*mpp*2.];
                rows[POSE+2*i]=[pose[0]as f32,pose[1]as f32,pose[2]as f32,scale as f32];
                rows[POSE+2*i+1]=[v.root as f32,v.end as f32,0.,v.base as f32];
                for axis in 0..3 {lo[axis]=lo[axis].min(pose[axis]+v.min[axis]*scale);hi[axis]=hi[axis].max(pose[axis]+v.max[axis]*scale);}
                continue;
            }
            let connected = inserts
                .get(slot.index)
                .is_some_and(|p| p["editor_open"] == true);
            let kit = if connected {
                self.plugged.as_ref().unwrap().as_ref()
            } else {
                self
            };
            let hardware = super::plugin_inserts::hardware()?;
            let scale = slot.rect[2] * mpp / (hardware.max[0] - hardware.min[0]);
            let center = [
                slot.rect[0] + slot.rect[2] * 0.5,
                slot.rect[1] + slot.rect[3] * 0.5,
            ];
            // Both poses use the bare housing center; opening an editor never
            // moves the socket or shifts the following insert.
            let pose = [
                (center[0] - view.width * 0.5) * mpp - (self.min[0] + self.max[0]) * 0.5 * scale,
                (view.height * 0.5 - center[1]) * mpp - (self.min[1] + self.max[1]) * 0.5 * scale,
                layout.depth + 0.0005 + lift*mpp*2.
                    - self.min[2] * scale
                    - if presentation["pressed"] == inserts[slot.index]["id"] {
                        mpp
                    } else {
                        0.
                    },
            ];
            rows[POSE + 2 * i] = [pose[0] as f32, pose[1] as f32, pose[2] as f32, scale as f32];
            let insert = &inserts[slot.index];
            let flags = if insert["bypassed"] == true { 4. } else { 0. }
                + if presentation["selected"] == insert["id"] {
                    2.
                } else if presentation["hover"] == insert["id"] {
                    1.
                } else {
                    0.
                };
            rows[POSE + 2 * i + 1] = [kit.root as f32, kit.end as f32, flags, kit.base as f32];
            for axis in 0..3 {
                lo[axis] = lo[axis].min(pose[axis] + kit.min[axis] * scale);
                hi[axis] = hi[axis].max(pose[axis] + kit.max[axis] * scale);
            }
        }
        if lo[0].is_finite() {
            for axis in 0..3 {
                rows[15][axis] = ((lo[axis] + hi[axis]) * 0.5) as f32;
                rows[31][axis] = ((hi[axis] - lo[axis]) * 0.5 + 1e-6) as f32;
            }
        }
        rows[12][0] = (13 + slots.len()) as f32;
        rows[232][1] = slots.len() as f32;
        rows[232][2] = 4.;
        if enabled && let Some(x) = presentation["drop_x"].as_f64().filter(|v| v.is_finite()) {
            let a = world(x, layout.top + layout.size);
            let b = world(x, layout.top);
            rows[POSE + 41] = [a[0] as f32, a[1] as f32, b[1] as f32, (2. * mpp) as f32];
            rows[POSE + 42][0] = (layout.depth + 0.001) as f32;
        }
        Ok(())
    }
}
