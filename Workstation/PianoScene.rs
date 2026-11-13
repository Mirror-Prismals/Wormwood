//! Dynamic data for the separate piano room. Key planes come from the Blender
//! model, not a keyboard texture. Coordinates share the editor's logical pixels.
use serde_json::Value;
use std::path::Path;
pub(crate) const BASE: usize = super::timeline_scene::END;
pub(crate) const SPECTRAL: usize = BASE + 2048;
pub(crate) const END: usize = SPECTRAL + 4096;
pub(crate) const GPU_END: usize = GPU_BASE + END - BASE;
pub(crate) const GPU_BASE: usize = super::fader_components::CAP_BASE
    + super::fader_components::CAP_EDGE * super::fader_components::CAP_EDGE;
const _: () = assert!(GPU_BASE == 4_041_216);
const KEYS: usize = BASE + 16;
const PLANES: usize = BASE + 416;
const TEXT: usize = BASE + 512;
pub(crate) fn append(rows: &mut Vec<[f32; 4]>, root: &Path, packet: &Value) -> Result<(), String> {
    rows.resize(END, [0.; 4]);
    rows[BASE..END].fill([0.; 4]);
    // Dedicated drum workspace marker; no keyboard or piano toolbar geometry.
    rows[BASE][3] = if packet["drum_workspace"]==true {1.}else{0.};
    if packet["active"] != true {
        return Ok(());
    }
    let array = |v: &Value| -> Result<[f64; 4], String> {
        let a: Vec<f64> = v
            .as_array()
            .ok_or("piano bounds missing")?
            .iter()
            .map(|x| x.as_f64().unwrap_or(f64::NAN))
            .collect();
        if a.len() != 4 || !a.iter().all(|x| x.is_finite()) {
            return Err("invalid piano bounds".into());
        }
        Ok([a[0], a[1], a[2], a[3]])
    };
    let canvas = array(&packet["canvas"])?;
    let field = array(&packet["field"])?;
    let w = canvas[2];
    let h = canvas[3];
    if w <= 0. || h <= 0. {
        return Err("invalid piano viewport".into());
    }
    let mpp = 1.3 / h;
    rows[BASE] = [1., w as f32, h as f32, 0.];
    // Latched toolbar selection for the persistent baked button compositor.
    rows[BASE + 6][0] = match packet["mode"].as_str() {
        Some("paint") => 2., Some("arp") => 9., _ => 1.,
    };
    rows[BASE + 1] = field.map(|x| x as f32);
    if let Some(grid)=packet["spectral_grid"].as_array().filter(|a|a.len()==128*128) {
        rows[BASE+7]=[1.,128.,128.,(GPU_BASE+SPECTRAL-BASE)as f32];
        for (i,value) in grid.iter().enumerate() {rows[SPECTRAL+i/4][i%4]=value.as_f64().unwrap_or(0.).clamp(0.,1.) as f32;}
    }
    let low = packet["lowest_pitch"].as_f64().unwrap_or(54.);
    let count = packet["pitches_per_height"].as_f64().unwrap_or(12.);
    let physical_low = super::piano_key_layout::physical(low);
    let step = field[3] / (super::piano_key_layout::physical(low + count) - physical_low);
    let edo = packet["edo"].as_u64().unwrap_or(12) as u16;
    let scale = packet["scale"].as_u64().unwrap_or(0) as u8;
    let root_key = packet["root"].as_u64().unwrap_or(0) as u8;
    rows[BASE + 2] = [low as f32, count as f32, edo as f32, 0.];
    rows[BASE + 3] = [KEYS as f32, PLANES as f32, TEXT as f32, 0.];
    let model: Value = serde_json::from_slice(
        &std::fs::read(root.join("Art/AudioWorkstation/PianoKeyboard/PianoKeyboard_runtime.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let planes = model["planes"]
        .as_array()
        .filter(|p| p.len() <= 64 && !p.is_empty())
        .ok_or("invalid piano key planes")?;
    for (i, p) in planes.iter().enumerate() {
        rows[PLANES + i] = array(p)?.map(|x| x as f32);
    }
    let mut key_count = 0;
    let mut text_count = 0;
    let world = |x: f64, y: f64| [(x - w * 0.5) * mpp, (h * 0.5 - y) * mpp];
    let mut add_key =
        |x: f64, y: f64, width: f64, height: f64, z: f64, depth: f64, color: [f64; 3], key: f64| {
            if key_count >= 132 {
                return;
            }
            let at = KEYS + key_count * 3;
            let p = world(x, y);
            rows[at] = [p[0] as f32, p[1] as f32, z as f32, key as f32];
            rows[at + 1] = [
                (width * mpp) as f32,
                (height * mpp) as f32,
                depth as f32,
                0.,
            ];
            rows[at + 2] = [color[0] as f32, color[1] as f32, color[2] as f32, 0.];
            key_count += 1;
        };
    let kw = packet["keyboard_width"].as_f64().unwrap_or(130.);
    let x0 = 10.;
    let usable = kw - 12.;
    add_key(
        kw * 0.5,
        field[1] + field[3] * 0.5,
        kw,
        field[3] + 14.,
        -0.025,
        0.03,
        [0.04, 0.022, 0.014],
        -1.,
    );
    add_key(
        5.,
        field[1] + field[3] * 0.5,
        10.,
        field[3] + 18.,
        0.002,
        0.055,
        [0.075, 0.036, 0.019],
        -1.,
    );
    for key in 0u8..=127 {
        let black = super::piano_tuning::black(key);
        let p = f64::from(key);
        let y = field[1] + field[3]
            - (super::piano_key_layout::physical(p + 0.5) - physical_low) * step;
        if y + step < field[1] || y - step > field[1] + field[3] {
            continue;
        }
        let height = super::piano_key_layout::height(key);
        let mut color = if black {
            [0.008, 0.009, 0.012]
        } else {
            [0.78, 0.75, 0.65]
        };
        if !black && edo != 12 {
            let hue = ((p - 60.).rem_euclid(f64::from(edo))) / f64::from(edo);
            for (i, c) in color.iter_mut().enumerate() {
                *c = 0.22
                    + 0.52 * (0.5 + 0.5 * (std::f64::consts::TAU * (hue - i as f64 / 3.)).cos());
            }
        }
        if edo == 12 && !super::piano_tuning::in_scale(key, scale, root_key) {
            for c in &mut color {
                *c *= 0.32;
            }
        }
        let pressed = packet["preview_key"].as_u64() == Some(u64::from(key));
        if pressed {
            for c in &mut color {
                *c *= 0.7;
            }
        }
        let width = usable
            * if black {
                super::piano_key_layout::BLACK_LENGTH
            } else {
                1.
            };
        let yy = y;
        let top = yy - height * step * 0.5;
        let bottom = yy + height * step * 0.5;
        let clipped_top = top.max(field[1]);
        let clipped_bottom = bottom.min(field[1] + field[3]);
        if clipped_bottom <= clipped_top {
            continue;
        }
        add_key(
            x0 + width * 0.5,
            (clipped_top + clipped_bottom) * 0.5,
            width,
            (clipped_bottom - clipped_top).max(0.1),
            if black { 0.014 } else { -0.002 } - if pressed { 0.004 } else { 0. },
            if black { 0.023 } else { 0.018 },
            color,
            p,
        );
    }
    rows[BASE + 4] = [key_count as f32, planes.len() as f32, 0., 0.];
    for (i,value) in packet["control_rects"].as_array().into_iter().flatten().take(18).enumerate(){rows[BASE+1980+i]=array(value)?.map(|v|v as f32);}
    rows[BASE+8][0]=packet["labels"].as_array().map_or(0,Vec::len)as f32;
    let mut label = |s: &str, x: f64, y: f64, font: f64, color: [f32; 3]| {
        if text_count >= 80 {
            return;
        }
        let at = TEXT + text_count * 18;
        let p = world(x, y);
        let chars: Vec<u8> = s
            .bytes()
            .take(60)
            .map(|c| if (32..=126).contains(&c) { c } else { b' ' })
            .collect();
        rows[at] = [
            p[0] as f32,
            p[1] as f32,
            (font * mpp) as f32,
            chars.len() as f32,
        ];
        rows[at + 1] = [color[0], color[1], color[2], 0.];
        for (i, c) in chars.iter().enumerate() {
            rows[at + 2 + i / 4][i % 4] = *c as f32;
        }
        text_count += 1;
    };
    let controls=packet["control_rects"].as_array();
    for (i,value) in packet["labels"].as_array().into_iter().flatten().enumerate() {
        let bounds=controls.and_then(|r|r.get(i)).map(&array).transpose()?.unwrap_or([0.,0.,100.,34.]);
        let text=value.as_str().unwrap_or("");let font=13f64.min((w/((w/106.).floor().clamp(4.,9.))-12.)/(text.len().max(1)as f64*0.55));
        label(text,bounds[0]+(bounds[2]-text.len()as f64*font*0.55)*0.5,bounds[1]-6.,font,[0.85,0.88,0.9]);
    }
    for (i,value) in packet["control_headings"].as_array().into_iter().flatten().enumerate(){
        let bounds=controls.and_then(|r|r.get(i)).map(&array).transpose()?.unwrap_or([0.,0.,100.,34.]);
        let text=value.as_str().unwrap_or("");let font=10f64.min((w/((w/106.).floor().clamp(4.,9.))-12.)/(text.len().max(1)as f64*0.55));
        label(text,bounds[0]+(bounds[2]-text.len()as f64*font*0.55)*0.5,bounds[1]-23.,font,[0.86,0.89,0.92]);
    }
    label(
        packet["title"].as_str().unwrap_or("PIANO ROLL"),
        field[0],
        field[1] - 70.,
        10.,
        [0.40, 0.45, 0.50],
    );
    for key in 0u8..=127 {
        if key % 12 != 0 {
            continue;
        }
        let y = field[1] + field[3]
            - (super::piano_key_layout::physical(f64::from(key) + 0.5) - physical_low) * step;
        if y < field[1] + 8. || y > field[1] + field[3] - 8. {
            continue;
        }
        let name = if edo == 12 {
            format!("C{}", key as i32 / 12 - 1)
        } else {
            let semitones = super::piano_tuning::semitones(key, edo);
            format!("{:0.0}c", (semitones - 60.) * 100.)
        };
        label(&name, kw * 0.60, y + 3., 9., [0.12, 0.12, 0.13]);
    }
    let first = packet["first_quarter"].as_f64().unwrap_or(0.);
    let span = packet["quarters_per_width"]
        .as_f64()
        .unwrap_or(16.)
        .max(0.001);
    let stride = (span / 16.).ceil().max(1.);
    for i in 0..18 {
        let q = (first / stride).ceil() * stride + i as f64 * stride;
        if q > first + span {
            break;
        }
        let x = field[0] + (q - first) / span * field[2];
        label(
            &format!("{}", q + 1.),
            x,
            field[1] - 9.,
            9.,
            [0.42, 0.46, 0.50],
        );
    }
    label(
        "Wheel: pitch | Shift-wheel: time | Alt-wheel: pitch zoom",
        18.,
        h - 15.,
        12.,
        [0.40, 0.44, 0.48],
    );
    label(
        packet["notice"]
            .as_str()
            .unwrap_or("Cmd-wheel: time zoom | Controls: click/wheel; Shift reverses"),
        18.,
        h - 30.,
        12.,
        [0.46, 0.49, 0.52],
    );
    rows[BASE + 3][3] = text_count as f32;
    // The scene layer supplies the selected plain/camera backdrop.
    rows[9][1] = 0.;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_packet_has_real_bevels_and_black_accidentals_in_both_tunings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for edo in [12, 19] {
            let packet = serde_json::json!({"active":true,"canvas":[0.,0.,960.,600.],"field":[152.,82.,784.,480.],"keyboard_width":134.,"lowest_pitch":58.,"pitches_per_height":17.,"edo":edo,"labels":[]});
            let mut rows = vec![[0.; 4]; BASE];
            append(&mut rows, root, &packet).unwrap();
            assert_eq!(rows.len(), END);
            assert_eq!(rows[BASE + 4][1], 26.);
            let count = rows[BASE + 4][0] as usize;
            assert!(count > 12);
            let mut colored = false;
            for i in 2..count {
                let at = KEYS + i * 3;
                let key = rows[at][3] as u8;
                let rgb = rows[at + 2];
                if super::super::piano_tuning::black(key) {
                    assert!(rgb[..3].iter().all(|x| *x < 0.02));
                } else if edo != 12 {
                    colored |= (rgb[0] - rgb[1]).abs() > 0.1;
                }
            }
            assert!(edo == 12 || colored);
        }
    }
}

#[cfg(test)]mod matching_geometry_tests{
 use super::*;
 #[test]fn bubbles_match_real_key_centers_and_front_heights(){
  use crate::workstation::{piano_roll::{Editor,Rect},panel_layout::Viewport,piano_workspace::Workspace,bubble_notes};
  let snapshot=serde_json::json!({"revision":1,"tracks":[{"id":"m","kind":"midi"}],"clips":[{"id":"c","track":"m","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":16.,"meter":{"numerator":4,"denominator":4},"notes":[{"id":"white","quarter":1.,"length_quarters":1.,"pitch":60,"velocity":0.8},{"id":"black","quarter":3.,"length_quarters":1.,"pitch":61,"velocity":0.8},{"id":"white2","quarter":5.,"length_quarters":1.,"pitch":64,"velocity":0.8}]}]});
  let mut editor=Editor::default();editor.open(&snapshot,"c").unwrap();let mut workspace=Workspace::default();workspace.focus_clip(&snapshot,"c");let rect=Rect{left:0.,top:0.,width:960.,height:600.};let view=Viewport{width:960.,height:600.,canvas_scale:1.};let room=workspace.scene(&editor,rect);let bubbles=workspace.presentation(&editor,rect,view);let mut rows=vec![[0.;4];BASE];let root=Path::new(env!("CARGO_MANIFEST_DIR"));
  bubble_notes::Kit::load(root).unwrap().append(&mut rows,&bubbles).unwrap();append(&mut rows,root,&room).unwrap();assert_eq!(rows[237][3],2.);
  for note in bubbles["notes"].as_array().unwrap(){let pitch=note["pitch"].as_u64().unwrap()as f32;let key=(0..rows[BASE+4][0]as usize).map(|i|KEYS+i*3).find(|at|rows[*at][3]==pitch).unwrap();assert!((f64::from(rows[key][1])-note["y"].as_f64().unwrap()).abs()<1e-6);assert!((f64::from(rows[key+1][1])-note["height_m"].as_f64().unwrap()).abs()<1e-6);}
  assert_eq!(bubbles["notes"][0]["height_m"],bubbles["notes"][2]["height_m"]);assert!(bubbles["notes"][1]["height_m"].as_f64().unwrap()<bubbles["notes"][0]["height_m"].as_f64().unwrap());
 }
 #[test]fn room_does_not_clear_camera_composition_flag(){let root=Path::new(env!("CARGO_MANIFEST_DIR"));let mut rows=vec![[0.;4];BASE];rows[9][2]=1.;append(&mut rows,root,&serde_json::json!({"active":true,"canvas":[0.,0.,960.,600.],"field":[152.,82.,784.,480.],"keyboard_width":134.,"lowest_pitch":58.,"pitches_per_height":17.,"labels":[]})).unwrap();assert_eq!(rows[9][2],1.);}
}
