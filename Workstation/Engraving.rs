//! Embedded SSLE lettering. Glyph occupancy comes from Blender's actual font
//! meshes; character poses remain separate from the glass and can be rebuilt.
use super::{
    browser, decorative_controls, floor_light,
    panel_layout::{self, Options, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::path::Path;

pub(crate) const BASE: usize = 256 + floor_light::ROWS + decorative_controls::ROWS;
pub(crate) const ROWS: usize = 4096;
pub(crate) const DENSITY_EDGE: usize = 128;
pub(crate) const DENSITY_LEVELS: usize = 8;
pub(crate) const DENSITY_TEXELS_PER_GLYPH: usize = 21846;
pub(crate) const DENSITY_ROWS: usize = 95 * DENSITY_TEXELS_PER_GLYPH / 2;
pub(crate) const DENSITY_BASE: usize = BASE + ROWS + 57 * 57 * 57;
const GLYPH_ROWS: usize = 144;
const ATLAS_ROWS: usize = 95 * GLYPH_ROWS;
const MAX_CHARACTERS: usize = 160;
const MAX_LINES: usize = 16;

struct Glyph {
    codepoint: usize,
    advance_em: f64,
}
#[derive(Clone)]
struct Line {
    text: String,
    font_size_px: f64,
    baseline_px: [f64; 2],
}
#[derive(Clone)]
struct Block {
    panel_index: usize,
    margin_px: f64,
    bbox_px: [f64; 4],
    lines: Vec<Line>,
}
struct Style {
    layer_offsets_em: [f64; 3],
    radius_em: f64,
    layer_shift_cells: [[f64; 2]; 3],
    opacity: f64,
    base_color_linear_rgb: [f64; 3],
    layer_weights: [f64; 3],
    point_jitter_cells: f64,
    point_radius_range: [f64; 2],
    scatter_gain: f64,
}
struct Manifest {
    schema: usize,
    glyph_first: usize,
    glyph_count: usize,
    cell_width: usize,
    cell_height: usize,
    pitch_em: f64,
    origin_em: [f64; 2],
    glyphs: Vec<Glyph>,
    style: Style,
    demo_blocks: Vec<Block>,
}

fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "invalid SSLE numeric field".into())
}
fn integer(v: &Value) -> Result<usize, String> {
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| "invalid SSLE integer field".into())
}
fn array(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or_else(|| "invalid SSLE array".into())
}
fn vector<const N: usize>(v: &Value) -> Result<[f64; N], String> {
    let values = array(v)?;
    if values.len() != N {
        return Err("invalid SSLE vector length".into());
    }
    let mut result = [0.; N];
    for (i, value) in values.iter().enumerate() {
        result[i] = number(value)?;
    }
    Ok(result)
}
impl Manifest {
    fn read(v: &Value) -> Result<Self, String> {
        let glyphs = array(&v["glyphs"])?
            .iter()
            .map(|g| {
                Ok(Glyph {
                    codepoint: integer(&g["codepoint"])?,
                    advance_em: number(&g["advance_em"])?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut demo_blocks = Vec::new();
        for block in array(&v["demo_blocks"])? {
            let lines = array(&block["lines"])?
                .iter()
                .map(|line| {
                    Ok(Line {
                        text: line["text"].as_str().ok_or("invalid SSLE text")?.to_owned(),
                        font_size_px: number(&line["font_size_px"])?,
                        baseline_px: vector(&line["baseline_px"])?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            demo_blocks.push(Block {
                panel_index: integer(&block["panel_index"])?,
                margin_px: number(&block["margin_px"])?,
                bbox_px: vector(&block["bbox_px"])?,
                lines,
            });
        }
        let s = &v["style"];
        let shifts = array(&s["layer_shift_cells"])?;
        if shifts.len() != 3 {
            return Err("SSLE requires three layers".into());
        }
        Ok(Self {
            schema: integer(&v["schema"])?,
            glyph_first: integer(&v["glyph_first"])?,
            glyph_count: integer(&v["glyph_count"])?,
            cell_width: integer(&v["cell_width"])?,
            cell_height: integer(&v["cell_height"])?,
            pitch_em: number(&v["pitch_em"])?,
            origin_em: vector(&v["origin_em"])?,
            glyphs,
            demo_blocks,
            style: Style {
                layer_offsets_em: vector(&s["layer_offsets_em"])?,
                radius_em: number(&s["radius_em"])?,
                layer_shift_cells: [
                    vector(&shifts[0])?,
                    vector(&shifts[1])?,
                    vector(&shifts[2])?,
                ],
                opacity: number(&s["opacity"])?,
                base_color_linear_rgb: vector(&s["base_color_linear_rgb"])?,
                layer_weights: vector(&s["layer_weights"])?,
                point_jitter_cells: number(&s["point_jitter_cells"])?,
                point_radius_range: vector(&s["point_radius_range"])?,
                scatter_gain: number(&s["scatter_gain"])?,
            },
        })
    }
}
pub(crate) struct Kit {
    manifest: Manifest,
    bounds: Vec<Option<[f64; 4]>>,
}

impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("Art/AudioWorkstation/LaserEngraving");
        let document: Value = serde_json::from_slice(
            &std::fs::read(path.join("engraving.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut manifest = Manifest::read(&document)?;
        // Reuse the existing authored alphabet and optical material. Only the
        // left block's words change; its actions share Browser's exact fit.
        if let Some(block) = manifest.demo_blocks.iter_mut().find(|b| b.panel_index == 2) {
            block.margin_px = browser::MARGIN_PX;
            block.bbox_px = browser::spec()?.bbox_px;
            block.lines = browser::LABELS
                .iter()
                .map(|label| Line {
                    text: label.text.to_owned(),
                    font_size_px: label.font_size_px,
                    baseline_px: label.baseline_px,
                })
                .collect();
        }
        let data = std::fs::read(path.join("engraving.bin")).map_err(|e| e.to_string())?;
        let m = &manifest;
        if m.schema != 3
            || m.glyph_first != 32
            || m.glyph_count != 95
            || m.cell_width != 96
            || m.cell_height != 96
            || m.glyphs.len() != 95
            || data.len() != ATLAS_ROWS * 16
            || integer(&document["rows_per_glyph"])? != GLYPH_ROWS
            || integer(&document["words_per_glyph"])? != GLYPH_ROWS * 4
            || integer(&document["bits_per_word"])? != 16
            || document["binary"] != "engraving.bin"
            || document["density_binary"] != "density.bin"
            || !document["density_sha256"]
                .as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            || integer(&document["density_edge"])? != DENSITY_EDGE
            || integer(&document["density_levels"])? != DENSITY_LEVELS
            || integer(&document["density_texels_per_glyph"])? != DENSITY_TEXELS_PER_GLYPH
            || m.pitch_em != 0.0125
            || m.origin_em != [-0.12, -0.24]
            || m.style.layer_offsets_em != [0., 0.07, 0.14]
            || m.style.layer_shift_cells != [[0., 0.], [0.7, -3.6], [1.7, -9.2]]
            || m.style.radius_em != 0.00375
            || !m.style.opacity.is_finite()
            || !(0.0..=1.0).contains(&m.style.opacity)
            || m.style.layer_weights[0] != 1.
            || m.style
                .layer_weights
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !(0.0..=0.3).contains(&m.style.point_jitter_cells)
            || m.style.point_radius_range[0] <= 0.
            || m.style.point_radius_range[1] < m.style.point_radius_range[0]
            || m.style.point_radius_range[1] * m.style.radius_em >= m.pitch_em * 0.5
            || !(0.0..=8.0).contains(&m.style.scatter_gain)
            || m.style
                .base_color_linear_rgb
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("invalid Blender SSLE glyph package".into());
        }
        for (i, glyph) in m.glyphs.iter().enumerate() {
            if glyph.codepoint != 32 + i
                || !glyph.advance_em.is_finite()
                || !(0.01..2.).contains(&glyph.advance_em)
            {
                return Err("invalid SSLE font metrics".into());
            }
        }
        let mut atlas = vec![[0.; 4]; ATLAS_ROWS];
        for (row, bytes) in atlas.iter_mut().zip(data.chunks_exact(16)) {
            for (v, word) in row.iter_mut().zip(bytes.chunks_exact(4)) {
                *v = f32::from_le_bytes(word.try_into().unwrap());
                if !v.is_finite() || !(0.0..=65535.0).contains(v) || v.fract() != 0. {
                    return Err("invalid SSLE occupancy word".into());
                }
            }
        }
        let mut bounds = Vec::new();
        for glyph in 0..95 {
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for cell in 0..m.cell_width * m.cell_height {
                let word = cell / 16;
                if (atlas[glyph * GLYPH_ROWS + word / 4][word % 4] as u32 >> (cell % 16)) & 1 == 0 {
                    continue;
                }
                let x =
                    m.origin_em[0] + (cell % m.cell_width) as f64 * m.pitch_em + m.pitch_em * 0.5;
                let y =
                    m.origin_em[1] + (cell / m.cell_width) as f64 * m.pitch_em + m.pitch_em * 0.5;
                b[0] = b[0].min(x);
                b[1] = b[1].min(y);
                b[2] = b[2].max(x);
                b[3] = b[3].max(y);
            }
            bounds.push(if b[0].is_finite() { Some(b) } else { None });
        }
        if bounds[0].is_some() || bounds[(b'A' - 32) as usize].is_none() {
            return Err("invalid SSLE font silhouettes".into());
        }
        if m.demo_blocks.len() != 2 {
            return Err("SSLE demo requires both side-panel specimens".into());
        }
        let mut total = 0;
        let mut line_count = 0;
        for (block, panel) in m.demo_blocks.iter().zip([2, 3]) {
            if block.panel_index != panel
                || !block.margin_px.is_finite()
                || block.margin_px < 0.
                || block.bbox_px.iter().any(|v| !v.is_finite())
                || block.bbox_px[2] <= block.bbox_px[0]
                || block.bbox_px[3] <= block.bbox_px[1]
            {
                return Err("invalid SSLE specimen bounds".into());
            }
            for line in &block.lines {
                if !line.font_size_px.is_finite()
                    || line.font_size_px <= 0.
                    || line.baseline_px.iter().any(|v| !v.is_finite())
                    || !line.text.bytes().all(|c| (32..=126).contains(&c))
                    || line.text.len() > 48
                {
                    return Err("invalid SSLE specimen text".into());
                }
                total += line.text.len();
                line_count += 1;
            }
        }
        if total > MAX_CHARACTERS || line_count > MAX_LINES {
            return Err("SSLE specimen exceeds glyph capacity".into());
        }
        Ok(Self { manifest, bounds })
    }

    pub(crate) fn append(
        &self,
        rows: &mut Vec<[f32; 4]>,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<(), String> {
        self.append_for_page(rows,view,motion,options,2)
    }
    pub(crate) fn append_for_page(&self,rows:&mut Vec<[f32;4]>,view:Viewport,motion:&Snapshot,options:Options,right_page:usize)->Result<(),String>{
        self.append_with_browser(rows,view,motion,options,right_page,&Value::Null)
    }
    pub(crate) fn append_browser_labels(&self,rows:&mut Vec<[f32;4]>,view:Viewport,motion:&Snapshot,options:Options,right_page:usize,groups:&Value)->Result<(),String>{
        if !groups.is_array() { rows.resize(super::piano_scene::END+4096,[0.;4]); return Ok(()); }
        let mut packet=vec![[0.;4];BASE];
        self.append_with_browser(&mut packet,view,motion,options,right_page,groups)?;
        rows.resize(super::piano_scene::END+4096,[0.;4]);
        rows[super::piano_scene::END..].copy_from_slice(&packet[BASE+4096..]);
        rows[212..225].copy_from_slice(&packet[212..225]);
        let delta=(super::piano_scene::GPU_END) as f32-(BASE+4096) as f32;
        rows[212][1]+=delta;rows[222][0]+=delta;
        Ok(())
    }
    pub(crate) fn append_with_browser(&self,rows:&mut Vec<[f32;4]>,view:Viewport,motion:&Snapshot,options:Options,right_page:usize,browser_groups:&Value)->Result<(),String>{
        if rows.len() != BASE {
            return Err("invalid SSLE frame offset".into());
        }
        let rects = panel_layout::layout(view, motion, options)?;
        let selected_style = super::user_style::selected()?;
        let browser_layout = browser::layout(view, motion, options)?;
        let m = &self.manifest;
        let style = &m.style;
        let mpp = 1.3 / view.height;
        // The live catalog has its own bounded tail, outside monitor/transport poses.
        let dynamic = browser_groups.is_array();
        let max_characters = if dynamic { 1536 } else { MAX_CHARACTERS };
        let max_lines = if dynamic { 64 } else { MAX_LINES };
        let chars_base = if dynamic { BASE+4096 } else { BASE };
        let lines_base = chars_base + max_characters * 2;
        debug_assert!(lines_base+max_lines*3 <= BASE+ROWS+4096);
        rows.resize(BASE + ROWS + if dynamic {4096}else{0}, [0.; 4]);
        rows[212] = [DENSITY_BASE as f32, chars_base as f32, 0., 1.];
        rows[213] = [
            m.pitch_em as f32,
            m.origin_em[0] as f32,
            m.origin_em[1] as f32,
            style.radius_em as f32,
        ];
        rows[214] = [
            style.base_color_linear_rgb[0] as f32,
            style.base_color_linear_rgb[1] as f32,
            style.base_color_linear_rgb[2] as f32,
            style.opacity as f32,
        ];
        rows[215] = [
            style.layer_offsets_em[1] as f32,
            style.layer_offsets_em[2] as f32,
            style.layer_weights[1] as f32,
            style.layer_weights[2] as f32,
        ];
        rows[216] = [
            style.layer_shift_cells[1][0] as f32,
            style.layer_shift_cells[1][1] as f32,
            style.layer_shift_cells[2][0] as f32,
            style.layer_shift_cells[2][1] as f32,
        ];
        rows[223] = [
            style.point_jitter_cells as f32,
            style.point_radius_range[0] as f32,
            style.point_radius_range[1] as f32,
            style.scatter_gain as f32,
        ];
        rows[224] = [
            DENSITY_BASE as f32,
            DENSITY_EDGE as f32,
            1.2,
            DENSITY_TEXELS_PER_GLYPH as f32,
        ];
        let mut count = 0;
        let mut line_count = 0;
        let mut blocks = m.demo_blocks.clone();
        if let Some(groups) = browser_groups.as_array() {
            let mut lines = Vec::new();
            let reserved_chars: usize = blocks.iter().filter(|b| b.panel_index==3 && right_page==2)
                .flat_map(|b| &b.lines).map(|l| l.text.bytes().filter(|b| *b!=b' ').count()).sum();
            let reserved_lines: usize = blocks.iter().filter(|b| b.panel_index==3 && right_page==2).map(|b| b.lines.len()).sum();
            let mut remaining = max_characters.saturating_sub(reserved_chars);
            for label in groups.iter().flat_map(|g| g["labels"].as_array().into_iter().flatten()) {
                if lines.len() >= max_lines-reserved_lines { break; }
                let r = vector::<4>(&label["rect"])?;
                let mut font = number(&label["size"])?;
                if label["fit"]==true {
                    let advance: f64=label["text"].as_str().unwrap_or("").chars().take(48).map(|ch|{
                        let code=if ch.is_ascii_graphic()||ch==' ' {ch.to_ascii_uppercase() as u8}else{b'?'};
                        m.glyphs[(code-32) as usize].advance_em
                    }).sum();
                    if advance>0. {font=font.min(((r[2]-1.).max(0.)/advance).max(font*0.65));}
                }
                let mut text = String::new();
                let mut width = 0.;
                for ch in label["text"].as_str().unwrap_or("").chars().take(48) {
                    let code = if ch.is_ascii_graphic() || ch==' ' {ch.to_ascii_uppercase() as u8} else {b'?'};
                    let advance = m.glyphs[(code-32) as usize].advance_em*font;
                    if width+advance > r[2] || (code!=b' ' && remaining==0) {break;}
                    width+=advance;
                    if code!=b' ' {remaining-=1;}
                    text.push(code as char);
                }
                lines.push(Line {text,font_size_px:font,
                    baseline_px:[r[0],r[1]+r[3]*0.5+font*0.28]});
            }
            blocks.retain(|b| b.panel_index!=2);
            blocks.insert(0,Block {panel_index:2,margin_px:0.,bbox_px:[0.,0.,view.width,view.height],lines});
        }
        for block in &blocks {
            // Page zero is track I/O; legacy SETTINGS/LOAD/SAVE/ORIGAMI text
            // belongs exclusively to the settings page, never underneath I/O.
            if block.panel_index==3 && right_page!=2 {continue;}
            // The static desktop specimen is replaced by the live catalog.
            // Its labels and hit rectangles share PluginBrowser's layout.
            if cfg!(target_os="macos") && block.panel_index==2 && !browser_groups.is_array() { continue; }
            let rect = rects[block.panel_index];
            let custom_surface = selected_style.as_ref()
                .map(|kit| kit.surface(block.panel_index, view, motion, options))
                .transpose()?.flatten();
            let shape = if custom_surface.is_none() {
                Some(panel_layout::panel_shape(view, motion, options, block.panel_index)?)
            } else { None };
            let safe = custom_surface.as_ref().map(|surface| surface.size)
                .unwrap_or_else(|| shape.as_ref().unwrap().safe_rectangle_m);
            let b = block.bbox_px;
            let mut scale = (mpp * view.canvas_scale)
                .min(safe[0] / (b[2] - b[0] + 2. * block.margin_px))
                .min(safe[1] / (b[3] - b[1] + 2. * block.margin_px));
            let center = rect.center();
            let world = custom_surface.as_ref().map(|surface| [surface.origin[0], surface.origin[1]])
                .unwrap_or([
                (center[0] - view.width * 0.5) * mpp,
                (view.height * 0.5 - center[1]) * mpp,
            ]);
            let mut anchor = [
                world[0] - safe[0] * 0.5 + (block.margin_px - b[0]) * scale,
                world[1] + safe[1] * 0.5 - (block.margin_px - b[1]) * scale,
            ];
            if block.panel_index == 2 {
                scale = browser_layout.scale_m_per_px;
                anchor = browser_layout.anchor_m;
                if browser_groups.is_array() {
                    scale = mpp;
                    anchor = [-view.width*0.5*mpp,view.height*0.5*mpp];
                }
            }
            let depth = custom_surface.as_ref().map(|surface| surface.origin[2] + 0.00001)
                .unwrap_or_else(|| -0.08 * shape.as_ref().unwrap().local_unit_m);
            let start = count;
            let mut group = Bounds::new();
            for line in &block.lines {
                let line_start = count;
                let mut line_bounds = Bounds::new();
                let em = line.font_size_px * scale;
                let mut x = anchor[0] + line.baseline_px[0] * scale;
                let y = anchor[1] - line.baseline_px[1] * scale;
                for code in line.text.bytes().map(|c| if block.panel_index==2 {c.to_ascii_uppercase()} else {c}) {
                    let glyph = (code - 32) as usize;
                    if let Some(gb) = self.bounds[glyph] {
                        rows[chars_base + 2 * count] =
                            [x as f32, y as f32, depth as f32, em as f32];
                        rows[chars_base + 2 * count + 1] =
                            [glyph as f32, block.panel_index as f32, 0., 0.];
                        count += 1;
                        // Imported opaque panels carry one surface lettering layer.
                        // Gem lettering retains its original three interior layers.
                        for layer in 0..(if custom_surface.is_some() { 1 } else { 3 }) {
                            let shift = style.layer_shift_cells[layer];
                            let z = depth - style.layer_offsets_em[layer] * em;
                            let r = style.radius_em * style.point_radius_range[1] * em;
                            let spread = r + style.point_jitter_cells * m.pitch_em * em;
                            let lo = [
                                x + (gb[0] + shift[0] * m.pitch_em) * em - spread,
                                y + (gb[1] + shift[1] * m.pitch_em) * em - spread,
                                z - r * 0.45,
                            ];
                            let hi = [
                                x + (gb[2] + shift[0] * m.pitch_em) * em + spread,
                                y + (gb[3] + shift[1] * m.pitch_em) * em + spread,
                                z + r * 0.45,
                            ];
                            // Every gem microfracture fits strictly inside its cut.
                            // A user model may be a plane with no interior at all.
                            for plane in shape.iter().flat_map(|shape| &shape.planes) {
                                let support = (0..3)
                                    .map(|a| {
                                        plane[a]
                                            * (if plane[a] >= 0. { hi[a] } else { lo[a] }
                                                - if a < 2 { world[a] } else { 0. })
                                    })
                                    .sum::<f64>();
                                if support > plane[3] + 1e-8 {
                                    return Err("SSLE specimen left its panel interior".into());
                                }
                            }
                            line_bounds.include(lo, hi);
                            group.include(lo, hi);
                        }
                    }
                    x += m.glyphs[glyph].advance_em * em;
                }
                if count > line_start {
                    let at = lines_base + 3 * line_count;
                    line_bounds.write(rows, at);
                    rows[at][3] = line_start as f32;
                    rows[at + 1][3] = (count - line_start) as f32;
                    rows[at + 2] = [block.panel_index as f32, em as f32, 0., 0.];
                    line_count += 1;
                }
            }
            let slot = (block.panel_index - 2) * 2;
            rows[217][slot] = start as f32;
            rows[217][slot + 1] = (count - start) as f32;
            if count > start { group.write(rows, 218 + slot); }
        }
        rows[212][2] = count as f32;
        rows[222] = [lines_base as f32, line_count as f32, 0., 0.];
        Ok(())
    }
}
struct Bounds {
    lo: [f64; 3],
    hi: [f64; 3],
}
impl Bounds {
    fn new() -> Self {
        Self {
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
        }
    }
    fn include(&mut self, lo: [f64; 3], hi: [f64; 3]) {
        for a in 0..3 {
            self.lo[a] = self.lo[a].min(lo[a]);
            self.hi[a] = self.hi[a].max(hi[a]);
        }
    }
    fn write(&self, rows: &mut [[f32; 4]], at: usize) {
        for a in 0..3 {
            rows[at][a] = ((self.lo[a] + self.hi[a]) * 0.5) as f32;
            rows[at + 1][a] = ((self.hi[a] - self.lo[a]) * 0.5 + 1e-7) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_browser_uses_embedded_glyphs_and_preserves_other_frame_owners() {
        let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut motion=super::super::panel_motion::State::default();
        motion.set_all_open(true);motion.advance(1.);
        let view=Viewport {width:1920.,height:1200.,canvas_scale:1.};
        let options=Options::default();
        let catalog=vec![serde_json::json!({"name":"Example synth","format":"VST3"})];
        for category in 0..3 {
            let layout=super::super::plugin_browser::layout(view,&motion.snapshot(),options,&catalog,category,0.).unwrap();
            let labels=layout.presentation(category,None);
            let mut rows=vec![[0.;4];BASE];
            kit.append_with_browser(&mut rows,view,&motion.snapshot(),options,2,&labels).unwrap();
            assert!(rows.iter().flatten().all(|x|x.is_finite()));
            assert_eq!(rows[212][1],(BASE+4096) as f32);
            assert!(rows[217][1]>0.);
            for glyph in rows[BASE+4096..BASE+4096+rows[217][1] as usize*2].chunks_exact(2) {
                assert!(!(b'a'..=b'z').contains(&((glyph[1][0] as u8)+32)));
            }
            assert!(rows[217][3]>0.); // settings retains the same optical lettering
            assert!(rows[BASE+368..BASE+432].iter().all(|r|*r==[0.;4]));
            let chars=rows[217][1] as usize;
            assert!(rows[BASE+4096..BASE+4096+chars*2].chunks_exact(2).all(|c|c[1][1]==2.));
            let mut frame=vec![[7.;4];super::super::piano_scene::END];
            kit.append_browser_labels(&mut frame,view,&motion.snapshot(),options,2,&labels).unwrap();
            assert_eq!(frame.len(),super::super::piano_scene::END+4096);
            assert_eq!(frame[212][1],(super::super::piano_scene::GPU_END) as f32);
            assert!(frame[BASE..BASE+ROWS].iter().all(|r|*r==[7.;4]));
            assert_eq!(&frame[super::super::piano_scene::END..],&rows[BASE+4096..]);
        }
    }
    #[test]
    fn workstation_ssle_stays_inside_animated_panels() {
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        for (width, height) in [(1280., 720.), (640., 480.), (390., 844.), (844., 390.)] {
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
                    for extension in [0., 0.3, 1.] {
                        let mut snapshot = motion.snapshot();
                        snapshot.extensions[2] = extension;
                        snapshot.extensions[3] = extension;
                        let mut rows = vec![[0.; 4]; BASE];
                        kit.append(&mut rows, view, &snapshot, options).unwrap();
                        assert_eq!(rows.len(), BASE + ROWS);
                        assert!(rows.iter().flatten().all(|v| v.is_finite()));
                        assert!(rows[212][2] > 20.);
                        assert_eq!(rows[222][1], if cfg!(target_os="macos"){4.}else{8.}); // Desktop browser uses the UIOnly catalog layer.
                        // Each sample's front is behind the table; changes preserve upright text.
                        for i in 0..rows[212][2] as usize {
                            let p = rows[BASE + 2 * i];
                            assert!(p[2] < 0. && p[3] > 0.);
                        }
                    }
                }
            }
        }
    }

    // Desktop catalog lettering is now in the pass-through native surface.
    // Its component target coverage is tested through registered Step routes.
    #[cfg(not(target_os="macos"))]
    #[test]
    fn browser_targets_contain_the_rendered_action_lettering() {
        let kit = Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(
            kit.manifest.demo_blocks[0]
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            browser::LABELS
                .iter()
                .map(|label| label.text)
                .collect::<Vec<_>>()
        );
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        for (width, height, canvas_scale, swapped) in [
            (1280., 720., 1., false),
            (390., 844., 1., false),
            (844., 390., 0.5, true),
        ] {
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
            let placement = browser::layout(view, &motion.snapshot(), options).unwrap();
            let mut rows = vec![[0.; 4]; BASE];
            kit.append(&mut rows, view, &motion.snapshot(), options)
                .unwrap();
            let lines_base = rows[222][0] as usize;
            let mpp = 1.3 / height;
            for (i, target) in placement.targets.iter().enumerate() {
                let center = rows[lines_base + i * 3];
                let half = rows[lines_base + i * 3 + 1];
                let left = width * 0.5 + (center[0] - half[0]) as f64 / mpp;
                let right = width * 0.5 + (center[0] + half[0]) as f64 / mpp;
                let top = height * 0.5 - (center[1] + half[1]) as f64 / mpp;
                let bottom = height * 0.5 - (center[1] - half[1]) as f64 / mpp;
                assert!(left >= target.x && right <= target.x + target.width);
                assert!(top >= target.y && bottom <= target.y + target.height);
                assert_eq!(
                    browser::hit(view, &motion.snapshot(), options, target.center()),
                    browser::LABELS[i].action
                );
            }
        }
    }
}
