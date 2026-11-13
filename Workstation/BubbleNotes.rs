//! Authored bubble membrane data and bounded piano-roll appearance instances.
//!
//! The Blender Short mesh remains the topology authority. Duration regenerates
//! its rounded-boundary/ring coordinates instead of stretching a finished mesh.
//! The immutable packet retains source geometry and placement shape-key deltas;
//! the native shader uses their validated parameterization for arbitrary widths.
//! No audio state, texture generation, note editing or grid drawing lives here.
//! The caller may window distant offscreen duration spans to the viewport plus
//! two note heights for numerical conditioning. That changes presentation only;
//! visible onsets/ends and the document's actual durations remain authoritative.
use super::{edison_demo, engraving};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

pub(crate) const BASE: usize = (edison_demo::BASE + edison_demo::ROWS + 255) & !255;
pub(crate) const ROWS: usize = 32768;
pub(crate) const HEADER: usize = 234;
pub(crate) const DYNAMIC_BASE: usize = engraving::BASE + 512;
pub(crate) const MAX_NOTES: usize = 2048;
// Optional animation identities follow the complete dynamic browser-label packet.
// This extends the derived render packet by32KiB without moving any old address.
pub(crate) const IDENTITY_BASE: usize = super::piano_scene::END + 4096;
pub(crate) const BEND_BASE: usize = IDENTITY_BASE + MAX_NOTES;
pub(crate) const IDENTITY_END: usize = BEND_BASE + MAX_NOTES;
pub(crate) const BEND_GPU_BASE: usize = super::piano_scene::GPU_BASE + BEND_BASE - super::piano_scene::BASE;
pub(crate) const PITCH_BASE: usize = DYNAMIC_BASE + MAX_NOTES;
pub(crate) const PITCHES: usize = 128;
pub(crate) const DYNAMIC_END: usize = PITCH_BASE + PITCHES * 2;
pub(crate) const PANEL_ID_BASE: usize = 100000;
const DIRECTORY: &str = "Art/AudioWorkstation/BubbleNotes";
const VERTEX_STRIDE: usize = 6;
const NODE_STRIDE: usize = 4;
const SETTLE_SECONDS: f64 = 1.35;

pub(crate) struct Kit {
    rows: usize,
    root: usize,
    end: usize,
    node_count: usize,
    vertex_base: usize,
    vertex_count: usize,
    triangle_base: usize,
    triangle_count: usize,
}

fn integer(value: &Value, label: &str) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("invalid bubble {label}"))
}

fn number(value: &Value, low: f64, high: f64, label: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (low..=high).contains(v))
        .ok_or_else(|| format!("bubble {label} must be finite and within {low}..={high}"))
}

fn row_integer(value: f32, label: &str) -> Result<usize, String> {
    if value < 0. || value > 16_777_216. || value.fract() != 0. {
        return Err(format!("invalid bubble packet {label}"));
    }
    Ok(value as usize)
}

impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(DIRECTORY).join("BubbleNotes_runtime.json");
        let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("bubble manifest exceeds its size bound".into());
        }
        let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let get = |key: &str| integer(&data[key], key);
        let kit = Self {
            rows: get("rows")?,
            root: get("root")?,
            end: get("end")?,
            node_count: get("node_count")?,
            vertex_base: get("vertex_base")?,
            vertex_count: get("vertex_count")?,
            triangle_base: get("triangle_base")?,
            triangle_count: get("triangle_count")?,
        };
        // Establish small bounds before any address arithmetic. This loader is
        // intentionally for the exported authored kit, not arbitrary glTF.
        if get("schema")? != 1
            || get("base")? != BASE
            || get("capacity")? != ROWS
            || get("vertex_stride")? != VERTEX_STRIDE
            || get("node_stride")? != NODE_STRIDE
            || kit.rows > ROWS
            || !(1..=2048).contains(&kit.node_count)
            || kit.vertex_count != 1474
            || kit.triangle_count != 2944
            || kit.root != BASE
            || kit.end != BASE + kit.node_count * NODE_STRIDE
            || kit.vertex_base != kit.end
            || kit.triangle_base != kit.vertex_base + kit.vertex_count * VERTEX_STRIDE
            || kit.triangle_base + kit.triangle_count != BASE + kit.rows
            || data["source_object"].as_str() != Some("BubbleNote_Short")
            || data["material"]["procedural_textures"].as_bool() != Some(false)
            || data["material"]["textures"].as_array().is_none_or(|a| !a.is_empty())
            || DYNAMIC_END > engraving::BASE + engraving::ROWS
        {
            return Err("invalid authored bubble geometry manifest".into());
        }
        Ok(kit)
    }

    /// Immutable rows, uploaded once beyond the aligned Edison packet. Missing
    /// art is an error; there is no synthesized sphere or raster fallback.
    pub(crate) fn read_geometry(&self, root: &Path) -> Result<Vec<[f32; 4]>, String> {
        let path = root.join(DIRECTORY).join("BubbleNotes_runtime.bin");
        let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.take((ROWS * 16 + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() != self.rows * 16 {
            return Err("bubble geometry length disagrees with its manifest".into());
        }
        let mut rows = vec![[0.; 4]; ROWS];
        for (row, encoded) in rows.iter_mut().zip(bytes.chunks_exact(16)) {
            for (value, word) in row.iter_mut().zip(encoded.chunks_exact(4)) {
                *value = f32::from_le_bytes(word.try_into().unwrap());
                if !value.is_finite() {
                    return Err("nonfinite bubble geometry".into());
                }
            }
        }
        self.validate_geometry(&rows)?;
        Ok(rows)
    }

    fn validate_geometry(&self, rows: &[[f32; 4]]) -> Result<(), String> {
        for at in (self.root..self.end).step_by(NODE_STRIDE) {
            let lo = rows[at - BASE];
            let hi = rows[at + 1 - BASE];
            let links = rows[at + 2 - BASE];
            let count = row_integer(rows[at + 3 - BASE][0], "leaf count")?;
            let escape = row_integer(links[2], "escape address")?;
            let first = row_integer(links[3], "triangle address")?;
            if lo.iter().zip(hi).any(|(a, b)| *a > b || a.abs() > 2. || b.abs() > 2.)
                || links[0] < -1e-6
                || links[1] > 0.600001
                || links[0] > links[1]
                || escape <= at
                || escape > self.end
                || (escape - self.root) % NODE_STRIDE != 0
                || count > 4
                || (count == 0 && at + NODE_STRIDE >= self.end)
                || first < self.triangle_base
                || first > self.triangle_base + self.triangle_count
                || count > self.triangle_base + self.triangle_count - first
            {
                return Err("invalid bubble coefficient BVH".into());
            }
        }
        for index in 0..self.vertex_count {
            let at = self.vertex_base + index * VERTEX_STRIDE - BASE;
            let c = rows[at];
            let ring = rows[at + 1];
            let tangent = rows[at + 2];
            let original = rows[at + 3];
            let radius = 0.018_f32 * 0.28;
            let restored = [c[0] * 0.0225 + c[1] * radius,
                            c[2] * 0.018 + c[3] * radius, ring[0] * 0.018];
            let source = [tangent[2], tangent[3], original[0]];
            let normal_length = original[1..].iter().map(|v| v * v).sum::<f32>();
            if c.iter().any(|v| v.abs() > 2.)
                || ring[0] < -1e-6 || ring[0] > 0.600001
                || ring[1] < -1e-6 || ring[1] > 1.000001
                || ring[2].abs() > 1.000001
                || ![-1., 0., 1.].contains(&ring[3])
                || (ring[3] == 0. && ring[1] < 0.01)
                || tangent[0].abs() > 1.000001 || tangent[1].abs() > 1.000001
                || !(0.98..=1.02).contains(&normal_length)
                || restored.iter().zip(source).any(|(a, b)| (*a - b).abs() > 2e-8)
                || rows[at + 4][0].abs() > 1e-8 || rows[at + 5][0].abs() > 1e-8
            {
                return Err("invalid authored bubble surface descriptor".into());
            }
        }
        for triangle in &rows[self.triangle_base - BASE..self.triangle_base - BASE + self.triangle_count] {
            for index in &triangle[..3] {
                if row_integer(*index, "vertex index")? >= self.vertex_count {
                    return Err("bubble triangle references a missing vertex".into());
                }
            }
        }
        Ok(())
    }

    /// Write into the already allocated, unused engraving frame rows. The
    /// control thread supplies world-space note appearances; all validation is
    /// completed before the frame is changed. Over-capacity is an explicit error.
    pub(crate) fn append(&self, rows: &mut Vec<[f32; 4]>, presentation: &Value) -> Result<(), String> {
        if rows.len() < DYNAMIC_END || rows.len() < HEADER + 4 {
            return Err("bubble instances require the complete dynamic scene packet".into());
        }
        if presentation.is_null() {
            rows[HEADER..HEADER + 4].fill([0.; 4]);
            rows[DYNAMIC_BASE..DYNAMIC_END].fill([0.; 4]);
            return Ok(());
        }
        let height = number(&presentation["height_m"], 1e-6, 100., "height")?;
        let z = match presentation.get("z_m") {
            Some(value) => number(value, -1000., 1000., "Z origin")?,
            None => 0.,
        };
        let clip_array = presentation["clip_rect"]
            .as_array().filter(|v| v.len() == 4)
            .ok_or("bubble clip rectangle requires xmin,ymin,xmax,ymax")?;
        let mut clip = [0.; 4];
        for (destination, value) in clip.iter_mut().zip(clip_array) {
            *destination = number(value, -1000., 1000., "clip coordinate")? as f32;
        }
        if clip[0] >= clip[2] || clip[1] >= clip[3] {
            return Err("bubble clip rectangle must have positive dimensions".into());
        }
        let source = presentation["notes"].as_array().ok_or("bubble notes require an array")?;
        if source.len() > MAX_NOTES {
            return Err(format!("bubble presentation exceeds {MAX_NOTES} notes"));
        }
        let per_pitch_height=source.iter().any(|n|n.get("height_m").is_some());
        let mut notes = Vec::with_capacity(source.len());
        for note in source {
            let pitch = integer(&note["pitch"], "MIDI pitch")?;
            if pitch >= PITCHES {
                return Err("bubble MIDI pitch must be within 0..127".into());
            }
            let x = number(&note["x"], -1000., 1000., "onset coordinate")?;
            let y = number(&note["y"], -1000., 1000., "pitch coordinate")?;
            // A valid document duration can project far below a pixel after
            // zooming. Retain it in the packet; raster sampling, not validation,
            // decides whether an extremely narrow membrane is visible.
            let length = number(&note["length"], 1e-12, 1000., "duration width")?;
            let age = number(&note["age"], 0., 1e12, "placement age")?.min(SETTLE_SECONDS);
            let selected = match note.get("selected") {
                Some(value) => value.as_bool().ok_or("bubble selected must be boolean")?,
                None => false,
            };
            let note_height=match note.get("height_m"){Some(v)=>number(v,1e-6,100.,"note height")?,None=>height};
            notes.push((pitch, [x as f32, y as f32, length as f32,
                               (age + if selected { 4. } else { 0. }) as f32],note_height));
        }
        notes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1[0].total_cmp(&b.1[0])));
        let mut dynamic = vec![[0.; 4]; DYNAMIC_END - DYNAMIC_BASE];
        for (index, (_, note, _)) in notes.iter().enumerate() {
            dynamic[index] = *note;
        }
        let mut first = 0;
        for pitch in 0..PITCHES {
            let count = notes[first..].iter().take_while(|n| n.0 == pitch).count();
            let at = PITCH_BASE - DYNAMIC_BASE + pitch * 2;
            if count > 0 {
                let mut lo = [f32::INFINITY; 3];
                let mut hi = [f32::NEG_INFINITY; 3];
                if per_pitch_height {let (_,reference,h)=&notes[first];if notes[first..first+count].iter().any(|(_,n,nh)|(n[1]-reference[1]).abs()>1e-6||(nh-h).abs()>1e-9){return Err("matched bubble heights require one center and height per pitch".into());}}
                for (_, note, note_height) in &notes[first..first + count] {
                    let height=*note_height;
                    // Includes the complete fixed-X squash/wave envelope and
                    // initial drop. These bounds remain valid at every age.
                    lo[0] = lo[0].min(note[0]);
                    hi[0] = hi[0].max(note[0] + note[2]);
                    lo[1] = lo[1].min(note[1] - height as f32 * 0.6);
                    hi[1] = hi[1].max(note[1] + height as f32 * 0.6);
                    let lift = if note[3] >= 4. { height * 0.15 } else { 0. };
                    lo[2] = lo[2].min((z + lift) as f32);
                    hi[2] = hi[2].max((z + lift + height * 1.4) as f32);
                }
                dynamic[at] = [lo[0], lo[1], lo[2], first as f32];
                dynamic[at + 1] = [hi[0], hi[1], hi[2], count as f32];
            }
            first += count;
        }
        rows[DYNAMIC_BASE..DYNAMIC_END].copy_from_slice(&dynamic);
        let height=notes.iter().fold(height,|h,n|h.max(n.2));
        rows[HEADER] = [notes.len() as f32, height as f32, (height * 0.6) as f32, z as f32];
        rows[HEADER + 1] = clip;
        rows[HEADER + 2] = [self.root as f32, self.end as f32,
                            self.vertex_base as f32, self.triangle_base as f32];
        rows[HEADER + 3] = [DYNAMIC_BASE as f32, PITCH_BASE as f32, MAX_NOTES as f32, if per_pitch_height {2.}else{1.}];
        Ok(())
    }
    /// Append only after the furniture/browser writers finish resizing their
    /// partial packet. Identities are derived display data, never mesh storage.
    pub(crate) fn append_animation_ids(&self,rows:&mut Vec<[f32;4]>,presentation:&Value)->Result<(),String>{
        let mut notes=Vec::new();
        if !presentation.is_null(){
            let source=presentation["notes"].as_array().filter(|v|v.len()<=MAX_NOTES).ok_or("bounded bubble notes required")?;
            for note in source {
                let pitch=integer(&note["pitch"],"MIDI pitch")?;
                if pitch>=PITCHES{return Err("bubble pitch outside MIDI range".into());}
                let x=number(&note["x"],-1000.,1000.,"onset coordinate")?;
                let identity=if let Some(id)=note.get("id"){
                    let id=id.as_str().filter(|id|!id.is_empty()&&id.len()<=256).ok_or("bounded bubble note identity required")?;
                    let hash=id.bytes().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3));
                    std::array::from_fn(|i|((hash>>(i*16))&0xffff)as f32)
                }else{[0.;4]};
                let mut bend=[0.;4];
                if let Some(curve)=note.get("bend_curve"){
                    let curve=curve.as_array().filter(|a|a.len()==4).ok_or("bend curve needs four coefficients")?;
                    for(i,v)in curve.iter().enumerate(){bend[i]=number(v,-1000.,1000.,"bend coefficient")?as f32;}
                }
                notes.push((pitch,x as f32,identity,bend));
            }
        }
        notes.sort_by(|a,b|a.0.cmp(&b.0).then_with(||a.1.total_cmp(&b.1)));
        rows.resize(IDENTITY_END,[0.;4]);rows[IDENTITY_BASE..IDENTITY_END].fill([0.;4]);
        for(i,(_,_,identity,bend))in notes.into_iter().enumerate(){rows[IDENTITY_BASE+i]=identity;rows[BEND_BASE+i]=bend;}
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kit() -> Kit {
        Kit { rows: 0, root: BASE, end: BASE + 4, node_count: 1,
              vertex_base: BASE + 4, vertex_count: 1474,
              triangle_base: BASE + 8848, triangle_count: 2944 }
    }

    fn presentation(count: usize) -> Value {
        let notes: Vec<Value> = (0..count).map(|i| json!({
            "x": (count - i) as f64 * 0.01, "y": (i % 128) as f64 * 0.02,
            "length": 0.03, "age": 99., "selected": true, "pitch": i % 128
        })).collect();
        json!({"height_m": 0.018, "clip_rect": [-1., -1., 50., 5.], "notes": notes})
    }

    #[test]
    fn stable_animation_identity_survives_pan_resize_and_pitch_partitioning() {
        let mut rows=vec![[0.;4];super::super::piano_scene::END+4096];
        let mut p=presentation(2);
        p["notes"][0]["id"]=json!("source-note-a");
        p["notes"][1]["id"]=json!("source-note-b");
        kit().append_animation_ids(&mut rows,&p).unwrap();
        let identities=rows[IDENTITY_BASE..IDENTITY_BASE+2].to_vec();
        assert_ne!(identities[0],identities[1]);
        for note in p["notes"].as_array_mut().unwrap(){note["x"]=json!(note["x"].as_f64().unwrap()+2.);note["length"]=json!(0.2);note["selected"]=json!(false);}
        p["notes"].as_array_mut().unwrap().reverse();
        kit().append_animation_ids(&mut rows,&p).unwrap();
        assert_eq!(rows[IDENTITY_BASE..IDENTITY_BASE+2],identities);
        let before=rows.clone();p["notes"][0]["id"]=json!("");
        assert!(kit().append_animation_ids(&mut rows,&p).is_err());assert_eq!(rows,before);
    }

    #[test]
    fn full_document_capacity_is_preserved_and_partitioned_without_overwriting_neighbors() {
        let mut rows = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        kit().append(&mut rows, &presentation(MAX_NOTES)).unwrap();
        assert_eq!(rows[HEADER][0], MAX_NOTES as f32);
        assert_eq!(rows[DYNAMIC_BASE - 1], [123.; 4]);
        assert_eq!(rows[DYNAMIC_END], [123.; 4]);
        let mut total = 0;
        for pitch in 0..PITCHES {
            let group = PITCH_BASE + pitch * 2;
            assert_eq!(rows[group][3], total as f32);
            assert_eq!(rows[group + 1][3], 16.);
            // Selection raises geometry, including its conservative bounds.
            assert!((rows[group][2] - 0.018 * 0.15).abs() < 1e-7);
            assert!((rows[group + 1][2] - 0.018 * 1.55).abs() < 1e-7);
            total += rows[group + 1][3] as usize;
        }
        assert_eq!(total, MAX_NOTES);
        assert!((rows[DYNAMIC_BASE][3] - 5.35).abs() < 1e-6);
        assert_eq!(rows[DYNAMIC_BASE + MAX_NOTES - 1][2], 0.03);
    }

    #[test]
    fn oversized_or_invalid_appearance_does_not_partially_modify_the_frame() {
        let original = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        let mut rows = original.clone();
        assert!(kit().append(&mut rows, &presentation(MAX_NOTES + 1)).is_err());
        assert_eq!(rows, original);
        let mut bad = presentation(1);
        bad["notes"][0]["pitch"] = json!(128);
        assert!(kit().append(&mut rows, &bad).is_err());
        assert_eq!(rows, original);
        bad = presentation(1);
        bad["notes"][0]["length"] = json!(0.);
        assert!(kit().append(&mut rows, &bad).is_err());
        assert_eq!(rows, original);
    }

    #[test]
    fn zoomed_subpixel_duration_is_retained_without_rounding_or_a_hidden_visibility_cap() {
        let mut rows = vec![[0.; 4]; engraving::BASE + engraving::ROWS];
        let mut tiny = presentation(1);
        tiny["notes"][0]["length"] = json!(1e-12);
        kit().append(&mut rows, &tiny).unwrap();
        assert_eq!(rows[HEADER][0], 1.);
        assert_eq!(rows[DYNAMIC_BASE][2], 1e-12_f32);
        assert_eq!(rows[PITCH_BASE + 1][3], 1.);
    }

    #[test]
    fn clearing_roll_removes_instances_but_preserves_engraving_and_edison_headers() {
        let mut rows = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        kit().append(&mut rows, &presentation(2)).unwrap();
        kit().append(&mut rows, &Value::Null).unwrap();
        assert_eq!(rows[HEADER], [0.; 4]);
        assert_eq!(rows[232], [123.; 4]);
        assert_eq!(rows[233], [123.; 4]);
        assert!(rows[DYNAMIC_BASE..DYNAMIC_END].iter().all(|row| *row == [0.; 4]));
        assert_eq!(rows[engraving::BASE], [123.; 4]);
    }
}
