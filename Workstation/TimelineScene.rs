//! Document track rows using the existing authored gem buttons and tape meshes.
//! The control thread projects elapsed seconds and packs source-derived stereo
//! rainbow previews into the existing tape surfaces.
use super::{
    arrangement::{LANE_HEIGHT, MAX_VISIBLE_TRACKS, RULER_HEIGHT, clip_body},
    audio_spheres, control_engraving, engraving,
    panel_layout::Viewport,
    piano_roll::Rect,
};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

pub(crate) const BASE: usize = control_engraving::BASE + control_engraving::ROWS;
pub(crate) const ROWS: usize = 262_144;
pub(crate) const HEADER: usize = 248;
pub(crate) const DYNAMIC_BASE: usize = audio_spheres::END;
pub(crate) const MAX_TRACKS: usize = MAX_VISIBLE_TRACKS;
pub(crate) const MAX_CLIPS: usize = 128;
const MAX_HANDLES: usize = MAX_TRACKS * 4;
const MAX_INSTANCES: usize = MAX_HANDLES + MAX_CLIPS;
const LABEL_BASE: usize = DYNAMIC_BASE + MAX_INSTANCES * 3;
pub(crate) const WAVE_STRIDE: usize = 1025;
pub(crate) const WAVE_ROWS: usize = MAX_INSTANCES * WAVE_STRIDE;
// Keep immutable model addresses stable; large display data follows the dynamic tail.
pub(crate) const WAVE_GPU_BASE: usize = super::piano_scene::GPU_BASE + super::timeline_controls::END - super::piano_scene::BASE;
pub(crate) const WAVE_BASE: usize = engraving::BASE + engraving::ROWS;
pub(crate) const END: usize = WAVE_BASE + WAVE_ROWS;
const DIRECTORY: &str = "Art/AudioWorkstation/Timeline";

struct Asset {
    root: usize,
    end: usize,
    material_base: usize,
    materials: usize,
    triangles: usize,
    min: [f64; 3],
    max: [f64; 3],
}
pub(crate) struct Kit {
    used: usize,
    assets: Vec<Asset>,
}

fn integer(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("invalid timeline integer".into())
}
fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("invalid timeline coordinate".into())
}
fn bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("oversized timeline asset".into());
    }
    Ok(bytes)
}

impl Kit {
    pub(crate) fn bottom_controls(&self)->[([usize;3],[[f64;3];2]);2]{std::array::from_fn(|i|{let a=&self.assets[i];([a.root,a.end,a.material_base],[a.min,a.max])})}
    /// Reuse the authored mini gem for the neighboring insert navigation.
    pub(crate) fn mini_control(&self) -> ([usize; 3], [[f64; 3]; 2]) {
        let a = &self.assets[1];
        ([a.root, a.end, a.material_base], [a.min, a.max])
    }
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let value: Value = serde_json::from_slice(&bounded_file(
            &root.join(DIRECTORY).join("runtime.json"),
            65536,
        )?)
        .map_err(|e| e.to_string())?;
        let used = integer(&value["rows"])?;
        if integer(&value["schema"])? != 1
            || integer(&value["base"])? != BASE
            || integer(&value["capacity"])? != ROWS
            || used > ROWS
            || LABEL_BASE + MAX_HANDLES * 2 > engraving::BASE + engraving::ROWS
        {
            return Err("invalid timeline packet layout".into());
        }
        let source = value["assets"]
            .as_array()
            .filter(|a| a.len() == 3)
            .ok_or("timeline needs its three authored meshes")?;
        let mut assets = Vec::new();
        for (v, name) in source
            .iter()
            .zip(["GemButton_Wide", "GemButton_Mini", "TapeClip_Long"])
        {
            let asset = Asset {
                root: integer(&v["root"])?,
                end: integer(&v["end"])?,
                material_base: integer(&v["material_base"])?,
                materials: integer(&v["materials"])?,
                triangles: integer(&v["triangles"])?,
                min: serde_json::from_value(v["min"].clone()).map_err(|e| e.to_string())?,
                max: serde_json::from_value(v["max"].clone()).map_err(|e| e.to_string())?,
            };
            if v["name"] != name
                || !(1..16).contains(&asset.materials)
                || asset.material_base < BASE
                || asset.material_base >= BASE + used
                || asset.root != asset.material_base + asset.materials * 40
                || asset.end <= asset.root
                || asset.end > BASE + used
                || (asset.end - asset.root) % 3 != 0
                || (asset.end - asset.root) / 3 > 16384
                || asset.triangles > ROWS / 6
                || asset.end + asset.triangles * 6 > BASE + used
                || asset.min.iter().chain(&asset.max).any(|x| !x.is_finite())
                || (0..3).any(|i| asset.min[i] >= asset.max[i])
            {
                return Err("invalid timeline mesh bounds".into());
            }
            assets.push(asset);
        }
        Ok(Self { used, assets })
    }

    pub(crate) fn read_geometry(&self, root: &Path) -> Result<Vec<[f32; 4]>, String> {
        let bytes = bounded_file(&root.join(DIRECTORY).join("runtime.bin"), ROWS * 16)?;
        if bytes.len() != self.used * 16 {
            return Err("timeline geometry length mismatch".into());
        }
        let rows: Vec<[f32; 4]> = bytes
            .chunks_exact(16)
            .map(|b| {
                std::array::from_fn(|i| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect();
        if rows.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite timeline geometry".into());
        }
        let address =
            |v: f32| v >= BASE as f32 && v <= (BASE + self.used) as f32 && v.fract() == 0.;
        for asset in &self.assets {
            for at in (asset.root..asset.end).step_by(3) {
                let a = rows[at - BASE];
                let b = rows[at + 1 - BASE];
                let n = rows[at + 2 - BASE][0];
                if !address(a[3])
                    || !address(b[3])
                    || a[3] as usize <= at
                    || a[3] as usize > asset.end
                    || (a[3] as usize - asset.root) % 3 != 0
                    || !(0.0..=4.).contains(&n)
                    || n.fract() != 0.
                    || (b[3] as usize) < asset.end
                    || (b[3] as usize - asset.end) % 6 != 0
                    || b[3] as usize + n as usize * 6 > asset.end + asset.triangles * 6
                    || (0..3).any(|i| a[i] > b[i])
                {
                    return Err("invalid timeline BVH".into());
                }
            }
            for at in (asset.end..asset.end + asset.triangles * 6).step_by(6) {
                let m = rows[at - BASE][3];
                if !(0.0..asset.materials as f32).contains(&m) || m.fract() != 0. {
                    return Err("invalid timeline material reference".into());
                }
            }
        }
        Ok(rows)
    }

    pub(crate) fn append(
        &self,
        rows: &mut [[f32; 4]],
        view: Viewport,
        value: &Value,
    ) -> Result<(), String> {
        if rows.len() < END || rows.len() < 256 {
            return Err("timeline needs complete frame storage".into());
        }
        // Construct the bounded packet separately; invalid input cannot partly
        // mutate a frame subsequently submitted to a native graphics backend.
        let mut packed = vec![[0.; 4]; WAVE_BASE - DYNAMIC_BASE];
        let mut wave_rows = vec![[0.; 4]; WAVE_ROWS];
        let mut header = [[0.; 4]; 8];
        if value.is_null() {
            rows[HEADER..HEADER+8].copy_from_slice(&header);
            rows[DYNAMIC_BASE..WAVE_BASE].copy_from_slice(&packed);
            rows[WAVE_BASE..END].copy_from_slice(&wave_rows);
            return Ok(());
        }
        let canvas = &value["canvas"];
        let get = |key: &str, index: usize| -> Result<f64, String> {
            number(if canvas.is_array() {
                &canvas[index]
            } else {
                &canvas[key]
            })
        };
        let (left, top, width, height) = (
            get("left", 0)?,
            get("top", 1)?,
            get("width", 2)?,
            get("height", 3)?,
        );
        let seconds = number(&value["seconds_per_width"])?;
        let start = number(&value["start_seconds"])?;
        if !(0.000001..=86400.).contains(&seconds)
            || !(-86400.0..=86400.).contains(&start)
            || [left, top, width, height].iter().any(|v| v.abs() > 1e6)
        {
            return Err("timeline view outside supported elapsed-time range".into());
        }
        let mut body = clip_body(Rect {
            left,
            top,
            width:width-super::timeline_controls::reserve(super::timeline_controls::flags(value)),
            height,
        });
        if let Some(r)=value["bar_body"].as_array().filter(|r|r.len()==4){body=Rect{left:number(&r[0])?,top:number(&r[1])?,width:number(&r[2])?,height:number(&r[3])?};}
        if body.width <= 0. || (body.height <= 0. && value["rack_controls"].as_array().is_none_or(|c|c.is_empty())) {
            return self.append(rows, view, &Value::Null);
        }
        let tracks = value["tracks"]
            .as_array()
            .ok_or("timeline tracks must be an array")?;
        let clips = value["clips"]
            .as_array()
            .ok_or("timeline clips must be an array")?;
        let scroll = integer(&value["scroll_tracks"])?;
        let mpp = 1.3 / view.height;
        let world = |x: f64, y: f64| [(x - view.width * 0.5) * mpp, (view.height * 0.5 - y) * mpp];
        let low = world(left, top + height);
        let high = world(body.left + body.width, body.top);
        header[1] = [low[0] as f32, low[1] as f32, high[0] as f32, high[1] as f32];
        header[5][0] = mpp as f32;
        let mut count = 0;
        let mut visible = Vec::new();
        let visible_rows = super::arrangement::lane_count(tracks,scroll,body.height);
        for (index, track) in tracks.iter().enumerate().skip(scroll).take(visible_rows) {
            let lane_height=super::arrangement::lane_height(track);
            let y = top + RULER_HEIGHT + super::arrangement::lane_top(tracks,scroll,index);
            visible.push((track["id"].as_str().ok_or("track requires an ID")?, y,lane_height));
            if track["trackless"]==true{continue;}
            // Cardinal's unlabelled full-height vertical handle, using exactly
            // the ruler's optical cut rather than a stretched button mesh.
            let w = super::arrangement::HANDLE_FACE_WIDTH * mpp;
            let h = lane_height * mpp;
            let shape = super::panel_layout::optical_shape(w, h)?;
            let u = shape.local_unit_m;
            let center = world(left + super::arrangement::HANDLE_FACE_WIDTH * 0.5,
                               y + lane_height * 0.5);
            let at = count * 3;
            packed[at] = [center[0] as f32, center[1] as f32,
                         (0.001 + shape.dimensions_m[2] + if value["selected_track"] == track["id"] {2. * mpp} else {0.}) as f32, u as f32];
            // Negative type selects the shared analytic ruler cut. Keep the
            // existing ruby material and instance IDs; no label is packed.
            packed[at + 1] = [0., 0., if track["comp_parent"].is_string(){12704.}else{32.}, if value["selected_track"] == track["id"] {-2.} else {-1.}];
            packed[at + 2] = [(w / (2. * u)) as f32, (h / (2. * u)) as f32,
                             (u / (rows[35][0].min(rows[35][1]) as f64 - 0.000001).max(1e-12)) as f32, 0.];
            count += 1;
        }
        let handles = count;
        let asset = &self.assets[2];
        
        let body_left = body.left;
        let body_width = body.width;
        let body_right = body_left + body_width;
        let mut clip_count = 0;
        for clip in clips {
            let (y,lane_height)=if clip["track"]=="" {
                (number(&clip["free_top"])?,number(&clip["free_height"])?)
            }else{let Some((_,y,h))=visible.iter().find(|(id,_,_)|clip["track"].as_str()==Some(*id))else{continue;};(*y,*h)};
            let x = body_left + (number(&clip["start_seconds"])? - start) / seconds * body_width;
            let clip_width = (number(&clip["duration_seconds"])? / seconds * body_width).max(6.);
            if x + clip_width <= body_left || x >= body_right {
                continue;
            }
            if clip_count >= MAX_CLIPS {
                return Err(
                    "visible timeline exceeds 128 clips; zoom or scroll to reduce the view".into(),
                );
            }
            // Window huge offscreen clips before converting to float32, while
            // retaining the authored tile phase at the original clip onset.
            // Perforations therefore stay anchored to the clip when panning.
            let from = x.max(body_left);
            let to = (x + clip_width).min(body_right);
            let scale = (if clip["track"]==""{lane_height*(1.-2.*super::arrangement::CLIP_MARGIN/96.)}else{(lane_height-2.*super::arrangement::CLIP_MARGIN).max(lane_height*0.5)}) * mpp / (asset.max[1] - asset.min[1]);
            let center = world(from, y + lane_height*0.5);
            let at = count * 3;
            packed[at] = [
                center[0] as f32,
                center[1] as f32,
                // Equal tape heights preserve reverse document order at
                // overlaps, matching the controller's highest-priority pick.
                0.001,
                scale as f32,
            ];
            packed[at + 1] = [
                asset.root as f32,
                asset.end as f32,
                asset.material_base as f32,
                if value["selected_clip"] == clip["id"] {2.} else {1.},
            ];
            packed[at + 2] = [
                ((to - from) * mpp / scale) as f32,
                0.036,
                0.00045,
                (((from - x) * mpp / scale).rem_euclid(0.22)) as f32,
            ];
            // Retain the analyzer's complete 128 x 32 field on the tape face.
            if let Some(intensity)=clip["clip_spectrogram"]["intensity"].as_array().filter(|v|v.len()==128*32) {
                let wave_at=count*WAVE_STRIDE;
                wave_rows[wave_at]=[3.,((from-x)/clip_width) as f32,((to-from)/clip_width) as f32,0.];
                for (index,energy) in intensity.iter().enumerate() {
                    wave_rows[wave_at+1+index/4][index%4]=energy.as_f64().unwrap_or(0.) as f32;
                }
            } else {
            let detailed=clip["waveform_detail"].as_array().filter(|v|v.len()==128);
            if let Some(wave) = detailed.or_else(||clip["rainbow_waveform"].as_array().filter(|v| v.len() == 128)) {
                let wave_at = count * WAVE_STRIDE;
                wave_rows[wave_at] = [1., ((from - x) / clip_width) as f32,
                    ((to - from) / clip_width) as f32, 0.];
                if detailed.is_some(){
                    let lo=clip["waveform_view"][0].as_f64().unwrap_or(0.);
                    let hi=clip["waveform_view"][1].as_f64().unwrap_or(1.);
                    let span=(hi-lo).max(f64::EPSILON);
                    wave_rows[wave_at]=[2.,(((from-x)/clip_width-lo)/span) as f32,((to-from)/clip_width/span) as f32,0.];
                }
                for (index, column) in wave.iter().enumerate() {
                    for component in 0..4 {
                        wave_rows[wave_at + 1 + index][component] = column[component].as_f64().unwrap_or(0.) as f32;
                    }
                }
            }
            }
            count += 1;
            clip_count += 1;
        }
        header[0] = [
            count as f32,
            DYNAMIC_BASE as f32,
            LABEL_BASE as f32,
            handles as f32,
        ];
        header[3] = [0.22, 0.036, 0.00045, WAVE_GPU_BASE as f32];
        // The seek hit region has a real thin ruler rail and ticks, suspended
        // over the same studio. Tick positions use elapsed seconds only.
        let ruler_y = world(0., top + RULER_HEIGHT - 8.)[1];
        header[2] = [
            world(body_left, 0.)[0] as f32,
            world(body_right, 0.)[0] as f32,
            ruler_y as f32,
            1.,
        ];
        let target_step = seconds / 8.;
        let magnitude = 10_f64.powf(target_step.log10().floor());
        let step = [1., 2., 5., 10.]
            .into_iter()
            .find(|factor| magnitude * factor >= target_step)
            .unwrap_or(10.)
            * magnitude;
        let first = (start / step).ceil() * step;
        let tick_count = (((start + seconds - first) / step).floor() + 1.).clamp(0., 32.);
        header[6] = [
            world(body_left + (first - start) / seconds * body_width, 0.)[0] as f32,
            (step / seconds * body_width * mpp) as f32,
            (5. * mpp) as f32,
            tick_count as f32,
        ];
        if let Some(playhead) = value["playhead_seconds"].as_f64().filter(|s| s.is_finite()) {
            let x = body_left + (playhead - start) / seconds * body_width;
            if x >= body_left && x <= body_right {
                header[4] = [world(x, 0.)[0] as f32, low[1] as f32, high[1] as f32, 1.];
            }
        }
        if packed
            .iter()
            .chain(&header)
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err("nonfinite timeline frame".into());
        }
        if let Some(controls)=value["rack_controls"].as_array(){
            if handles+controls.len()>MAX_HANDLES || count+controls.len()>192{return Err("rack exceeds timeline instance budget".into());}
            let n=controls.len();
            packed.copy_within(handles*3..count*3,(handles+n)*3);
            wave_rows.copy_within(handles*WAVE_STRIDE..count*WAVE_STRIDE,(handles+n)*WAVE_STRIDE);
            wave_rows[handles*WAVE_STRIDE..(handles+n)*WAVE_STRIDE].fill([0.;4]);
            for (i,c) in controls.iter().enumerate(){
                let index=handles+i;let r:&Value=&c["rect"];
                let (x,y,w,h)=(number(&r[0])?,number(&r[1])?,number(&r[2])?,number(&r[3])?);
                if w<=0.||h<=0.{return Err("rack control requires positive dimensions".into());}
                let asset=&self.assets[if w>h*1.7{0}else{1}];
                let scale=(w*mpp/(asset.max[0]-asset.min[0])).min(h*mpp/(asset.max[1]-asset.min[1]));
                let center=world(x+w*0.5,y+h*0.5);let at=index*3;
                packed[at]=[center[0] as f32,center[1] as f32,(-asset.min[2]*scale+if c["popup"]==true{if c["background"]==true{0.25}else{0.263}}else{0.001}+if c["active"]==true{2.*mpp}else{0.}) as f32,scale as f32];
                packed[at+1]=[asset.root as f32,asset.end as f32,asset.material_base as f32,0.];
                let label=c["label"].as_str().filter(|v|v.is_ascii()&&v.len()<=8).ok_or("rack label exceeds eight ASCII characters")?;
                let font=(9.*mpp).min(w*mpp/(label.len().max(1) as f64*0.58));
                packed[at+2]=[(asset.max[0]-asset.min[0]) as f32,(asset.max[1]-asset.min[1]) as f32,(asset.max[2]-asset.min[2]) as f32,font as f32];
                for (j,b) in label.bytes().enumerate(){packed[LABEL_BASE-DYNAMIC_BASE+index*2+j/4][j%4]=b as f32;}
                if c["background"]==true {
                    let shape=super::panel_layout::optical_shape(w*mpp,h*mpp)?;
                    let u=shape.local_unit_m;
                    packed[at]=[center[0] as f32,center[1] as f32,0.25,u as f32];
                    packed[at+1]=[0.,0.,32.,-1.];
                    packed[at+2]=[(w*mpp/(2.*u)) as f32,(h*mpp/(2.*u)) as f32,
                        (u/(rows[35][0].min(rows[35][1]) as f64-0.000001).max(1e-12)) as f32,0.];
                }
                let low=world(x,y+h);let high=world(x+w,y);
                header[1][0]=header[1][0].min(low[0] as f32);header[1][1]=header[1][1].min(low[1] as f32);
                header[1][2]=header[1][2].max(high[0] as f32);header[1][3]=header[1][3].max(high[1] as f32);
            }
            header[0][0]=(count+n) as f32;header[0][3]=(handles+n) as f32;
        }
        rows[DYNAMIC_BASE..WAVE_BASE].copy_from_slice(&packed);
            rows[WAVE_BASE..END].copy_from_slice(&wave_rows);
        rows[HEADER..HEADER + 8].copy_from_slice(&header);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn spectral_clip_packs_real_intensity_and_visible_time_range() {
        let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut rows=vec![[0.;4];END];
        let mut intensity=vec![0.;128*32];
        intensity[31*128+127]=0.9;
        let value=json!({"canvas":{"left":100.,"top":100.,"width":700.,"height":400.},
            "start_seconds":8.,"seconds_per_width":8.,"scroll_tracks":0,
            "tracks":[{"id":"one","name":"One","kind":"audio","gain":1.}],
            "clips":[{"id":"clip","track":"one","kind":"audio","start_seconds":0.,"duration_seconds":32.,
                "clip_spectrogram":{"intensity":intensity}}]});
        kit.append(&mut rows,Viewport{width:1000.,height:800.,canvas_scale:1.},&value).unwrap();
        let at=WAVE_BASE+WAVE_STRIDE;
        assert_eq!(rows[at],[3.,0.25,0.25,0.]);
        assert_eq!(rows[at+1024][3],0.9);
        assert_eq!(rows[at+1],[0.;4]);
    }
    #[test]
    fn timeline_uses_document_rows_and_authored_geometry_without_preview_tracks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let kit = Kit::load(root).unwrap();
        kit.read_geometry(root).unwrap();
        let mut rows = vec![[0.; 4]; END];
        let view = Viewport {
            width: 1000.,
            height: 800.,
            canvas_scale: 1.,
        };
        kit.append(&mut rows,view,&json!({"canvas":{"left":100.,"top":100.,"width":700.,"height":400.},
            "start_seconds":0.,"seconds_per_width":16.,"scroll_tracks":0,"tracks":[{"id":"one","name":"One","kind":"midi","gain":0.5}],
            "clips":[{"id":"clip","track":"one","start_seconds":4.,"duration_seconds":4.}]})).unwrap();
        assert_eq!(rows[HEADER][0], 2.);
        assert_eq!(rows[HEADER][3], 1.);
        let pose = rows[DYNAMIC_BASE + 3];
        let expected = (100. + super::super::arrangement::BODY_OFFSET + (700. - super::super::arrangement::BODY_OFFSET - 57.) * 0.25 - 500.) * 1.3 / 800.;
        assert!((pose[0] as f64 - expected).abs() < 1e-7);
        kit.append(&mut rows, view, &Value::Null).unwrap();
        assert!(rows[HEADER..HEADER + 8].iter().flatten().all(|v| *v == 0.));
    }

    #[test]
    fn timeline_windowing_keeps_tape_pattern_anchored_and_selection_preserves_overlap_height() {
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let view = Viewport {
            width: 1000.,
            height: 800.,
            canvas_scale: 1.,
        };
        let canvas = Rect {
            left: 100.,
            top: 100.,
            width: 700.,
            height: 400.,
        };
        let body = clip_body(canvas);
        let mut value = json!({"canvas":{"left":canvas.left,"top":canvas.top,"width":canvas.width,"height":canvas.height},
            "start_seconds":0.,"seconds_per_width":16.,"scroll_tracks":0,
            "selected_track":"track","selected_clip":"earlier","tracks":[{"id":"track","name":"Audio","kind":"audio","gain":1.}],
            "clips":[{"id":"earlier","track":"track","start_seconds":0.,"duration_seconds":32.},
                     {"id":"later","track":"track","start_seconds":1.,"duration_seconds":24.}]});
        let mut rows = vec![[0.; 4]; END];
        let mut source_at_twelve = None::<f64>;
        for start in [2., 3., 11.9] {
            value["start_seconds"] = json!(start);
            kit.append(&mut rows, view, &value).unwrap();
            let first = DYNAMIC_BASE + rows[HEADER][3] as usize * 3;
            assert_eq!(rows[DYNAMIC_BASE+1][3],-2.,"selected handle carries blue display state");
            assert_eq!(rows[first+1][3],2.,"selected tape carries blue display state");
            assert_eq!(rows[first+4][3],1.,"other tape retains its artwork");
            let pose = rows[first];
            let phase = rows[first + 2][3] as f64;
            assert!((0.0..0.22).contains(&phase));
            assert!(
                phase > 0.001,
                "an offscreen onset must not restart its tape tile"
            );
            assert_eq!(
                pose[2],
                rows[first + 3][2],
                "selection must not override later-clip picking priority"
            );
            let x = body.left + (12. - start) / 16. * body.width;
            let world_x = (x - view.width * 0.5) * 1.3 / view.height;
            let sampled = (phase + (world_x - pose[0] as f64) / pose[3] as f64).rem_euclid(0.22);
            if let Some(expected) = source_at_twelve {
                assert!(
                    (sampled - expected).abs() < 1e-6,
                    "panning changed the tape surface at elapsed second12"
                );
            } else {
                source_at_twelve = Some(sampled);
            }
        }
    }

    #[test]
    fn timeline_rows_have_only_unlabelled_vertical_handles_and_a_ruler() {
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let view = Viewport {
            width: 1000.,
            height: 800.,
            canvas_scale: 1.,
        };
        let mut rows = vec![[0.; 4]; END];
        kit.append(
            &mut rows,
            view,
            &json!({"canvas":{"left":100.,"top":100.,"width":700.,"height":400.},
            "start_seconds":3.,"seconds_per_width":16.,"scroll_tracks":0,
            "tracks":[{"id":"cc","name":"Envelope","kind":"cc"},
                      {"id":"midi","name":"Piano","kind":"midi","gain":1.}],"clips":[]}),
        )
        .unwrap();
        assert_eq!(rows[HEADER][3], 2., "one handle per track of any kind");
        let mpp = 1.3 / view.height;
        for instance in 0..2 {
            let pose = rows[DYNAMIC_BASE + instance * 3];
            let shape = rows[DYNAMIC_BASE + instance * 3 + 2];
            assert_eq!(rows[DYNAMIC_BASE + instance * 3 + 1][3], -1.);
            assert!((2. * shape[0] as f64 * pose[3] as f64 / mpp - 13.).abs() < 1e-4);
            assert!((2. * shape[1] as f64 * pose[3] as f64 / mpp - LANE_HEIGHT).abs() < 1e-4);
            assert_eq!(rows[LABEL_BASE + instance * 2], [0.; 4]);
        }
        assert_eq!(
            rows[HEADER + 2][3],
            1.,
            "seek ruler must render even with no clips"
        );
        assert!(rows[HEADER + 2][0] < rows[HEADER + 2][1]);
        assert!(rows[HEADER + 6][1] > 0. && rows[HEADER + 6][3] > 0.);
        assert!(
            rows[HEADER + 2][2] > rows[HEADER + 1][3],
            "ruler must be above lane clipping"
        );
    }
    #[test]
    fn variable_lanes_pack_handles_and_tapes_at_the_shared_bounds() {
        let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let view=Viewport{width:1000.,height:800.,canvas_scale:1.};
        let mut rows=vec![[0.;4];END];
        kit.append(&mut rows,view,&json!({"canvas":[100.,100.,700.,400.],
            "start_seconds":0.,"seconds_per_width":16.,"scroll_tracks":0,
            "tracks":[{"id":"a","lane_height":112.},{"id":"b","lane_height":48.}],
            "clips":[{"id":"c","track":"b","start_seconds":0.,"duration_seconds":2.}]})).unwrap();
        assert_eq!(rows[HEADER][3],2.);
        let mpp=1.3/view.height;
        for (i,(height,center)) in [(112.,100.+RULER_HEIGHT+56.),(48.,100.+RULER_HEIGHT+112.+24.)].into_iter().enumerate(){
            let p=rows[DYNAMIC_BASE+i*3];let shape=rows[DYNAMIC_BASE+i*3+2];
            assert!((2.*shape[1] as f64*p[3] as f64/mpp-height).abs()<1e-4);
            assert!((view.height*0.5-p[1] as f64/mpp-center).abs()<1e-4);
        }
        assert_eq!(rows[DYNAMIC_BASE+7][3],1.);
        assert!((rows[DYNAMIC_BASE+6][1]-rows[DYNAMIC_BASE+3][1]).abs()<1e-6);
    }

}

#[cfg(test)]mod signed_timeline_view_tests {
 use super::*;use serde_json::json;
 #[test]fn negative_viewport_is_finite_and_keeps_clips_at_their_document_time(){
  let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  let mut rows=vec![[0.;4];END];
  let view=Viewport{width:1000.,height:800.,canvas_scale:1.};
  kit.append(&mut rows,view,&json!({"canvas":{"left":100.,"top":100.,"width":700.,"height":400.},
   "start_seconds":-4.,"seconds_per_width":16.,"scroll_tracks":0,
   "tracks":[{"id":"one","name":"One","kind":"midi","gain":1.}],
   "clips":[{"id":"clip","track":"one","start_seconds":0.,"duration_seconds":4.}]})).unwrap();
  let pose=rows[DYNAMIC_BASE+3];let offset=super::super::arrangement::BODY_OFFSET;let expected=(100.+offset+(700.-offset-57.)*0.25-500.)*1.3/800.;
  assert!((pose[0] as f64-expected).abs()<1e-7);assert!(rows.iter().flatten().all(|n|n.is_finite()));
 }
}

#[cfg(test)]mod drum_rack_scene_tests{
 use super::*;use serde_json::json;
 #[test]fn drum_rack_four_rows_pack_only_controls_without_a_background(){
  let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  let mut ui=super::super::drum_rack_ui::Ui::default();
  ui.open=true;
  ui.sync(&json!({"drum_rack":{"channels":(0..4).map(|i|json!({"name":format!("Row {i}"),"steps":1,"volume":0.82})).collect::<Vec<_>>()}}));
  ui.update(&super::super::piano_roll::Input{left:true,pointer:Some([500.,10.]),..Default::default()},Rect{left:0.,top:0.,width:760.,height:600.},false);
  let mut rows=vec![[0.;4];END];
  kit.append(&mut rows,Viewport{width:760.,height:600.,canvas_scale:1.},&json!({"canvas":[0.,225.,760.,375.],"seconds_per_width":16.,"start_seconds":0.,"scroll_tracks":0,"tracks":[{"id":"track"}],"clips":[],"rack_controls":ui.packet()})).unwrap();
  assert_eq!(rows[HEADER][0],23.);assert_eq!(rows[HEADER][3],23.);
  assert_eq!(rows[DYNAMIC_BASE+1][3],-1.); // Existing timeline handle.
  assert_eq!(rows[DYNAMIC_BASE+4][3],0.);
  assert!(rows[DYNAMIC_BASE+4][0]>=BASE as f32);
  assert_eq!(rows[LABEL_BASE+2][0],b'T' as f32);
  for i in 1..23 {assert_eq!(rows[DYNAMIC_BASE+i*3+1][3],0.,"rack controls have no panel background");}
  assert!(rows.iter().flatten().all(|v|v.is_finite()));
 }
}

#[cfg(test)]mod drum_rack_small_canvas_tests{
 use super::*;use serde_json::json;
 #[test]fn drum_rack_remains_visible_when_no_timeline_lane_fits(){let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();let mut rows=vec![[0.;4];END];kit.append(&mut rows,Viewport{width:960.,height:600.,canvas_scale:1.},&json!({"canvas":[220.,360.,500.,40.],"start_seconds":0.,"seconds_per_width":16.,"scroll_tracks":0,"tracks":[],"clips":[],"rack_controls":[{"rect":[220.,200.,80.,25.],"label":"DRUMS","active":true}]})).unwrap();assert_eq!(rows[HEADER][0],1.);assert_eq!(rows[HEADER][3],1.);assert_eq!(rows[LABEL_BASE][0],b'D' as f32);}
}

#[cfg(test)]mod comp_lane_tests{
 use super::*;use serde_json::json;
 #[test]fn comp_lane_handle_uses_ivory_casing_palette_with_same_gem_cut(){
  let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();let mut rows=vec![[0.;4];END];
  kit.append(&mut rows,Viewport{width:1000.,height:800.,canvas_scale:1.},&json!({"canvas":{"left":100.,"top":100.,"width":700.,"height":400.},"start_seconds":0.,"seconds_per_width":16.,"scroll_tracks":0,"tracks":[{"id":"a","kind":"audio","lane_height":64.},{"id":"take","kind":"audio","comp_parent":"a","lane_height":96.}],"clips":[]})).unwrap();
  assert_eq!(rows[DYNAMIC_BASE+1],[0.,0.,32.,-1.]);assert_eq!(rows[DYNAMIC_BASE+4],[0.,0.,12704.,-1.]);
  assert_eq!(rows[DYNAMIC_BASE][3],rows[DYNAMIC_BASE+3][3]);assert_eq!(rows[DYNAMIC_BASE+2][0],rows[DYNAMIC_BASE+5][0]);
  assert!((rows[DYNAMIC_BASE+5][1]/rows[DYNAMIC_BASE+2][1]-1.5).abs()<1e-6);
 }
}
