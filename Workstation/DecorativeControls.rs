//! Authored control models. Transport interaction belongs to the layer-five controller;
//! live transport poses retain the original authored geometry and materials.
//! Packed BVHs retain the GLB component triangles and material boundaries.
use super::{
    control_engraving, floor_light,
    panel_layout::{self, Options, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::{io::{Read, Seek, SeekFrom}, path::Path};

pub(crate) const BASE: usize = 256 + floor_light::ROWS;
pub(crate) const ROWS: usize = 32768;
// A maximum of 129 leaves needs 257 two-row nodes. Keep the frame size and
// every authored geometry/material address stable. Larger future packets may
// use this tail themselves; those retain the linear shader traversal.
const INSTANCE_ROWS: usize = 516;
const INSTANCE_BASE: usize = BASE + ROWS - INSTANCE_ROWS;
const MAX_INSTANCES: usize = 129;
const _: () = assert!(super::track_io_scene::END <= INSTANCE_BASE);
const _: () = assert!(INSTANCE_BASE + INSTANCE_ROWS <= super::transport_pose::META_BASE);

#[derive(Clone, Copy, Debug)]
struct Bounds { lo: [f32; 3], hi: [f32; 3] }
impl Bounds {
    fn union(self, other: Self) -> Self {
        Self {
            lo: std::array::from_fn(|i| self.lo[i].min(other.lo[i])),
            hi: std::array::from_fn(|i| self.hi[i].max(other.hi[i])),
        }
    }
    fn area(self) -> f32 {
        let d: [f32; 3] = std::array::from_fn(|i| self.hi[i] - self.lo[i]);
        d[0]*d[1] + d[1]*d[2] + d[2]*d[0]
    }
    fn placed(self, pose: [f32; 4]) -> Self {
        // Enclose the original shader's padded local root box, including
        // float32 transform cancellation. This only admits extra candidates;
        // the unchanged local-space triangle test decides all actual hits.
        let center: [f32; 3] = std::array::from_fn(|i| (self.lo[i]+self.hi[i])*0.5);
        let extent: [f32; 3] = std::array::from_fn(|i| (self.hi[i]-self.lo[i])*0.5+1e-7);
        let lo: [f32; 3] = std::array::from_fn(|i| pose[i]+(center[i]-extent[i])*pose[3]);
        let hi: [f32; 3] = std::array::from_fn(|i| pose[i]+(center[i]+extent[i])*pose[3]);
        let guard: [f32; 3] = std::array::from_fn(|i| 4e-6*(1.+lo[i].abs().max(hi[i].abs())+pose[3]));
        Self { lo: std::array::from_fn(|i| lo[i]-guard[i]), hi: std::array::from_fn(|i| hi[i]+guard[i]) }
    }
}

/// Small CPU-owned instance hierarchy. Its boxes come from the actual mesh BVH
/// roots, not decorative silhouettes; geometry is loaded only at session entry.
/// Call append after all controls, lane monitors and Edison poses are final.
pub(crate) struct InstanceBounds {
    roots: Vec<(usize, Bounds)>,
    available: bool,
}
impl InstanceBounds {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let mut result = Self { roots: Vec::new(), available: true };
        for (directory, stem) in [
            ("DecorativeControls", "controls"), ("InputMonitor", "runtime"),
            ("EdisonHardware", "runtime"), ("EdisonHardware", "connected"),
            ("Timeline", "runtime"), ("CrosswalkArming", "runtime"),
        ] {
            let path = root.join("Art/AudioWorkstation").join(directory);
            let manifest = path.join(format!("{stem}.json"));
            let document: Value = serde_json::from_slice(&std::fs::read(&manifest)
                .map_err(|e| format!("{}: {e}", manifest.display()))?).map_err(|e| e.to_string())?;
            let number = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| "invalid control instance bounds manifest".to_owned());
            let base = number(&document["base"])?;
            let used = number(&document["rows"])?;
            if directory == "DecorativeControls" { result.available = used <= ROWS-1024; }
            let geometry = path.join(format!("{stem}.bin"));
            let mut file = std::fs::File::open(&geometry).map_err(|e| format!("{}: {e}", geometry.display()))?;
            let assets: Vec<&Value> = match document["assets"].as_array() {
                Some(assets) => assets.iter().collect(), None => vec![&document],
            };
            for asset in assets {
                let address = number(&asset["root"])?;
                let offset = address.checked_sub(base).filter(|&n| n.checked_add(2).is_some_and(|end| end <= used))
                    .ok_or("invalid control instance root address")?;
                let mut bytes = [0u8; 32];
                file.seek(SeekFrom::Start((offset as u64)*16)).map_err(|e| e.to_string())?;
                file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                let word = |i: usize| f32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap());
                let bounds = Bounds { lo: std::array::from_fn(|i| word(i)), hi: std::array::from_fn(|i| word(i+4)) };
                if (0..3).any(|i| !bounds.lo[i].is_finite() || !bounds.hi[i].is_finite() || bounds.lo[i] > bounds.hi[i]) {
                    return Err("invalid control instance root bounds".into());
                }
                result.roots.push((address, bounds));
            }
        }
        let stock=super::stock_eq_gems::Kit::load(root)?;
        for v in &stock.variants {result.roots.push((v.root,Bounds{lo:v.min.map(|x|x as f32),hi:v.max.map(|x|x as f32)}));}
        Ok(result)
    }

    pub(crate) fn append(&self, rows: &mut [[f32; 4]]) -> Result<(), String> {
        if rows.len() < super::engraving::BASE + super::engraving::ROWS {
            return Err("control instance bounds need a complete workstation frame".into());
        }
        rows[12][2] = 0.; rows[12][3] = 0.;
        if !self.available { return Ok(()); }
        let packet = build_instance_packet(rows, |root| {
            self.roots.iter().find(|(address, _)| *address as f32 == root).map(|(_, bounds)| *bounds)
        })?;
        debug_assert!(packet.len() <= INSTANCE_ROWS);
        rows[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS].fill([0.; 4]);
        rows[INSTANCE_BASE..INSTANCE_BASE+packet.len()].copy_from_slice(&packet);
        if !packet.is_empty() {
            rows[12][2] = INSTANCE_BASE as f32;
            rows[12][3] = (INSTANCE_BASE+packet.len()) as f32;
        }
        Ok(())
    }
}

// One implementation supplies both the live hierarchy and its retained-image
// dependency proof. A changed acceleration packet is never ignored unchecked.
fn build_instance_packet(rows: &[[f32; 4]], mut lookup: impl FnMut(f32) -> Option<Bounds>) -> Result<Vec<[f32; 4]>, String> {
        let mut entries = Vec::with_capacity(MAX_INSTANCES);
        for instance in 0..MAX_INSTANCES {
            let active = instance_active(rows,instance);
            if !active { continue; }
            let at = control_pose_row(rows, instance);
            let pose = *rows.get(at).ok_or("invalid control instance pose address")?;
            let tree = *rows.get(at+1).ok_or("invalid control instance tree address")?;
            if pose[3] <= 0. { continue; }
            if !pose.iter().all(|v| v.is_finite()) { return Err("nonfinite control instance pose".into()); }
            let bounds = lookup(tree[0]).ok_or("unknown control instance geometry root")?;
            entries.push((instance, bounds.placed(pose)));
        }
        let mut packet = Vec::with_capacity(INSTANCE_ROWS);
        if !entries.is_empty() { pack_instance_tree(&entries, &mut packet); }
        Ok(packet)
}

// A Host frame contains the dynamic prefix only. InputMonitor/Edison/Timeline
// mesh roots live in the renderer's separately loaded immutable suffix. Validate
// every available local root, and reconstruct the entire ordered hierarchy from
// its leaves. Pairwise retention additionally proves every non-button leaf and
// its pose/descriptor unchanged; absent static roots are never blindly exempted.
fn validated_instance_entries(rows: &[[f32; 4]]) -> Option<Vec<(usize, Bounds)>> {
    if rows.len() < INSTANCE_BASE + INSTANCE_ROWS || rows[12][2] != INSTANCE_BASE as f32 { return None; }
    let end = rows[12][3];
    if !end.is_finite() || end.fract()!=0. || end<=INSTANCE_BASE as f32 || end>(INSTANCE_BASE+INSTANCE_ROWS) as f32 { return None; }
    let end=end as usize;if (end-INSTANCE_BASE)%2!=0 { return None; }
    let mut leaves=[None;MAX_INSTANCES];let mut leaf_count=0;
    for at in (INSTANCE_BASE..end).step_by(2) {
        let lo=rows[at];let hi=rows[at+1];let instance=hi[3];
        if (0..3).any(|i| !lo[i].is_finite() || !hi[i].is_finite() || lo[i]>hi[i]) { return None; }
        if instance == -1. { continue; }
        if !instance.is_finite() || instance.fract()!=0. || !(0.0..MAX_INSTANCES as f32).contains(&instance) { return None; }
        let instance=instance as usize;if leaves[instance].is_some() { return None; }
        leaves[instance]=Some(Bounds {lo:[lo[0],lo[1],lo[2]],hi:[hi[0],hi[1],hi[2]]});leaf_count+=1;
    }
    let mut entries=Vec::with_capacity(MAX_INSTANCES);
    for instance in 0..MAX_INSTANCES {
        let active=instance_active(rows,instance);
        if !active { continue; }
        let at=control_pose_row(rows,instance);let pose=*rows.get(at)?;let tree=*rows.get(at+1)?;
        if !pose.iter().all(|v| v.is_finite()) { return None; }if pose[3]<=0. { continue; }
        let bounds=leaves[instance]?;
        if !tree[0].is_finite() || tree[0]<256. || tree[0].fract()!=0. { return None; }
        let root=tree[0] as usize;
        if (INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS).contains(&root) { return None; }
        if root<rows.len() {
            let low=rows.get(root)?;let high=rows.get(root.checked_add(1)?)?;
            if (0..3).any(|i| !low[i].is_finite() || !high[i].is_finite() || low[i]>high[i]) { return None; }
            let expected=Bounds {lo:[low[0],low[1],low[2]],hi:[high[0],high[1],high[2]]}.placed(pose);
            if !same_bounds(expected,bounds) { return None; }
        } else if instance<4 { return None; }
        entries.push((instance,bounds));
    }
    if entries.is_empty() || entries.len()!=leaf_count { return None; }
    let mut packet=Vec::with_capacity(INSTANCE_ROWS);pack_instance_tree(&entries,&mut packet);
    if packet.len()!=end-INSTANCE_BASE || !packet.iter().zip(&rows[INSTANCE_BASE..end]).all(|(a,b)| a.map(f32::to_bits)==b.map(f32::to_bits)) { return None; }
    if rows[end..INSTANCE_BASE+INSTANCE_ROWS].iter().any(|r|r.map(f32::to_bits)!=[0;4]) { return None; }
    Some(entries)
}
fn same_bounds(a:Bounds,b:Bounds)->bool {a.lo.map(f32::to_bits)==b.lo.map(f32::to_bits)&&a.hi.map(f32::to_bits)==b.hi.map(f32::to_bits)}

pub(crate) fn derived_instance_range(rows: &[[f32;4]]) -> Option<std::ops::Range<usize>> {
    validated_instance_entries(rows).map(|_|INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS)
}

pub(crate) fn derived_instance_motion_range(a:&[[f32;4]],b:&[[f32;4]]) -> Option<std::ops::Range<usize>> {
    let x=validated_instance_entries(a)?;let y=validated_instance_entries(b)?;
    if x.len()!=y.len() { return None; }
    for ((i,old),(j,new)) in x.into_iter().zip(y) {
        if i!=j { return None; }
        if i>=4 {
            let pa=control_pose_row(a,i);let pb=control_pose_row(b,i);
            if !same_bounds(old,new) || a[pa].map(f32::to_bits)!=b[pb].map(f32::to_bits)
                || a[pa+1].map(f32::to_bits)!=b[pb+1].map(f32::to_bits) { return None; }
        }
    }
    Some(INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS)
}

// Live construction and retained-packet validation must enumerate the same
// active instances, including the independent page-zero I/O allocation.
fn instance_active(rows:&[[f32;4]],instance:usize)->bool {
    if instance>=125 {
        ((instance-125) as f32)<rows[super::keyboard_toggle::HEADER][1]
    } else if instance>=super::track_io_scene::FIRST_INSTANCE {
        ((instance-super::track_io_scene::FIRST_INSTANCE) as f32)<rows[super::track_io_scene::HEADER][1]
    } else if instance>=61 {((instance-61) as f32)<rows[232][2]}
    else if instance>=29 {((instance-29) as f32)<rows[247][1]}
    else {(instance as f32)<rows[12][0]}
}
fn control_pose_row(rows: &[[f32; 4]], instance: usize) -> usize {
    if instance >= 125 { super::keyboard_toggle::POSE_BASE + super::keyboard_toggle::STRIDE*(instance-125) }
    else if instance >= 65 { super::track_io_scene::POSE_BASE + super::track_io_scene::STRIDE*(instance-65) }
    else if instance >= 61 { rows[232][0] as usize + 32 + 2*(instance-61) }
    else if instance >= 29 { rows[247][0] as usize + 2*(instance-29) }
    else if instance >= 13 { rows[232][0] as usize + 2*(instance-13) }
    else if instance >= 10 { 226 + 2*(instance-10) }
    else { 192 + 2*instance }
}

fn pack_instance_tree(entries: &[(usize, Bounds)], packet: &mut Vec<[f32; 4]>) {
    let at = packet.len();
    packet.extend_from_slice(&[[0.; 4]; 2]);
    let bounds = entries.iter().skip(1).fold(entries[0].1, |b, e| b.union(e.1));
    let mut instance = -1.;
    if entries.len() == 1 { instance = entries[0].0 as f32; }
    else {
        // Ordered surface-area splits reduce overlap while retaining original
        // instance/triangle tie ownership. No per-ray stack or sorting needed.
        let mut suffix = vec![bounds; entries.len()];
        suffix[entries.len()-1] = entries[entries.len()-1].1;
        for i in (0..entries.len()-1).rev() { suffix[i] = suffix[i+1].union(entries[i].1); }
        let mut left = entries[0].1;
        let mut split = 1; let mut cost = f32::INFINITY;
        for i in 1..entries.len() {
            let candidate = left.area()*i as f32 + suffix[i].area()*(entries.len()-i) as f32;
            if candidate < cost { cost = candidate; split = i; }
            left = left.union(entries[i].1);
        }
        pack_instance_tree(&entries[..split], packet);
        pack_instance_tree(&entries[split..], packet);
    }
    packet[at] = [bounds.lo[0], bounds.lo[1], bounds.lo[2], (INSTANCE_BASE+packet.len()) as f32];
    packet[at+1] = [bounds.hi[0], bounds.hi[1], bounds.hi[2], instance];
}
// Bottom-panel meter/fader specimens; timeline monitors have their own layout.
pub(crate) const MIXER_WIDTH: f64 = 0.30;
pub(crate) const MIXER_HEIGHT: f64 = 0.18;
struct Asset {
    name: String,
    root: usize,
    end: usize,
    triangles: usize,
    band_acceleration: usize,
    min: [f64; 3],
    max: [f64; 3],
}
struct Manifest {
    schema: usize,
    base: usize,
    capacity: usize,
    rows: usize,
    materials: usize,
    assets: Vec<Asset>,
}
pub(crate) struct Kit {
    manifest: Manifest,
    rows: Vec<[f32; 4]>,
    engraving_rows: usize,
}
impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("Art/AudioWorkstation/DecorativeControls");
        let document: Value = serde_json::from_slice(
            &std::fs::read(path.join("controls.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let integer = |v: &Value| -> Result<usize, String> {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| "invalid control manifest integer".into())
        };
        let bounds = |v: &Value| -> Result<[f64; 3], String> {
            serde_json::from_value(v.clone()).map_err(|e| e.to_string())
        };
        let mut assets = Vec::new();
        for asset in document["assets"]
            .as_array()
            .ok_or("missing control assets")?
        {
            assets.push(Asset {
                name: asset["name"]
                    .as_str()
                    .ok_or("missing control name")?
                    .to_owned(),
                root: integer(&asset["root"])?,
                end: integer(&asset["end"])?,
                triangles: integer(&asset["triangles"])?,
                band_acceleration: asset
                    .get("band_acceleration")
                    .map(&integer)
                    .transpose()?
                    .unwrap_or(0),
                min: bounds(&asset["min"])?,
                max: bounds(&asset["max"])?,
            });
        }
        let manifest = Manifest {
            schema: integer(&document["schema"])?,
            base: integer(&document["base"])?,
            capacity: integer(&document["capacity"])?,
            rows: integer(&document["rows"])?,
            materials: integer(&document["materials"])?,
            assets,
        };
        let bytes = std::fs::read(path.join("controls.bin")).map_err(|e| e.to_string())?;
        if manifest.schema != 2
            || manifest.base != BASE
            || manifest.capacity != ROWS
            || manifest.rows > ROWS
            || manifest.materials >= 256
            || bytes.len() != manifest.rows * 16
            || manifest.assets.len() != 6
        {
            return Err("invalid decorative control geometry package".into());
        }
        let mut rows = vec![[0.; 4]; ROWS];
        for (row, bytes) in rows.iter_mut().zip(bytes.chunks_exact(16)) {
            for (v, word) in row.iter_mut().zip(bytes.chunks_exact(4)) {
                *v = f32::from_le_bytes(word.try_into().unwrap());
                if !v.is_finite() {
                    return Err("nonfinite decorative control mesh".into());
                }
            }
        }
        for (asset, name) in manifest.assets.iter().zip([
            "GemButton_Rewind",
            "GemButton_Stop",
            "GemButton_Record",
            "GemButton_Play",
            "GemFader_Ivory",
            "GemMeter_Ivory",
        ]) {
            if asset.name != name
                || asset.root < BASE + manifest.materials * 40
                || asset.end <= asset.root
                || asset.end > BASE + manifest.rows
                || asset.triangles > manifest.rows / 6
                || asset.end + asset.triangles * 6 > BASE + manifest.rows
                || asset.min.iter().chain(&asset.max).any(|v| !v.is_finite())
            {
                return Err("invalid decorative control model bounds".into());
            }
            if asset.band_acceleration != 0 {
                validate_band_acceleration(asset, &rows, manifest.rows)?;
            }
        }
        let engraving = control_engraving::Kit::load(root)?;
        if document["control_engraving_manifest_sha256"].as_str() != Some(engraving.manifest_sha256()) {
            return Err("decorative controls and engraving manifest were exported from different assets".into());
        }
        engraving.validate_controls(&rows, manifest.rows, manifest.materials)?;
        Ok(Self { manifest, rows, engraving_rows: engraving.used_rows() })
    }
    pub(crate) fn append(
        &self,
        rows: &mut Vec<[f32; 4]>,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<(), String> {
        if rows.len() != BASE {
            return Err("invalid decorative control frame offset".into());
        }
        let rects = panel_layout::layout(view, motion, options)?;
        let mpp = 1.3 / view.height;
        let mut instances = Vec::new();
        // Cardinal transport order: rewind, stop, record, play, across the top.
        // Fit uniformly inside the table; never stretch a gem or its glyph.
        for (panel, design_width, design_height) in
            [(0, 0.205, 0.04), (1, MIXER_WIDTH, MIXER_HEIGHT)]
        {
            let rect = rects[panel];
            let surface = panel_layout::resolved_surface(view, motion, options, panel)?;
            let safe = surface.size;
            let scale = (safe[0] * 0.88 / design_width).min(safe[1] * 0.80 / design_height);
            let center = rect.center();
            let origin = [surface.origin[0], surface.origin[1]];
            if panel == 0 {
                for i in 0..4 {
                    instances.push((i, origin, (i as f64 - 1.5) * 0.055, scale));
                }
            } else {
                for i in 0..3 {
                    let left = -0.15 + i as f64 * 0.11;
                    instances.push((5, origin, left + 0.015, scale));
                    instances.push((4, origin, left + 0.060, scale));
                }
            }
        }
        rows[12] = [instances.len() as f32, BASE as f32, 0., 0.];
        rows[control_engraving::HEADER] = [control_engraving::BASE as f32, self.engraving_rows as f32, 0., 0.];
        for (i, (asset_index, center, x, scale)) in instances.iter().enumerate() {
            let asset = &self.manifest.assets[*asset_index];
            let at = 192 + 2 * i;
            rows[at] = [
                (center[0] + x * scale) as f32,
                center[1] as f32,
                (-asset.min[2] * scale + 0.0005) as f32,
                *scale as f32,
            ];
            rows[at + 1] = [
                asset.root as f32,
                asset.end as f32,
                asset.band_acceleration as f32,
                0.,
            ];
        }
        // Cull entire panel-mounted groups before traversing individual models.
        for (first, end, center_row, extent_row) in [(0, 4, 13, 14), (4, 10, 15, 31)] {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for i in first..end {
                let (asset_index, _, _, scale) = instances[i];
                let asset = &self.manifest.assets[asset_index];
                let pose = rows[192 + 2 * i];
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(pose[axis] as f64 + asset.min[axis] * scale);
                    hi[axis] = hi[axis].max(pose[axis] as f64 + asset.max[axis] * scale);
                }
            }
            for axis in 0..3 {
                rows[center_row][axis] = ((lo[axis] + hi[axis]) * 0.5) as f32;
                rows[extent_row][axis] = ((hi[axis] - lo[axis]) * 0.5 + 1e-6) as f32;
            }
        }
        rows.extend_from_slice(&self.rows);
        Ok(())
    }
}

fn validate_band_acceleration(asset: &Asset, rows: &[[f32; 4]], used: usize) -> Result<(), String> {
    let invalid = || "invalid decorative control band acceleration".to_owned();
    let index = |value: f32| -> Result<usize, String> {
        if value < 0. || value.fract() != 0. || value > (BASE + used) as f32 {
            return Err(invalid());
        }
        Ok(value as usize)
    };
    let row = |address: usize| -> Result<&[f32; 4], String> {
        address
            .checked_sub(BASE)
            .filter(|&at| at < used)
            .and_then(|at| rows.get(at))
            .ok_or_else(invalid)
    };
    let original = |address: usize| -> Result<(), String> {
        if address < asset.end
            || address >= asset.end + asset.triangles * 6
            || (address - asset.end) % 6 != 0
        {
            return Err(invalid());
        }
        Ok(())
    };
    let descriptor = row(asset.band_acceleration)?;
    let root = index(descriptor[0])?;
    let end = index(descriptor[1])?;
    let channels = index(descriptor[2])?;
    if descriptor[3] != 2.
        || root >= end
        || (end - root) % 3 != 0
        || channels != asset.band_acceleration + 1
    {
        return Err(invalid());
    }
    row(root)?;
    row(end - 1)?;
    if index(row(root)?[3])? != end {
        return Err(invalid());
    }
    for node in (root..end).step_by(3) {
        let lo = row(node)?;
        let hi = row(node + 1)?;
        let count = index(row(node + 2)?[0])?;
        let escape = index(lo[3])?;
        if escape <= node
            || escape > end
            || (escape - root) % 3 != 0
            || count > 4
            || (count == 0 && escape < node + 9)
            || (count > 0 && escape != node + 3)
            || (0..3).any(|axis| lo[axis] > hi[axis])
        {
            return Err(invalid());
        }
        let first = index(hi[3])?;
        if count > 0 && (first < end || first + count * 6 > asset.band_acceleration) {
            return Err(invalid());
        }
        for triangle in 0..count {
            let at = first + 6 * triangle;
            let rank = index(row(at + 2)?[3])?;
            original(rank)?;
            for word in 0..6 {
                let actual = row(at + word)?;
                let source = row(rank + word)?;
                if (word == 2 && actual[..3] != source[..3]) || (word != 2 && actual != source) {
                    return Err(invalid());
                }
            }
        }
    }
    for channel in 0..2 {
        let lo = row(channels + 2 * channel)?;
        let hi = row(channels + 2 * channel + 1)?;
        let count = index(lo[3])?;
        let records = index(hi[3])?;
        if !(2..=129).contains(&count)
            || records < channels + 4
            || (0..3).any(|axis| lo[axis] >= hi[axis])
        {
            return Err(invalid());
        }
        let mut previous = lo[1];
        for band in 0..count {
            let at = records + 4 * band;
            let range = row(at)?;
            if range[0] != previous || range[0] >= range[1] || range[1] > hi[1] {
                return Err(invalid());
            }
            previous = range[1];
            let mut previous_pointer = 0;
            for group in 1..=3 {
                for &pointer in row(at + group)? {
                    let pointer = index(pointer)?;
                    original(pointer)?;
                    if pointer <= previous_pointer {
                        return Err(invalid());
                    }
                    previous_pointer = pointer;
                }
            }
        }
        if previous != hi[1] {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance_frame() -> Vec<[f32; 4]> {
        let mut rows = vec![[0.; 4]; super::super::engraving::BASE + super::super::engraving::ROWS];
        rows[12][0] = 29.;
        rows[232] = [(super::super::engraving::BASE+448) as f32, 16., 4., 0.];
        rows[247] = [(super::super::engraving::BASE+368) as f32, 32., 0., 0.];
        rows[super::super::keyboard_toggle::HEADER]=[super::super::keyboard_toggle::POSE_BASE as f32,4.,0.,0.];
        rows[super::super::track_io_scene::HEADER]=[super::super::track_io_scene::POSE_BASE as f32,60.,0.,0.];
        rows
    }

    // Float32 mirror of gemBox: comparisons test the accelerator boundary, not
    // a looser double-precision ray/box oracle. Narrowphase remains unchanged.
    fn box_hit(origin: [f32; 3], ray: [f32; 3], bounds: Bounds, padding: f32, far: f32) -> bool {
        let mut near = 0f32; let mut end = far;
        for axis in 0..3 {
            let center = (bounds.lo[axis]+bounds.hi[axis])*0.5;
            let extent = (bounds.hi[axis]-bounds.lo[axis])*0.5+padding;
            let inverse = (if ray[axis] < 0. { -1. } else { 1. })/ray[axis].abs().max(1e-8);
            let a = (center-extent-origin[axis])*inverse;
            let b = (center+extent-origin[axis])*inverse;
            near = near.max(a.min(b)); end = end.min(a.max(b));
        }
        end >= near
    }

    fn candidates(rows: &[[f32; 4]], origin: [f32; 3], ray: [f32; 3], far: f32) -> Vec<usize> {
        let mut node = rows[12][2] as usize;
        let end = rows[12][3] as usize;
        let mut result = Vec::new(); let mut visits = 0;
        while node < end {
            visits += 1; assert!(visits <= 257);
            let lo = rows[node]; let hi = rows[node+1];
            let bounds = Bounds { lo: [lo[0],lo[1],lo[2]], hi: [hi[0],hi[1],hi[2]] };
            assert!(lo[3] as usize > node && lo[3] as usize <= end);
            if box_hit(origin, ray, bounds, 0., far) {
                node += 2;
                if hi[3] >= 0. { result.push(hi[3] as usize); }
            } else { node = lo[3] as usize; }
        }
        assert!(result.windows(2).all(|v| v[0] < v[1]), "original hit/tie order");
        result
    }

    #[test]
    fn workstation_control_instance_tree_capacity_hidden_scales_and_legacy_fallback() {
        let bounds = Bounds { lo: [-0.03,-0.09,-0.023], hi: [0.03,0.09,0.016] };
        let kit = InstanceBounds { roots: vec![(1234, bounds)], available: true };
        let mut rows = instance_frame();
        for instance in 0..MAX_INSTANCES {
            let at = control_pose_row(&rows, instance);
            rows[at] = [0.,0.,0.,1.]; rows[at+1][0] = 1234.;
        }
        let before = rows.clone();
        kit.append(&mut rows).unwrap();
        assert_eq!(rows[12][3]-rows[12][2], 514.);
        assert_eq!(candidates(&rows, [0.,0.,1.], [0.,0.,-1.], 10.), (0..129).collect::<Vec<_>>());
        for i in 0..rows.len() {
            if i == 12 { assert_eq!(&rows[i][..2], &before[i][..2]); }
            else if !(INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS).contains(&i) { assert_eq!(rows[i], before[i]); }
        }
        for (instance, scale) in [(1,0.), (2,-1.)] {
            let at = control_pose_row(&rows, instance);
            rows[at][3] = scale;
            rows[at+1][0] = -1.; // An inactive scale never accesses its mesh.
        }
        rows[12][0] = 4.; rows[232][2] = 0.; rows[247][1] = 0.; rows[super::super::track_io_scene::HEADER][1]=0.; rows[super::super::keyboard_toggle::HEADER][1]=0.;
        kit.append(&mut rows).unwrap();
        assert_eq!(candidates(&rows, [0.,0.,1.], [0.,0.,-1.], 10.), vec![0,3]);
        rows[12][0] = 0.;
        kit.append(&mut rows).unwrap();
        assert_eq!(&rows[12][2..], &[0.,0.]);
        let legacy = InstanceBounds { roots: vec![], available: false };
        rows[12][2] = 7.; rows[12][3] = 9.;
        rows[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS].fill([42.;4]);
        legacy.append(&mut rows).unwrap();
        assert_eq!(&rows[12][2..], &[0.,0.]);
        assert!(rows[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS].iter().all(|r| *r == [42.;4]));
    }

    #[test]
    fn workstation_transport_motion_validates_derived_bvh_without_hiding_other_geometry() {
        let bounds=Bounds {lo:[-0.02,-0.02,-0.023],hi:[0.02,0.02,0.]};
        let kit=InstanceBounds {roots:vec![(1234,bounds)],available:true};
        let mut original=instance_frame();original[12][0]=4.;original[232][2]=0.;original[247][1]=0.;
        original[0][2]=2.;original[1]=[0.,0.,3.,0.];original[2]=[1.,0.,0.,1.];original[3]=[0.,1.,0.,1.];original[4]=[0.,0.,-1.,0.];original[8]=[1200.,780.,0.,0.];
        original[13]=[0.,0.4,0.012,0.];original[14]=[0.2,0.02,0.012,0.];
        original[1234]=[-0.02,-0.02,-0.023,1240.];original[1235]=[0.02,0.02,0.,0.];
        for i in 0..4 {original[192+i*2]=[(i as f32-1.5)*0.055,0.4,0.0235,1.];original[193+i*2]=[1234.,1240.,0.,0.];}
        let mut neutral=original.clone();super::super::transport_pose::apply(&mut neutral,[0.;4],1.3/780.).unwrap();kit.append(&mut neutral).unwrap();
        let mut pressed=original;super::super::transport_pose::apply(&mut pressed,[0.5,0.25,0.,0.],1.3/780.).unwrap();kit.append(&mut pressed).unwrap();
        assert!(derived_instance_range(&neutral).is_some());assert!(derived_instance_range(&pressed).is_some());
        assert_ne!(neutral[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS],pressed[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS]);
        assert!(!super::super::transport_pose::optics_changed(&neutral,&pressed,true));
        assert!(super::super::transport_pose::optics_changed(&neutral,&pressed,false));
        let mut corrupted=pressed.clone();corrupted[INSTANCE_BASE][2]+=0.001;
        assert!(derived_instance_range(&corrupted).is_none());assert!(super::super::transport_pose::optics_changed(&neutral,&corrupted,true));
        corrupted=pressed.clone();corrupted[1234][0]-=0.001;
        assert!(super::super::transport_pose::optics_changed(&neutral,&corrupted,true));
        corrupted=pressed;corrupted[192][0]+=0.001;kit.append(&mut corrupted).unwrap();
        assert!(derived_instance_range(&corrupted).is_some());assert!(super::super::transport_pose::optics_changed(&neutral,&corrupted,true));
    }

    #[test]
    fn transport_bvh_validates_external_static_roots_and_rejects_changed_external_leaves() {
        let bounds=Bounds {lo:[-0.02,-0.02,-0.023],hi:[0.02,0.02,0.]};
        let external=1_276_966; // Real input-monitor root is outside the Host frame.
        let kit=InstanceBounds {roots:vec![(1234,bounds),(external,bounds)],available:true};
        let mut original=instance_frame();original[12][0]=5.;original[232][2]=0.;original[247][1]=0.;
        original[0][2]=2.;original[1]=[0.,0.,3.,0.];original[2]=[1.,0.,0.,1.];original[3]=[0.,1.,0.,1.];original[4]=[0.,0.,-1.,0.];original[8]=[1200.,780.,0.,0.];
        original[1234]=[-0.02,-0.02,-0.023,1240.];original[1235]=[0.02,0.02,0.,0.];
        for i in 0..5 {original[192+2*i]=[i as f32*0.055,0.4,0.0235,1.];original[193+2*i]=[if i<4 {1234.} else {external as f32},1240.,0.,0.];}
        assert!(external>=original.len());
        let mut neutral=original.clone();super::super::transport_pose::apply(&mut neutral,[0.;4],1.3/780.).unwrap();kit.append(&mut neutral).unwrap();
        let mut down=original;super::super::transport_pose::apply(&mut down,[0.5,0.25,0.,0.],1.3/780.).unwrap();kit.append(&mut down).unwrap();
        assert!(derived_instance_range(&neutral).is_some());assert!(derived_instance_range(&down).is_some());
        assert!(derived_instance_motion_range(&neutral,&down).is_some());assert!(!super::super::transport_pose::optics_changed(&neutral,&down,true));
        // Rebuild an internally consistent but altered external leaf tree.
        // Single-frame structure is valid; pairwise motion proof must reject it.
        let mut changed=down.clone();let mut entries=validated_instance_entries(&changed).unwrap();entries.last_mut().unwrap().1.lo[0]-=0.001;
        let mut packet=Vec::new();pack_instance_tree(&entries,&mut packet);
        changed[INSTANCE_BASE..INSTANCE_BASE+INSTANCE_ROWS].fill([0.;4]);changed[INSTANCE_BASE..INSTANCE_BASE+packet.len()].copy_from_slice(&packet);changed[12][3]=(INSTANCE_BASE+packet.len()) as f32;
        assert!(derived_instance_range(&changed).is_some());assert!(derived_instance_motion_range(&neutral,&changed).is_none());assert!(super::super::transport_pose::optics_changed(&neutral,&changed,true));
        changed=down.clone();changed[200][1]+=0.001;kit.append(&mut changed).unwrap();assert!(derived_instance_motion_range(&neutral,&changed).is_none());
        changed=down;changed[201][2]=1.;assert!(derived_instance_motion_range(&neutral,&changed).is_none());
    }

    #[test]
    fn io_leaves_participate_in_transport_retention_proof_without_touching_pose_metadata() {
        let bounds=Bounds{lo:[-0.02,-0.02,-0.023],hi:[0.02,0.02,0.]};
        let external=super::super::track_io_scene::BASE;
        let kit=InstanceBounds{roots:vec![(1234,bounds),(external,bounds)],available:true};
        let mut original=instance_frame();original[12][0]=4.;original[232][2]=0.;original[247][1]=0.;
        original[super::super::track_io_scene::HEADER][1]=1.;
        original[0][2]=2.;original[1]=[0.,0.,3.,0.];original[2]=[1.,0.,0.,1.];original[3]=[0.,1.,0.,1.];original[4]=[0.,0.,-1.,0.];original[8]=[1200.,780.,0.,0.];
        original[1234]=[-0.02,-0.02,-0.023,1240.];original[1235]=[0.02,0.02,0.,0.];
        for i in 0..4{original[192+2*i]=[i as f32*0.055,0.4,0.0235,1.];original[193+2*i]=[1234.,1240.,0.,0.];}
        let io=super::super::track_io_scene::POSE_BASE;original[io]=[0.6,0.,0.0235,1.];original[io+1]=[external as f32,(external+6) as f32,0.,0.];
        let mut neutral=original.clone();super::super::transport_pose::apply(&mut neutral,[0.;4],1.3/780.).unwrap();kit.append(&mut neutral).unwrap();
        let mut down=original;super::super::transport_pose::apply(&mut down,[0.5,0.,0.,0.],1.3/780.).unwrap();
        let meta=super::super::transport_pose::META_BASE;let retained=down[meta..meta+6].to_vec();kit.append(&mut down).unwrap();assert_eq!(&down[meta..meta+6],retained.as_slice());
        assert_eq!(validated_instance_entries(&down).unwrap().last().unwrap().0,65);
        assert!(derived_instance_motion_range(&neutral,&down).is_some());assert!(!super::super::transport_pose::optics_changed(&neutral,&down,true));
        down[io][0]+=0.001;kit.append(&mut down).unwrap();assert!(derived_instance_motion_range(&neutral,&down).is_none());
    }

    #[test]
    fn workstation_control_instance_tree_keeps_authored_grazing_ray_candidates_and_order() {
        let kit = InstanceBounds::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(kit.available);
        let mut rows = instance_frame();
        let mut source = Vec::new();
        for instance in 0..MAX_INSTANCES {
            let (root, bounds) = kit.roots[instance % kit.roots.len()];
            let at = control_pose_row(&rows, instance);
            let scale = [0.002,0.25,1.,7.][instance%4];
            let pose = [(instance%9) as f32*0.17-0.7, (instance/9) as f32*0.21-0.6, 0.03, scale];
            rows[at] = pose; rows[at+1][0] = root as f32;
            source.push((pose,bounds));
        }
        kit.append(&mut rows).unwrap();
        for (pose,bounds) in &source {
            let center: [f32;3] = std::array::from_fn(|a| (bounds.lo[a]+bounds.hi[a])*0.5);
            let extent: [f32;3] = std::array::from_fn(|a| (bounds.hi[a]-bounds.lo[a])*0.5+1e-7);
            // All root faces, extremely small scales, grazing/corner origins,
            // both sides, and inverse-ray clamp cases use the original math.
            for axis in 0..3 {
                for side in [-1.,1.] {
                    for slope in [0.,1e-9,-1e-9,1e-7,-1e-7,0.001,-0.001] {
                        let mut origin: [f32;3] = std::array::from_fn(|a| pose[a]+center[a]*pose[3]);
                        let mut ray = [slope;3]; ray[axis] = -side;
                        origin[axis] += side*(extent[axis]*pose[3]+1.);
                        let tangent = (axis+1)%3;
                        origin[tangent] += extent[tangent]*pose[3];
                        let original = |instance: usize| {
                            let (p,b) = source[instance];
                            let local = std::array::from_fn(|a| (origin[a]-p[a])/p[3]);
                            box_hit(local, ray, b, 1e-7, 10./p[3])
                        };
                        let expected: Vec<_> = (0..MAX_INSTANCES).filter(|&i| original(i)).collect();
                        let actual: Vec<_> = candidates(&rows, origin, ray, 10.).into_iter().filter(|&i| original(i)).collect();
                        assert_eq!(actual, expected, "grazing origin {origin:?}, ray {ray:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn workstation_decorative_controls_follow_panel_tables_without_stretching() {
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        for (width, height) in [(844., 390.), (390., 844.)] {
            for canvas_scale in [1., 0.5] {
                for swapped in [false, true] {
                    let view = Viewport {
                        width,
                        height,
                        canvas_scale,
                    };
                    let options = Options {
                        swap_sides: swapped,
                        swap_horizontal: swapped,
                        ..Options::default()
                    };
                    let mut rows = vec![[0.; 4]; BASE];
                    kit.append(&mut rows, view, &motion.snapshot(), options)
                        .unwrap();
                    assert_eq!(rows.len(), BASE + ROWS);
                    assert_eq!(rows[12][0], 10.);
                    let rects = panel_layout::layout(view, &motion.snapshot(), options).unwrap();
                    for i in 0..10 {
                        let panel = if i < 4 { 0 } else { 1 };
                        let asset_index = if i < 4 {
                            i
                        } else if i % 2 == 0 {
                            5
                        } else {
                            4
                        };
                        let a = &kit.manifest.assets[asset_index];
                        let pose = rows[192 + i * 2];
                        let r = rects[panel];
                        let mpp = 1.3 / height;
                        let cut =
                            panel_layout::optical_shape(r.width * mpp, r.height * mpp).unwrap();
                        let c = r.center();
                        let center = [(c[0] - width * 0.5) * mpp, (height * 0.5 - c[1]) * mpp];
                        for axis in 0..2 {
                            assert!(
                                (pose[axis] as f64 + a.min[axis] * pose[3] as f64 - center[axis])
                                    >= -cut.safe_rectangle_m[axis] * 0.5 - 1e-6
                            );
                            assert!(
                                (pose[axis] as f64 + a.max[axis] * pose[3] as f64 - center[axis])
                                    <= cut.safe_rectangle_m[axis] * 0.5 + 1e-6
                            );
                        }
                        assert!(pose[2] as f64 + a.min[2] * pose[3] as f64 > 0.);
                    }
                }
            }
        }
    }
}
