//! Bounded bottom-page packet. Existing authored Wide/Mini meshes remain
//! immutable. The source-data display is a thin native surface, not a UI overlay.
use super::{engraving, panel_layout::Viewport};
use serde_json::Value;
// Reserved gap after fader data and before page-zero I/O. The timeline ends
// at the frame boundary; appending here instead would overflow on every open.
pub(crate) const BASE: usize = super::fader_components::BASE + super::fader_components::ROWS;
pub(crate) const CONTROLS: usize = 12;
pub(crate) const POSES: usize = BASE + 12;
pub(crate) const LABELS: usize = POSES + CONTROLS * 3;
pub(crate) const WAVE: usize = LABELS + CONTROLS * 4;
pub(crate) const SPECTRUM: usize = WAVE + 32;
pub(crate) const MARKERS: usize = SPECTRUM + 128;
pub(crate) const MASKS: usize = MARKERS + 16;
pub(crate) const FADERS: usize = MASKS + 20;
pub(crate) const FADER_ROWS: usize = 24;
pub(crate) const END: usize = FADERS + FADER_ROWS;
const _: () = assert!(END <= super::track_io_scene::HEADER);
fn finite(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|v| v.is_finite() && v.abs() < 1e9)
        .ok_or_else(|| "invalid bottom-panel coordinate".into())
}
fn rect(v: &Value) -> Result<[f64; 4], String> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 4)
        .ok_or("bottom rect needs four coordinates")?;
    Ok([
        finite(&a[0])?,
        finite(&a[1])?,
        finite(&a[2])?,
        finite(&a[3])?,
    ])
}
// Preserve Cardinal's legacy distinction: drag hit tests use the outer view,
// but clip-editor pixels and normalized overlays use its 8-point inset.
fn display_rect(packet: &Value) -> Result<[f64; 4], String> {
    let mut r = rect(&packet["view"])?;
    if packet["page"] == 3 && r[2] > 0. && r[3] > 0. {
        r = [r[0] + 8., r[1] + 8., (r[2] - 16.).max(1.), (r[3] - 16.).max(1.)];
    }
    Ok(r)
}

// History carries separate channel envelopes; clip previews carry one mono envelope.
fn wave_pair(v:&Value,page:u64,channel:usize)->Result<[f32;2],String>{
 let p=if page==2&&v[0].is_array(){&v[channel]}else{v};
 let a=p.as_array().filter(|a|a.len()==2).ok_or("waveform envelope needs min/max")?;
 Ok([finite(&a[0])? as f32,finite(&a[1])? as f32])
}
fn text_label(label: &str) -> &str {
    match label {
        "chevron_left" => "<",
        "chevron_right" => ">",
        "keyboard_double_arrow_left" => "<<",
        "keyboard_double_arrow_right" => ">>",
        _ => label,
    }
}
pub(crate) fn append(
    rows: &mut [[f32; 4]],
    view: Viewport,
    packet: &Value,
    assets: [([usize; 3], [[f64; 3]; 2]); 2],
    depth: f64,
) -> Result<(), String> {
    if rows.len() < END || END > engraving::BASE + engraving::ROWS {
        return Err("bottom packet exceeds reserved frame rows".into());
    }
    if !depth.is_finite() {
        return Err("invalid bottom mounting depth".into());
    }
    let mut data = vec![[0f32; 4]; END - BASE];
    let mut count = 0;
    if packet.is_null() {
        rows[BASE..END].copy_from_slice(&data);
        rows[255][0] = 0.;
        rows[255][1] = 0.;
        rows[255][2] = 0.;
        return Ok(());
    }
    let panel = rect(&packet["panel"])?;
    let viewrect = display_rect(packet)?;
    if panel[2] <= 0. || panel[3] <= 0. {
        rows[BASE..END].copy_from_slice(&data);
        rows[255][0] = 0.;
        rows[255][1] = 0.;
        rows[255][2] = 0.;
        return Ok(());
    }
    if !view.height.is_finite() || view.height <= 0. {
        return Err("invalid bottom viewport".into());
    }
    let mpp = 1.3 / view.height;
    let world = |x: f64, y: f64| [(x - view.width * 0.5) * mpp, (view.height * 0.5 - y) * mpp];
    let bounds = |r: [f64; 4]| {
        let a = world(r[0], r[1] + r[3]);
        let b = world(r[0] + r[2], r[1]);
        [a[0] as f32, a[1] as f32, b[0] as f32, b[1] as f32]
    };
    data[4][3] = if packet["page"]==3 && (packet["editor_tab"]=="Waveform" || packet["editor_tab"]=="Warp") {1.}else{0.};
    if packet["page"]==3 && packet["editor_tab"]=="Waveform" && !packet["preview"]["id"].is_null() {
        data[10] = [packet["wave_view"][0].as_f64().unwrap_or(0.) as f32, packet["wave_view"][1].as_f64().unwrap_or(1.) as f32,
            packet["wave_loop"][0].as_f64().unwrap_or(0.) as f32, packet["wave_loop"][1].as_f64().unwrap_or(0.) as f32];
        data[11][0] = 1.;
    }
    data[0] = bounds(panel);
    data[1] = bounds(viewrect);
    data[2] = [
        mpp as f32,
        packet["page"].as_u64().unwrap_or(0) as f32,
        0.,
        depth as f32,
    ];
    data[8][2] = (depth - 0.001) as f32;
    data[9][2] = (depth + 0.0011) as f32;
    let controls: Vec<_> = packet["controls"]
        .as_array()
        .ok_or("bottom controls required")?
        .iter()
        .filter(|c| !c["id"].as_str().is_some_and(|id| id.starts_with("Fader(") || id.starts_with("SelectBus(") || id.starts_with("SelectTrack(")))
        .collect();
    if controls.len() > CONTROLS {
        return Err("bottom control packet exceeds 12 authored instances".into());
    }
    for c in controls {
        let r = rect(&c["rect"])?;
        if r[2] <= 0. || r[3] <= 0. {
            continue;
        }
        let label = text_label(c["label"].as_str().ok_or("bottom label required")?);
        if label.len() > 16 || !label.is_ascii() {
            return Err("bottom engraving needs at most 16 ASCII characters".into());
        }
        let index = if r[2] > r[3] * 1.7 { 0 } else { 1 };
        let (mut mesh, [min, max]) = assets[index];
        let selected_page = c["active"] == true && (packet["page"] == 3 || c["id"].as_str().is_some_and(|id| id.starts_with("SelectPage(") || id.starts_with("SelectPluginTrack(")));
        // Same persistent colorless ruby artwork as latched mute; no live optics.
        if selected_page { mesh[2] = 12704; }
        if c["id"]=="ClipOutput" {mesh[2]=match c["label"].as_str(){Some("TO L")=>12743+index,Some("TO FF")=>12783+index,Some("TO S")=>12823+index,_=>mesh[2]};}
        let scale = (r[2] * mpp / (max[0] - min[0])).min(r[3] * mpp / (max[1] - min[1]));
        let center = world(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        let pressure = c["pressure"]
            .as_f64()
            .filter(|v| v.is_finite())
            .unwrap_or(0.)
            .clamp(0., 0.5);
        // Preserve the control state separately from its animated panel transform.
        data[FADERS - BASE + count / 4][count % 4] = (pressure + if c["active"] == true { 1. } else { 0. }) as f32;
        let at = POSES - BASE + count * 3;
        data[at] = [
            center[0] as f32,
            center[1] as f32,
            (depth - min[2] * scale + 0.0005 - pressure * mpp * 2.
                + if selected_page { -0.75 * mpp } else if c["active"] == true { 2. * mpp } else { 0. }) as f32,
            scale as f32,
        ];
        data[8][2] = data[8][2].min(data[at][2] + (min[2] * scale) as f32);
        data[9][2] = data[9][2].max(data[at][2] + (max[2] * scale) as f32);
        data[at + 1] = [
            mesh[0] as f32,
            mesh[1] as f32,
            mesh[2] as f32,
            if c["enabled"] == false { -2. } else { 0. },
        ];
        // Uniform mesh scaling; font shrinks to fit original long control labels.
        let font = (9. * mpp).min(r[2] * mpp / (label.len().max(1) as f64 * 0.58));
        data[at + 2] = [
            (max[0] - min[0]) as f32,
            (max[1] - min[1]) as f32,
            (max[2] - min[2]) as f32,
            font as f32,
        ];
        for (i, b) in label.bytes().enumerate() {
            data[LABELS - BASE + count * 4 + i / 4][i % 4] = b as f32;
        }
        count += 1;
    }
    let page = packet["page"].as_u64().unwrap_or(0);
    let editor = packet["editor_tab"].as_str().unwrap_or("");
    let waveform = if page == 2 {
        &packet["loopback"]["waveform"]
    } else {
        &packet["preview"]["waveform"]
    };
    if let Some(wave) = waveform.as_array().filter(|w| !w.is_empty()) {
        for x in 0..64 {
            let a = x * wave.len() / 64;
            let b = ((x + 1) * wave.len() / 64).max(a + 1).min(wave.len());
            let mut lo = 1f32;
            let mut hi = -1f32;
            for f in &wave[a..b] {
                let pair=wave_pair(f,page,0)?;
                lo=lo.min(pair[0]);hi=hi.max(pair[1]);
            }
            data[WAVE - BASE + x / 2][(x % 2) * 2] = lo.clamp(-1., 1.);
            data[WAVE - BASE + x / 2][(x % 2) * 2 + 1] = hi.clamp(-1., 1.);
        }
        data[2][2] = 1.;
    }
    let spectral = &packet["preview"]["spectrogram"];
    if page == 3 && editor == "Spectral" {
        if let (Some(columns), Some(bins), Some(values)) = (
            spectral["columns"].as_u64(),
            spectral["bins"].as_u64(),
            spectral["intensity"].as_array(),
        ) {
            if columns == 0
                || bins == 0
                || columns > 1000
                || bins > 160
                || values.len() != columns as usize * bins as usize
            {
                return Err("invalid bottom spectral grid".into());
            }
            for y in 0..16 {
                for x in 0..32 {
                    let sx = (x as f64 * (columns - 1) as f64 / 31.).round() as usize;
                    let sy = (y as f64 * (bins - 1) as f64 / 15.).round() as usize;
                    let at = y * 32 + x;
                    data[SPECTRUM - BASE + at / 4][at % 4] =
                        (finite(&values[sy * columns as usize + sx])? as f32).clamp(0., 1.);
                }
            }
            data[2][2] = 2.;
        }
    }
    if let Some(s) = packet["spectral_selection"]
        .as_array()
        .filter(|a| a.len() == 4)
    {
        let coords = [
            finite(&s[0])?,
            finite(&s[1])?,
            finite(&s[2])?,
            finite(&s[3])?,
        ]
        .map(|v| v.clamp(0., 1.) as f32);
        data[3] = [
            coords[0].min(coords[1]),
            coords[0].max(coords[1]),
            coords[2].min(coords[3]),
            coords[2].max(coords[3]),
        ];
        data[4][0] = 1.;
    }
    if page==3 && editor=="Waveform" {
        if let Some(s)=packet["wave_selection"].as_array().filter(|s|s.len()==2){let a=finite(&s[0])?.clamp(0.,1.) as f32;let b=finite(&s[1])?.clamp(0.,1.) as f32;data[3]=[a.min(b),a.max(b),0.,1.];data[4][0]=1.;}
    }
    // Source selection is normalized against the displayed absolute history window.
    if page == 2 {
        let history = &packet["loopback"];
        if let (Some(selection), Some(first), Some(end)) = (
            history["selection"].as_array(),
            history["oldest_frame"].as_u64(),
            history["head_frame"].as_u64(),
        ) {
            if end > first && selection.len() == 2 {
                data[3] = [
                    ((selection[0].as_u64().unwrap_or(first).saturating_sub(first)) as f64
                        / (end - first) as f64)
                        .clamp(0., 1.) as f32,
                    ((selection[1].as_u64().unwrap_or(first).saturating_sub(first)) as f64
                        / (end - first) as f64)
                        .clamp(0., 1.) as f32,
                    0.,
                    1.,
                ];
                data[4][0] = 1.;
            }
        }
    }
    let preview = &packet["preview"];
    let source_start = preview["source_start_frame"].as_f64().unwrap_or(0.);
    let frames = preview["clip_frames"]
        .as_f64()
        .or_else(|| preview["frames"].as_f64())
        .unwrap_or(0.);
    let rate = preview["sample_rate"].as_f64().unwrap_or(48000.);
    if page == 3 && frames > 0. {
        if editor == "Warp" {
            if let Some(entry) = preview["warp"]
                .as_array()
                .and_then(|a| a.iter().find(|e| e["id"] == preview["id"]))
            {
                if let Some(markers) = entry["markers"].as_array() {
                    let mut n = 0;
                    for m in markers {
                        let x = (finite(&m["source_frame"])? - source_start) / frames;
                        if (0. ..=1.).contains(&x) && n < 64 {
                            data[MARKERS - BASE + n / 4][n % 4] = x as f32;
                            n += 1;
                        }
                    }
                    data[4][1] = n as f32;
                }
            }
        }
        if editor == "Spectral" {
            if let Some(masks) = preview["spectral_masks"].as_array() {
                let mut n = 0;
                let norm = |hz: f64| {
                    ((hz.max(30.) / 30.).ln() / ((rate * 0.5).max(30.0001) / 30.).ln())
                        .clamp(0., 1.)
                };
                for m in masks.iter().take(16) {
                    let x0 = (finite(&m["source_start_frame"])? - source_start) / frames;
                    let x1 = (finite(&m["source_end_frame"])? - source_start) / frames;
                    data[MASKS - BASE + n] = [
                        x0.clamp(0., 1.) as f32,
                        x1.clamp(0., 1.) as f32,
                        norm(finite(&m["low_hz"])?) as f32,
                        norm(finite(&m["high_hz"])?) as f32,
                    ];
                    data[MASKS - BASE + 16 + n / 4][n % 4] =
                        m["feather"].as_f64().unwrap_or(0.12).clamp(0., 0.5) as f32;
                    n += 1;
                }
                data[4][2] = n as f32;
            }
        }
    }
    if data.iter().flatten().any(|v| !v.is_finite()) {
        return Err("nonfinite bottom frame".into());
    }
    rows[BASE..END].copy_from_slice(&data);
    rows[255][0] = BASE as f32;
    rows[255][1] = 1.;
    rows[255][2] = count as f32;
    Ok(())
}

/// Full preview span supplied by the authored-control packet allocator. Three
/// 8-bit intensities fit exactly in each f32 integer, twelve per row. This keeps
/// all 240×100 source bins, while authored mesh addresses remain unchanged.
// Reserved together with split fader geometry: mesh end 40991, editor40992..43152,
// mixer43152..43664, I/O44288..44592, instance BVH44812..45312.
pub(crate) const FULL_PREVIEW_BASE: usize = 40_992;
pub(crate) const PREVIEW_ROWS: usize = 2160;
pub(crate) fn append_full_preview(
    rows: &mut [[f32; 4]],
    packet: &Value,
    preview_base: usize,
) -> Result<(), String> {
    if preview_base
        .checked_add(PREVIEW_ROWS)
        .is_none_or(|end| end > rows.len())
        || preview_base < 256
        || preview_base + PREVIEW_ROWS > BASE
    {
        return Err("full bottom preview is outside its reserved control span".into());
    }
    let preview_rate = if packet["preview"]["sample_rate"].is_null() {
        48000.
    } else {
        let rate = finite(&packet["preview"]["sample_rate"])?;
        if !(8000. ..=192000.).contains(&rate) {
            return Err("invalid bottom preview sample rate".into());
        }
        rate
    };
    let feather = if packet["feather"].is_null() {
        0.12
    } else {
        finite(&packet["feather"])?.clamp(0., 0.45)
    };
    let mut packed = vec![[0f32; 4]; PREVIEW_ROWS];
    let mut labels: Vec<(f64, f64, String)> = Vec::new();
    let mut header = [0f32; 4];
    if !packet.is_null() {
        let page = packet["page"].as_u64().unwrap_or(0);
        let wave = if page == 2 {
            &packet["loopback"]["waveform"]
        } else {
            &packet["preview"]["waveform"]
        };
        if let Some(wave) = wave.as_array().filter(|w| !w.is_empty()) {
            let columns = wave.len().min(240);
            for x in 0..columns {
                let a = x * wave.len() / columns;
                let b = ((x + 1) * wave.len() / columns).max(a + 1).min(wave.len());
                let mut lo = 1f32;
                let mut hi = -1f32;
                for f in &wave[a..b] {
                    let pair=wave_pair(f,page,0)?;
                    lo=lo.min(pair[0]);hi=hi.max(pair[1]);
                }
                packed[x / 2][(x % 2) * 2] = lo.clamp(-1., 1.);
                packed[x / 2][(x % 2) * 2 + 1] = hi.clamp(-1., 1.);
            }
            header[1] = columns as f32;
            if page==2 && packet["loopback"]["source"]=="PLAYER AUDIO" {
                for x in 0..columns {
                    let a=x*wave.len()/columns;let b=((x+1)*wave.len()/columns).max(a+1).min(wave.len());
                    let mut lo=1f32;let mut hi= -1f32;
                    for f in &wave[a..b]{let pair=wave_pair(f,page,1)?;lo=lo.min(pair[0]);hi=hi.max(pair[1]);}
                    packed[120+x/2][(x%2)*2]=lo.clamp(-1.,1.);packed[120+x/2][(x%2)*2+1]=hi.clamp(-1.,1.);
                }
                header[2]=2.;
            }
        }
        let spectrum = &packet["preview"]["spectrogram"];
        if page == 3 && packet["editor_tab"] == "Spectral" {
            if let (Some(columns), Some(bins), Some(values)) = (
                spectrum["columns"].as_u64(),
                spectrum["bins"].as_u64(),
                spectrum["intensity"].as_array(),
            ) {
                if columns == 0
                    || bins == 0
                    || columns > 1000
                    || bins > 160
                    || values.len() != columns as usize * bins as usize
                {
                    return Err("invalid full spectral grid".into());
                }
                let width = (columns as usize).min(240);
                let height = (bins as usize).min(100);
                let mut words = vec![0u32; (width * height).div_ceil(3)];
                for y in 0..height {
                    for x in 0..width {
                        let sx = if width == 1 {
                            0
                        } else {
                            x * (columns as usize - 1) / (width - 1)
                        };
                        let sy = if height == 1 {
                            0
                        } else {
                            y * (bins as usize - 1) / (height - 1)
                        };
                        let v = (finite(&values[sy * columns as usize + sx])?.clamp(0., 1.) * 255.)
                            .round() as u32;
                        let at = y * width + x;
                        words[at / 3] |= v << ((at % 3) * 8);
                    }
                }
                for (i, word) in words.into_iter().enumerate() {
                    packed[120 + i / 4][i % 4] = word as f32;
                }
                header[2] = width as f32;
                header[3] = height as f32;
            }
        }
        header[0] = preview_base as f32;
    }
    if packet["page"] == 3 && packet["editor_tab"] == "Spectral" {
        let area = display_rect(packet)?;
        let rate = packet["preview"]["sample_rate"].as_f64().unwrap_or(48000.);
        let frames = packet["preview"]["clip_frames"]
            .as_f64()
            .or_else(|| packet["preview"]["frames"].as_f64())
            .unwrap_or(0.);
        let width = area[2].max(1.);
        let height = area[3].max(1.);
        let norm = |hz: f64| {
            ((hz.max(30.) / 30.).ln() / ((rate * 0.5).max(30.0001) / 30.).ln()).clamp(0., 1.)
        };
        for hz in [100., 1000., 5000., 10000., 20000.] {
            if hz <= rate * 0.5 * 1.001 {
                labels.push((
                    5. / width,
                    norm(hz) + 3. / height,
                    if hz >= 1000. {
                        format!("{:.1} kHz", hz / 1000.)
                    } else {
                        format!("{:.0} Hz", hz)
                    },
                ));
            }
        }
        for t in [0.25, 0.5, 0.75] {
            labels.push((
                t + 3. / width,
                7. / height,
                format!("{:.2}s", frames / rate * t),
            ));
        }
        if let Some(masks) = packet["preview"]["spectral_masks"].as_array() {
            let start = packet["preview"]["source_start_frame"]
                .as_f64()
                .unwrap_or(0.);
            for m in masks.iter().take(16) {
                if labels.len() >= 20 {
                    break;
                }
                let xa =
                    ((finite(&m["source_start_frame"])? - start) / frames.max(1.)).clamp(0., 1.);
                let xb = ((finite(&m["source_end_frame"])? - start) / frames.max(1.)).clamp(0., 1.);
                let lo = norm(finite(&m["low_hz"])?);
                let hi = norm(finite(&m["high_hz"])?);
                if (xb - xa) * width > 50. && (hi - lo) * height > 17. {
                    labels.push((
                        xa + 4. / width,
                        hi - 11. / height,
                        format!("{:.0} dB", m["attenuation_db"].as_f64().unwrap_or(0.)),
                    ));
                }
            }
        }
        for (i, (x, y, label)) in labels.iter().enumerate() {
            let text = label.as_bytes();
            if text.len() > 12 {
                return Err("bottom surface label exceeds 12 characters".into());
            }
            packed[2120 + i * 2] = [*x as f32, *y as f32, 9., text.len() as f32];
            let mut words = [0u32; 4];
            for (j, byte) in text.iter().enumerate() {
                words[j / 3] |= u32::from(*byte) << ((j % 3) * 8);
            }
            packed[2121 + i * 2] = words.map(|v| v as f32);
        }
    }
    rows[preview_base..preview_base + PREVIEW_ROWS].copy_from_slice(&packed);
    rows[BASE + 5] = header;
    rows[BASE + 6][0] = labels.len() as f32;
    rows[BASE + 7] = [preview_rate as f32, 0., feather as f32, 0.];
    Ok(())
}

#[cfg(test)]mod retrospective_packet_tests{
 use super::*;use serde_json::json;
 #[test]fn clip_editor_exports_white_latched_buttons_transparency_and_bar_ranges(){
  let mut packet=json!({"page":3,"editor_tab":"Waveform","panel":[0.,0.,800.,200.],"view":[100.,80.,600.,100.],"controls":[{"id":"WarpToggle","label":"WARP ON","rect":[100.,10.,100.,28.],"active":true,"enabled":true}],"wave_view":[0.25,0.75],"wave_loop":[0.3,0.6],"preview":{"id":"c"}});
  let mut rows=vec![[0.;4];super::super::engraving::BASE+super::super::engraving::ROWS];
  let assets=[([256,257,258],[[0.,0.,0.],[1.,1.,1.]]);2];let view=Viewport{width:1280.,height:720.,canvas_scale:1.};
  append(&mut rows,view,&packet,assets,0.05).unwrap();
  assert_eq!(rows[POSES+1][2],12704.);assert_eq!(rows[BASE+4][3],1.);assert_eq!(rows[BASE+10],[0.25,0.75,0.3,0.6]);assert_eq!(rows[BASE+11][0],1.);
  packet["editor_tab"]=json!("Warp");append(&mut rows,view,&packet,assets,0.05).unwrap();assert_eq!(rows[BASE+4][3],1.);assert_eq!(rows[BASE+11][0],0.);
 }
 #[test]fn actual_stereo_history_envelopes_reach_both_gpu_channels(){
  use crate::workstation::loopback_history::{History,Source};
  let mut history=History::default();history.configure(Source::Player,8000).unwrap();history.drain(Source::Player,0,64,|_|Some([0.25,-0.75])).unwrap();
  let packet=json!({"page":2,"panel":[0.,0.,800.,200.],"view":[100.,80.,600.,100.],"controls":[],"loopback":{"source":"PLAYER AUDIO","waveform":history.waveform(240)}});
  let mut rows=vec![[0.;4];super::super::engraving::BASE+super::super::engraving::ROWS];
  append(&mut rows,Viewport{width:1280.,height:720.,canvas_scale:1.},&packet,[([256,257,258],[[0.,0.,0.],[1.,1.,1.]]);2],0.05).unwrap();
  append_full_preview(&mut rows,&packet,FULL_PREVIEW_BASE).unwrap();
  assert_eq!(&rows[FULL_PREVIEW_BASE][..2],&[0.25,0.25]);assert_eq!(&rows[FULL_PREVIEW_BASE+120][..2],&[-0.75,-0.75]);assert_eq!(rows[BASE+5][2],2.);
 }
}

#[cfg(test)]mod clip_output_art_tests {
 use super::*;use serde_json::json;
 #[test]fn waveform_output_gem_selects_existing_matrix_colors(){
  let assets=[([256,257,258],[[0.,0.,0.],[1.,1.,1.]]);2];
  let view=Viewport{width:1280.,height:720.,canvas_scale:1.};
  for (label,material) in [("TO L",12743.),("TO S",12823.),("TO FF",12783.),("TO R",258.)]{
   let mut rows=vec![[0.;4];super::super::engraving::BASE+super::super::engraving::ROWS];
   let packet=json!({"page":3,"editor_tab":"Waveform","panel":[0.,0.,800.,200.],"view":[100.,80.,600.,100.],"controls":[{"id":"ClipOutput","label":label,"rect":[100.,10.,100.,28.],"active":false,"enabled":true}]});
   append(&mut rows,view,&packet,assets,0.05).unwrap();assert_eq!(rows[POSES+1][2],material);
  }
 }
}
