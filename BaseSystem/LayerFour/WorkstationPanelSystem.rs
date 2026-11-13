//! Origami's changing rectangles realized as closed, parametric Blender gems.
//! Layer five owns motion; this System prepares physical optics for the native
//! per-pixel dielectric pipelines. No lighting is baked into vertex colors.
use crate::{
    host::Host,
    workstation::{
        audio_spheres, bubble_notes, decorative_controls, deferred_floor, edison_demo, engraving, floor_light,
        gem_mesh::{self, PanelMesh},
        input_monitor, timeline_scene,
        panel_layout::{self, Options, Viewport},
        panel_motion::Snapshot,
    },
};
use serde_json::{Value, json};

const ASSET_DIRECTORY: &str = "Art/AudioWorkstation/GemPanels";
const NAMES: [&str; 4] = [
    "GemPanel_Strip",
    "GemPanel_Broad",
    "GemPanel_Tall",
    "GemPanel_Compact",
];
const FRAME_ROWS: usize = 256;

#[derive(Default)]
pub(crate) struct State {
    meshes: Vec<PanelMesh>,
    retained_camera:Value,
    style: Option<std::sync::Arc<crate::workstation::user_style::Kit>>,
    runtime: Value,
    controls: Option<decorative_controls::Kit>,
    instance_bounds: Option<decorative_controls::InstanceBounds>,
    input_monitors: Option<input_monitor::Kit>,
    track_io: Option<crate::workstation::track_io_scene::Kit>,
    edison_demo: Option<edison_demo::Kit>,
    engraving: Option<engraving::Kit>,
    bubbles: Option<bubble_notes::Kit>,
    timeline: Option<timeline_scene::Kit>,
    fader_components:Option<crate::workstation::fader_components::Kit>,
    floor_light: floor_light::Cache,
    view_timings: Option<ViewTimings>,
}

// Explicit diagnostic only. Keep frame records in bounded memory and write
// once at Release, outside frame preparation and every audio callback.
struct ViewTimings {
    frames: Vec<[f64; 8]>,
    omitted: usize,
}

struct ViewStageClock {
    previous: Option<std::time::Instant>,
    elapsed: [f64; 8],
    next: usize,
}

impl ViewStageClock {
    fn new(enabled: bool) -> Self {
        Self { previous: enabled.then(std::time::Instant::now), elapsed: [0.; 8], next: 0 }
    }
    fn mark(&mut self) {
        if let Some(previous) = self.previous {
            let now = std::time::Instant::now();
            self.elapsed[self.next] = now.duration_since(previous).as_secs_f64() * 1000.;
            self.previous = Some(now);
            self.next += 1;
        }
    }
}

fn number(value: &Value, label: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("missing/invalid Blender {label}"))
}
fn vector<const N: usize>(value: &Value, label: &str) -> Result<[f64; N], String> {
    let array = value
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("invalid Blender {label}"))?;
    let mut result = [0.; N];
    for (i, value) in array.iter().enumerate() {
        result[i] = number(value, label)?;
    }
    Ok(result)
}
fn row(v: [f64; 3], w: f64) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w as f32]
}
fn snapshot(input: &Value) -> Result<Snapshot, String> {
    let extensions: [f64; 4] = serde_json::from_value(input["extensions"].clone())
        .map_err(|_| "four panel extensions required")?;
    let mobile_page=if input["mobile_page"].is_null(){None}else{Some(input["mobile_page"].as_str()
        .and_then(crate::workstation::panel_motion::MobilePage::parse).ok_or("invalid mobile panel page")?)};
    let mobile_progress=input["mobile_progress"].as_f64().unwrap_or(1.);
    if !mobile_progress.is_finite() || !(0. ..=1.).contains(&mobile_progress){return Err("invalid mobile slide progress".into());}
    Ok(Snapshot {
        preamp_split:input["preamp_split"].as_f64().unwrap_or(0.),
        mobile_page,mobile_progress,
        fullscreen: if input["fullscreen"].is_null(){[0.;4]}else{serde_json::from_value(input["fullscreen"].clone()).map_err(|_|"four fullscreen weights required")?},
        extensions,
        open: serde_json::from_value(input["open"].clone())
            .map_err(|_| "four panel open targets required")?,
        opening_order: serde_json::from_value(input["opening_order"].clone())
            .map_err(|_| "four Origami opening ranks required")?,
        left_width_scale: number(&input["left_width_scale"], "left width scale")?,
        bottom_height_scale: number(&input["bottom_height_scale"], "bottom height scale")?,
        overview_hide: serde_json::from_value(input["overview_hide"].clone())
            .map_err(|_| "four overview states required")?,
    })
}

// The primitive covers the view; its fragments intersect the actual 26-plane
// solids. This is a 3D ray tracer, not a flat image or a substitute panel mesh.
pub(crate) fn fullscreen() -> Vec<[f32; 24]> {
    [
        [[-1., -1.], [1., -1.], [1., 1.]],
        [[-1., -1.], [1., 1.], [-1., 1.]],
    ]
    .map(|corners| {
        let mut triangle = [0.; 24];
        for (i, p) in corners.iter().enumerate() {
            triangle[i * 4..i * 4 + 4].copy_from_slice(&[p[0], p[1], 0.5, 1.]);
        }
        triangle
    })
    .to_vec()
}

fn load(state: &mut State, root: &std::path::Path) -> Result<(), String> {
    if !state.meshes.is_empty() {
        return Ok(());
    }
    let runtime: Value = serde_json::from_slice(
        &std::fs::read(root.join(ASSET_DIRECTORY).join("GemPanels_runtime.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if runtime["schema_version"] != 2
        || runtime["cut"]["halfspaces"] != 26
        || (number(&runtime["cut"]["depth"], "cut depth")? - 0.85).abs() > 1e-6
        || (number(&runtime["cut"]["crown_inset"], "crown inset")? - 0.31).abs() > 1e-6
        || runtime["view"]["transform"] != "AgX"
        || runtime["view"]["display"] != "sRGB"
        || runtime["view"]["lut"]["edge"] != 57
    {
        return Err("unsupported Blender panel cut or display transform; re-export and update its optical contract".into());
    }
    let meshes = gem_mesh::load(root)?;
    for name in NAMES {
        if !meshes.iter().any(|mesh| mesh.name == name) {
            return Err(format!("missing authored panel {name}"));
        }
    }
    state.controls = Some(decorative_controls::Kit::load(root)?);
    state.instance_bounds = Some(decorative_controls::InstanceBounds::load(root)?);
    state.input_monitors = Some(input_monitor::Kit::load(root)?);
    state.track_io = Some(crate::workstation::track_io_scene::Kit::load(root)?);
    state.edison_demo = Some(edison_demo::Kit::load(root)?);
    state.engraving = Some(engraving::Kit::load(root)?);
    state.bubbles = Some(bubble_notes::Kit::load(root)?);
    state.timeline = Some(timeline_scene::Kit::load(root)?);
    state.fader_components=Some(crate::workstation::fader_components::Kit::load(root)?);
    state.style = crate::workstation::user_style::selected()?;
    state.meshes = meshes;
    state.runtime = runtime;
    if std::env::var("WORMWOOD_WORKSTATION_CPU_STAGES").as_deref() == Ok("1") {
        state.view_timings = Some(ViewTimings { frames: Vec::with_capacity(8192), omitted: 0 });
    }
    Ok(())
}

/// Final mobile admission: inactive full-size slots are layout-only. Clear
/// their actual solid/control admission before building the instance hierarchy.
fn apply_mobile_visibility(rows:&mut [[f32;4]],motion:&Snapshot,viewport:Viewport) {
    use crate::workstation::{panel_motion::Edge,track_io_scene,keyboard_toggle,
        fader_components,bottom_panel_scene,timeline_bar_scene};
    // UserStyle owns row246.xy. Its unused zw carry only the mobile cover's
    // half-plane: tagged edge and world-space boundary; zero keeps old packets
    // and all settled/desktop rendering unchanged.
    rows[246][2]=0.;rows[246][3]=0.;
    if motion.mobile_page.is_none() && motion.fullscreen.iter().all(|p|*p==0.){return;}
    let sliding=motion.mobile_progress<1.;
    if let Some(crate::workstation::panel_motion::MobilePage::Panel(edge))=motion.mobile_page {
        if sliding {
            let p=motion.mobile_progress.clamp(0.,1.);let mpp=1.3/viewport.height;
            let boundary=match edge {
                Edge::Top=>(viewport.height*0.5-viewport.height*p)*mpp,
                Edge::Bottom=>(viewport.height*p-viewport.height*0.5)*mpp,
                Edge::Left=>(viewport.width*p-viewport.width*0.5)*mpp,
                Edge::Right=>(viewport.width*0.5-viewport.width*p)*mpp,
            };
            rows[246][2]=12001.+edge as usize as f32;rows[246][3]=boundary as f32;
        }
    }
    for edge in Edge::ALL {if !motion.panel_visible(edge){rows[32+40*edge as usize][0]=0.;}}
    if !motion.panel_visible(Edge::Top){for i in 0..4 {rows[192+2*i][3]=0.;}}
    if !motion.panel_visible(Edge::Bottom){
        for i in 4..10 {rows[192+2*i][3]=0.;}
        rows[232][1]=0.;rows[232][2]=0.;
        let base=rows[232][0] as usize;
        if base>0&&base+64<=rows.len(){for i in 0..16 {rows[base+2*i][3]=0.;}for i in 0..4 {rows[base+32+2*i][3]=0.;}}
        rows[255][0]=0.;rows[255][1]=0.;rows[255][2]=0.;
        rows[fader_components::BASE][0]=0.;rows[bottom_panel_scene::BASE+5]=[0.;4];
    }
    if !motion.panel_visible(Edge::Right){rows[track_io_scene::HEADER][1]=0.;rows[keyboard_toggle::HEADER][1]=0.;}
    if !motion.timeline_visible()&&!sliding{
        for i in 0..3 {rows[226+2*i][3]=0.;}rows[247][1]=0.;
        rows[234..238].fill([0.;4]);rows[240..244].fill([0.;4]);rows[248..255].fill([0.;4]);
        for i in 0..2{rows[timeline_bar_scene::BASE+i]=[0.;4];rows[timeline_bar_scene::BASE+3+i]=[0.;4];}
        rows[timeline_bar_scene::BASE+8][1]=0.;rows[timeline_bar_scene::BASE+9][3]=0.;
    }
    if !motion.panel_visible(Edge::Top){rows[timeline_bar_scene::BASE+2]=[0.;4];rows[timeline_bar_scene::BASE+5]=[0.;4];}
    rows[timeline_bar_scene::BASE+8][3]=(0..3).filter(|i|rows[timeline_bar_scene::BASE+3+*i][3]>0.).count() as f32;
}

fn optical_frame(
    state: &State,
    viewport: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<Vec<[f32; 4]>, String> {
    let rects = panel_layout::layout(viewport, motion, options)?;
    let runtime = &state.runtime;
    // A head-on, orthographic workstation preserves Cardinal's exact screen
    // rectangles. The physical studio remains in Blender metres as panels move.
    let metres_per_pixel = 1.3 / viewport.height;
    let half_height = viewport.height * metres_per_pixel * 0.5;
    let half_width = viewport.width * metres_per_pixel * 0.5;
    let mut rows = vec![[0.; 4]; FRAME_ROWS];
    rows[0] = [
        1.,
        4.,
        2.,
        number(&runtime["view"]["exposure_stops"], "exposure")? as f32,
    ];
    rows[1] = [0., 0., 3., 0.];
    rows[2] = [1., 0., 0., half_width as f32];
    rows[3] = [0., 1., 0., half_height as f32];
    rows[4] = [0., 0., -1., 1.];
    let world = vector::<3>(&runtime["world"]["color_linear_rgb"], "world color")?;
    let strength = number(&runtime["world"]["strength"], "world strength")?;
    rows[5] = row(world.map(|v| v * strength), -1.);
    rows[6] = row(
        vector(&runtime["ground"]["base_color_linear_rgb"], "ground color")?,
        number(&runtime["ground"]["roughness"], "ground roughness")?,
    );
    let domain = vector::<2>(&runtime["view"]["lut"]["domain_log2"], "AgX domain")?;
    rows[7] = [
        (FRAME_ROWS + floor_light::ROWS + decorative_controls::ROWS + engraving::ROWS) as f32,
        57.,
        domain[0] as f32,
        domain[1] as f32,
    ];
    rows[8] = [viewport.width as f32, viewport.height as f32, 16., 64.];
    let lights = runtime["lights"]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or("Blender panel studio requires three area lights")?;
    rows[9] = [lights.len() as f32, 1., 0., if crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PLAIN_BACKDROP") {motion.fullscreen.iter().sum::<f64>() as f32}else{0.}];
    for (i, light) in lights.iter().enumerate() {
        let at = 16 + 5 * i;
        let size = vector::<2>(&light["size_m"], "light size")?;
        rows[at] = row(vector(&light["position_m"], "light position")?, 0.);
        rows[at + 1] = row(vector(&light["axis_u"], "light axis U")?, size[0] * 0.5);
        rows[at + 2] = row(vector(&light["axis_v"], "light axis V")?, size[1] * 0.5);
        rows[at + 3] = row(vector(&light["emission_normal"], "emitter normal")?, 0.);
        rows[at + 4] = row(
            vector(&light["radiance_watts_per_sr_m2_rgb"], "emitter radiance")?,
            0.,
        );
    }
    let materials = runtime["materials"]
        .as_array()
        .ok_or("missing Blender materials")?;
    let mut floor: f64 = 0.;
    for (i, rect) in rects.iter().enumerate() {
        let shape = panel_layout::panel_shape(viewport, motion, options, i)?;
        floor = floor.min(-shape.dimensions_m[2] - 0.02);
        let center = rect.center();
        let origin = [
            (center[0] - viewport.width * 0.5) * metres_per_pixel,
            (viewport.height * 0.5 - center[1]) * metres_per_pixel,
            0.,
        ];
        let material = materials
            .iter()
            .find(|m| m["asset"] == NAMES[i])
            .ok_or_else(|| format!("missing Blender material {}", NAMES[i]))?;
        let at = 32 + 40 * i;
        rows[at] = [
            if motion.panel_visible(crate::workstation::panel_motion::Edge::ALL[i]) {1.} else {0.},
            number(&material["ior"], "IOR")? as f32,
            number(&material["roughness"], "roughness")? as f32,
            number(&material["transmission"], "transmission")? as f32,
        ];
        rows[at + 1] = row(
            vector::<3>(&material["sigma_per_source_unit"], "volume absorption")?
                .map(|v| v / shape.local_unit_m),
            0.,
        );
        rows[at + 2] = row([origin[0], origin[1], -shape.dimensions_m[2] * 0.5], 0.);
        rows[at + 3] = row(shape.dimensions_m.map(|v| v * 0.5 + 1e-6), 0.);
        let base_color = vector::<4>(&material["base_color_linear_rgba"], "dielectric base color")?;
        rows[at + 4] = row([base_color[0], base_color[1], base_color[2]], 0.);
        for (offset, key) in [(32, "ggx_glass_E"), (36, "ggx_glass_inv_E")] {
            let energy = vector::<16>(&material["ggx_multiscatter"][key], "GGX energy table")?;
            if energy.iter().any(|v| *v <= 0. || *v > 1.01) {
                return Err("invalid Blender GGX directional albedo".into());
            }
            for j in 0..4 {
                rows[at + offset + j] = std::array::from_fn(|k| energy[4 * j + k] as f32);
            }
        }
        for (j, plane) in shape.planes.iter().enumerate() {
            let distance = plane[3] + plane[0] * origin[0] + plane[1] * origin[1];
            rows[at + 6 + j] = row([plane[0], plane[1], plane[2]], distance);
        }
    }
    rows[5][3] = floor as f32;
    if !rows.iter().flatten().all(|v| v.is_finite()) {
        return Err("nonfinite workstation optical data".into());
    }
    Ok(rows)
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.workstation_panels);
    let result = (|| match hook {
        "Load" => {
            load(&mut state, host.root())?;
            Ok(
                json!({"kind":"visual","shader":"workstation_panel","vertices":[[-1.,-1.,0.5,0.,0.,0.],[1.,-1.,0.5,0.,0.,0.],[1.,1.,0.5,0.,0.,0.]]}),
            )
        }
        "View" => {
            let mut stages = ViewStageClock::new(state.view_timings.is_some());
            if state.meshes.is_empty() {
                return Err("load the workstation before presenting it".into());
            }
            let entity = input["entity"]
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or("workstation view requires an Entity")?;
            let viewport = Viewport {
                width: number(&input["width"], "viewport width")?,
                height: number(&input["height"], "viewport height")?,
                canvas_scale: number(&input["canvas_scale"], "canvas scale")?,
            };
            let options = Options {
                origami: input["origami"].as_bool().unwrap_or(true),
                fixed_top: input["fixed_top"].as_bool().unwrap_or(true),
                swap_sides: input["swap_sides"] == true,
                swap_horizontal: input["swap_horizontal"] == true,
                ..Options::default()
            };
            let mut rows = optical_frame(&state, viewport, &snapshot(input)?, options)?;
            // Panel base-color W is display metadata; optical color remains XYZ.
            // Grey the paused browser by tinting its immutable baked image only.
            rows[116][3]=if input["browser_disabled"]==true {1.}else{0.};
            if let Some(style) = &state.style {
                let time = input["style_time"].as_f64().unwrap_or(0.);
                style.apply(&mut rows, viewport, &snapshot(input)?, options, time)?;
            }
            crate::workstation::lightbox::apply(&mut rows,&input["lightbox"])?;
            let framebuffer = vector::<2>(&input["framebuffer"], "framebuffer")?;
            rows[8][0] = framebuffer[0] as f32;
            rows[8][1] = framebuffer[1] as f32;
            let transport_pressure = if input["transport_pressure"].is_null() { [0.; 4] }
                else { vector::<4>(&input["transport_pressure"], "transport pressure")? };
            // Share the slowly varying floor-light visibility across all glass
            // paths. Gem transport and the floor's outgoing GGX stay per pixel.
            let domain = floor_light::domain(&rows);
            rows[10] = [
                FRAME_ROWS as f32,
                floor_light::EDGE as f32,
                domain[0],
                domain[1],
            ];
            rows[11] = [domain[2], domain[3], 0., 0.];
            stages.mark();
            let defer_floor = state.style.is_none() && crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_DEFER_FLOOR");
            if defer_floor {
                // A host-only request, consumed by every native backend. The
                // retained WebGPU path resolves only before optical work;
                // original/fallback paths always resolve before upload.
                deferred_floor::append_placeholder(&mut rows);
            } else {
                let visibility = state.floor_light.field(&rows);
                if state.style.is_some() { rows[10][1] = 0.; }
                rows.extend_from_slice(visibility);
            }
            stages.mark();
            state
                .controls
                .as_ref()
                .ok_or("missing authored controls")?
                .append(&mut rows, viewport, &snapshot(input)?, options)?;
            state
                .input_monitors
                .as_ref()
                .ok_or("missing authored input monitors")?
                .append(&mut rows, viewport, &snapshot(input)?, options)?;
            stages.mark();
            state
                .engraving
                .as_ref()
                .ok_or("missing Blender SSLE specimen")?
                .append_for_page(&mut rows, viewport, &snapshot(input)?, options,input["track_io"]["page"].as_u64().unwrap_or(2) as usize)?;
            stages.mark();
            state.bubbles.as_ref().ok_or("missing authored bubble notes")?
                .append(&mut rows,&input["bubbles"])?;
            audio_spheres::append(&mut rows, &input["spheres"])?;
            rows.resize(timeline_scene::END, [0.; 4]);
            state.timeline.as_ref().ok_or("missing authored timeline kit")?
                .append(&mut rows,viewport,&input["timeline"])?;
            state.edison_demo.as_ref().ok_or("missing Edison hardware")?
                .append(&mut rows,viewport,&snapshot(input)?,options,&input["plugins"])?;
            crate::workstation::timeline_bar_scene::append(&mut rows,viewport,&snapshot(input)?,options,
                &input["timeline"],&input["loop_control"])?;
            let bottom_depth=crate::workstation::plugin_inserts::layout(viewport,&snapshot(input)?,options)?.depth;
            crate::workstation::bottom_panel_scene::append(&mut rows,viewport,&input["bottom_panel"],
                state.timeline.as_ref().ok_or("missing authored timeline kit")?.bottom_controls(),bottom_depth)?;
            crate::workstation::bottom_panel_scene::append_full_preview(&mut rows,&input["bottom_panel"],
                crate::workstation::bottom_panel_scene::FULL_PREVIEW_BASE)?;
            if input["bottom_panel"].is_object(){for instance in 4..10{rows[192+2*instance][3]=0.;}}
            state.fader_components.as_ref().ok_or("missing authored fader components")?
                .append(&mut rows,[viewport.width,viewport.height],bottom_depth,&input["bottom_panel"])?;
            // Actual audio/MIDI lane furniture is visual only; the explicit
            // palette preview remains available for authored-material checks.
            if input["monitor_preview"] != true {
                state.input_monitors.as_ref().ok_or("missing authored input monitors")?
                    .append_lanes(&mut rows,viewport,&input["timeline"])?;
            }
            stages.mark();
            state.track_io.as_ref().ok_or("missing authored track I/O hardware")?
                .append(&mut rows,viewport,&input["track_io"])?;
            // Final furniture writers have finished. Move the actual authored
            // buttons before deriving the live instance hierarchy.
            crate::workstation::transport_pose::apply(&mut rows, transport_pressure, 1.3 / viewport.height)?;
            crate::workstation::keyboard_toggle::append(&mut rows,viewport,&input["computer_keyboard_controls"],
                state.timeline.as_ref().ok_or("missing mini gem kit")?.mini_control())?;
            apply_mobile_visibility(&mut rows,&snapshot(input)?,viewport);
            // Bounds follow the final poses, including Edison controls and
            // live lane monitors, before capture or native submission.
            state.instance_bounds.as_ref().ok_or("missing control instance bounds")?
                .append(&mut rows)?;
            crate::workstation::piano_scene::append(&mut rows,host.root(),&input["piano_scene"])?;
            // The piano room owns the entire view; timeline browser lettering
            // must not be appended over it, even when its panel remains open.
            if input["piano_scene"]["active"] != true && input["piano_scene"]["drum_workspace"] != true {
                state.engraving.as_ref().ok_or("missing engraving kit")?.append_browser_labels(
                    &mut rows,viewport,&snapshot(input)?,options,input["track_io"]["page"].as_u64().unwrap_or(2) as usize,&input["browser_labels"])?;
            }
            state.bubbles.as_ref().ok_or("missing authored bubble notes")?
                .append_animation_ids(&mut rows,&input["bubbles"])?;
            crate::workstation::timeline_controls::append(&mut rows,viewport,&input["timeline"],&input["bottom_panel"],
                state.fader_components.as_ref().ok_or("missing fader hardware")?,state.track_io.as_ref().ok_or("missing crosswalk hardware")?)?;
            // Timeline reconstructs the reserved header. Seal host metadata
            // only after all packet writers, before capture or native upload.
            if defer_floor { deferred_floor::mark_request(&mut rows)?; }
            stages.mark();
            let camera=&input["camera_view"];
            // Use the exact player scene builder and native scene passes. The
            // fullscreen optical surface is submitted only after the world.
            let has_camera=camera.is_object()&&camera["entity"].is_u64();
            let reuse_world=has_camera&&crate::workstation::paused_world::enabled(host.backend())&&state.retained_camera==*camera;
            if has_camera{rows[9][2]=if input["computer_screen"]==true && input["passthrough"]!=true{0.}else{1.};}
            let live_scene=if has_camera&&!reuse_world {
                let mut view=camera.clone();view["security_camera"]=json!(camera["id"]!="player");view["audio_rays_visible"]=json!(false);
                Some(host.camera_scene(&view)?)
            }else{None};
            let mut ray_view=camera.clone();
            if ray_view.is_object(){ray_view["audio_rays_visible"]=input["audio_rays"].clone();ray_view["time"]=input["style_time"].clone();}
            let audio_overlay=if has_camera && input["audio_rays"]==true && host.system_enabled("AudioRayVisualizerSystem"){host.audio_ray_geometry(&ray_view)?}else{Vec::new()};
            // In physical-screen mode the baked compositor uses the otherwise unused
            // studio area-light rows as the perspective map (never optical work).
            if input["computer_screen"]==true {
                let m=crate::workstation::computer_screen::map(camera,host.theatre.computer_position().ok_or("missing theatre computer")?,viewport.width/viewport.height);
                rows[9][0]=-1.;rows[16..19].copy_from_slice(&m);
            }
            let triangles=fullscreen();
            if input["capture"] == true {
                let capture_rows=deferred_floor::complete(&rows)?;
                let mut capture_batch=live_scene.as_ref().map_or_else(Vec::new,|scene|scene.triangles.clone());
                capture_batch.extend_from_slice(&triangles);
                let mut camera_capture=crate::workstation::benchmark::camera::evidence(camera,&capture_batch);
                if let Some(scene)=&live_scene {
                    camera_capture["scene"]=json!({"path":"shared player scene; native world passes then optical overlay","water_packets":scene.water.len(),"vegetation":scene.vegetation.is_some(),"view":scene.view});
                }
                let evidence = json!({"camera_capture":camera_capture,"rows":capture_rows.as_ref(),"logical_viewport":[viewport.width,viewport.height],"canvas_scale":viewport.canvas_scale,"materials":state.runtime["materials"],"user_style":state.style.as_ref().map(|s|json!({"id":s.id,"name":s.name,"animated":s.animated,"background":s.background}))});
                std::fs::write(
                    host.output_root().join("workstation-optical-frame.json"),
                    serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
            stages.mark();
            if reuse_world{
                host.reuse_workstation_scene(entity,&rows,input["capture"]==true,&audio_overlay)?;
            }else if let Some(scene)=live_scene {
                host.set_workstation_scene(entity,&rows,input["capture"]==true,scene,&audio_overlay)?;
            } else {
                host.set_workstation_view(entity,&triangles,&rows,input["capture"]==true)?;
            }
            state.retained_camera=if has_camera{camera.clone()}else{Value::Null};
            stages.mark();
            if let Some(timings) = &mut state.view_timings {
                if timings.frames.len() < 8192 { timings.frames.push(stages.elapsed); }
                else { timings.omitted += 1; }
            }
            Ok(Value::Null)
        }
        "Suspend" => {
            // Rebuild the observer-dependent packet on focus, retain all asset kits.
            state.retained_camera=Value::Null;
            Ok(Value::Null)
        }
        "Release" => {
            state.retained_camera=Value::Null;
            if let Some(timings) = &state.view_timings {
                let evidence = json!({
                    "schema_version": 1,
                    "scope": "CPU wall time inside WorkstationPanelSystem.View; upload includes synchronous SetView only, not GPU execution or presentation. Records are written after the frame loop at Release.",
                    "stage_names": ["header", "floor_visibility", "controls", "engraving", "dynamic_furniture", "instance_bounds", "capture", "upload"],
                    "frames_ms": timings.frames,
                    "omitted_frames": timings.omitted,
                    "capacity_frames": 8192,
                });
                std::fs::write(host.output_root().join("workstation-cpu-stages.json"),
                    serde_json::to_vec(&evidence).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            }
            state = State::default();
            Ok(Value::Null)
        }
        _ => Err("unknown workstation panel hook".into()),
    })();
    host.workstation_panels = state;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires WORMWOOD_TEST_ASSET_ROOT pointing to a built app's EngineAssets"]
    fn workstation_packaged_assets_load_panels() {
        let root = std::env::var_os("WORMWOOD_TEST_ASSET_ROOT")
            .expect("packaged EngineAssets path is required");
        let root = std::path::Path::new(&root).canonicalize().unwrap();
        assert_ne!(root, std::path::Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap());
        let mut state = State::default();
        load(&mut state, &root).unwrap();
        assert_eq!(state.meshes.len(), 4);
    }
    #[test]
    fn workstation_actual_panels_make_physical_frames_in_portrait_and_landscape() {
        let mut state = State::default();
        load(&mut state, std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(state.meshes.len(), 4);
        for mesh in &state.meshes {
            let cut =
                panel_layout::optical_shape(mesh.dimensions_m[0], mesh.dimensions_m[1]).unwrap();
            let tolerance = mesh.dimensions_m.iter().copied().fold(0.0, f64::max) * 5e-5;
            for vertex in &mesh.vertices {
                assert!(
                    cut.planes.iter().all(|p| p[0] * vertex.position[0]
                        + p[1] * vertex.position[1]
                        + p[2] * vertex.position[2]
                        - p[3]
                        <= tolerance),
                    "authored {} vertex outside reconstructed cut",
                    mesh.name
                );
            }
        }
        let mut motion = crate::workstation::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        for (width, height) in [(1280., 720.), (390., 844.)] {
            let rows = optical_frame(
                &state,
                Viewport {
                    width,
                    height,
                    canvas_scale: 1.,
                },
                &motion.snapshot(),
                Options::default(),
            )
            .unwrap();
            assert_eq!(rows.len(), 256);
            for i in 0..4 {
                let at = 32 + i * 40;
                assert!((rows[at][1] - 1.766).abs() < 1e-5);
                assert!((rows[at][2] - 0.018).abs() < 1e-5);
                assert_eq!(rows[at][3], 1.);
                assert!(rows[at + 1][1] > rows[at + 1][0] * 100.);
                for p in &rows[at + 6..at + 32] {
                    assert!((p[..3].iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-4);
                }
            }
        }
    }
}
