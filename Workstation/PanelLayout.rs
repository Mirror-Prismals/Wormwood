//! Cardinal Origami geometry and the matching, resizable Blender gem cut.
//!
//! Layout source: Salamander137/BaseSystem/PanelSystem.cpp, computePanelSpan,
//! horizontalOwnsCorner, fitSideToHorizontalPanels and the four compute*Rect
//! functions (lines 67–307). Default policy: Salamander137/Host.h, PanelContext.
//! Gem source: Salamander137/Procedures/Shaders/DawGem.frag.wgsl, plane/support;
//! Art/AudioWorkstation/GemPanels/make_gem_panels.py, planes/cut_polyhedron.
//!
//! Rectangles use logical viewport pixels, origin at upper left. Mesh geometry
//! uses metres, +X right, +Y up, +Z out of the table at Z=0. Resizing evaluates
//! the original cut's halfspaces again, including its crown, clipped corners
//! and split pavilion. It never stretches a previously cut mesh's vertices.
use super::{
    gem_mesh::{PanelMesh, Vertex},
    panel_motion::{Edge, Snapshot},
};

const SIDE_FRACTION: f64 = 0.2;
const HORIZONTAL_FRACTION: f64 = 0.3;
const FACE_INSET: f64 = 18.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Viewport {
    pub width: f64,
    pub height: f64,
    /// Cardinal's native view is 1; overview reduces the centered design canvas.
    pub canvas_scale: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AdaptiveSpan {
    #[default]
    Off,
    Both,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Options {
    pub origami: bool,
    pub fixed_top: bool,
    pub adaptive_span: AdaptiveSpan,
    pub swap_sides: bool,
    pub swap_horizontal: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            origami: true,
            fixed_top: true,
            adaptive_span: AdaptiveSpan::Off,
            swap_sides: false,
            swap_horizontal: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub(crate) fn center(self) -> [f64; 2] {
        [self.x + self.width * 0.5, self.y + self.height * 0.5]
    }
}

fn horizontal_owns(snapshot: &Snapshot, options: Options, horizontal: Edge, side: Edge) -> bool {
    if horizontal == Edge::Top && options.fixed_top {
        return false;
    }
    let horizontal_order = snapshot.opening_order[horizontal as usize];
    let side_order = snapshot.opening_order[side as usize];
    side_order == 0 || (horizontal_order != 0 && horizontal_order < side_order)
}

fn adapts(options: Options, horizontal: Edge) -> bool {
    options.adaptive_span == AdaptiveSpan::Both
        || matches!(
            (horizontal, options.adaptive_span),
            (Edge::Top, AdaptiveSpan::Top) | (Edge::Bottom, AdaptiveSpan::Bottom)
        )
}

/// Same edge rectangles used by Cardinal's material renderer and pointer
/// bounds. Order is top, bottom, left, right, even when physical edges swap.
pub(crate) fn layout(
    viewport: Viewport,
    snapshot: &Snapshot,
    options: Options,
) -> Result<[Rect; 4], String> {
    if !snapshot.preamp_split.is_finite() || !(0. ..=1.).contains(&snapshot.preamp_split) || !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || !(1.0..=1.0e6).contains(&viewport.width)
        || !(1.0..=1.0e6).contains(&viewport.height)
        || !viewport.canvas_scale.is_finite()
        || !(0.01..=1.0).contains(&viewport.canvas_scale)
        || snapshot
            .extensions
            .iter()
            .chain(&snapshot.overview_hide)
            .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
        || snapshot.fullscreen.iter().any(|p|!p.is_finite() || !(0. ..=1.).contains(p))
        || snapshot.fullscreen.iter().sum::<f64>()>1.+1.0e-6
        || !snapshot.mobile_progress.is_finite()
        || !(0.0..=1.0).contains(&snapshot.mobile_progress)
        || !snapshot.left_width_scale.is_finite()
        || !(1.0..=1.5).contains(&snapshot.left_width_scale)
        || !snapshot.bottom_height_scale.is_finite()
        || !(1.0..=1.5).contains(&snapshot.bottom_height_scale)
    {
        return Err("invalid workstation Origami viewport or motion snapshot".into());
    }
    if let Some(page) = snapshot.mobile_page { return Ok(mobile_rects(viewport,page,snapshot.mobile_progress)); }
    let width = viewport.width;
    let height = viewport.height;
    let design_width = width * viewport.canvas_scale;
    let design_height = height * viewport.canvas_scale;
    let origin_x = (width - design_width) * 0.5;
    let origin_y = (height - design_height) * 0.5;
    let [top, bottom, left, right] = snapshot.extensions;
    let left_width = design_width * SIDE_FRACTION * snapshot.left_width_scale;
    let right_width = design_width * SIDE_FRACTION;
    let top_height = design_height * HORIZONTAL_FRACTION;
    let bottom_height = design_height * HORIZONTAL_FRACTION * snapshot.bottom_height_scale;
    let top_y = origin_y - top_height + (FACE_INSET + top_height) * top;
    let bottom_y = origin_y + design_height - bottom_height * bottom;

    let span = |edge: Edge| {
        let fixed = || {
            let start = origin_x + FACE_INSET + left_width;
            let end = (origin_x + design_width - right_width).max(start + 1.0);
            (start, end)
        };
        if options.origami && options.fixed_top && edge == Edge::Top {
            return fixed();
        }
        let adaptive = options.origami || adapts(options, edge);
        if !adaptive {
            return fixed();
        }
        let left_owns = options.origami && horizontal_owns(snapshot, options, edge, Edge::Left);
        let right_owns = options.origami && horizontal_owns(snapshot, options, edge, Edge::Right);
        let left_state = if left_owns { 0.0 } else { left };
        let right_state = if right_owns { 0.0 } else { right };
        let mut start = origin_x + FACE_INSET + left_width * left_state;
        let mut end = (origin_x + design_width - right_width * right_state).max(start + 1.0);
        if viewport.canvas_scale < 0.999 {
            // Shift overview reclaims visible space outside the design canvas
            // only when that side gives up its corner (source computePanelSpan).
            let left_yields = if options.origami {
                left_owns
            } else {
                !snapshot.open[2]
            };
            let right_yields = if options.origami {
                right_owns
            } else {
                !snapshot.open[3]
            };
            if left_yields {
                start -= origin_x * (1.0 - left_state);
            }
            if right_yields {
                end += origin_x * (1.0 - right_state);
            }
        }
        (start, end)
    };
    let (top_left, top_right) = span(Edge::Top);
    let (bottom_left, bottom_right) = span(Edge::Bottom);
    // Overview exposes the whole narrow side panel even when its ordinary
    // slide is retracted. Only the separate overview-hide gesture parks it
    // offscreen. Blend placement, never width, from the normal slide path.
    let overview_alignment = ((1.0 - viewport.canvas_scale) / (1.0 - 0.61)).clamp(0.0, 1.0);
    let normal_left_x = -left_width + (FACE_INSET + left_width) * left;
    let normal_right_x = width - right_width * right;
    let mut result = [
        Rect {
            x: top_left,
            y: top_y,
            width: top_right - top_left,
            height: top_height,
        },
        Rect {
            x: bottom_left,
            y: bottom_y,
            width: bottom_right - bottom_left,
            height: bottom_height,
        },
        Rect {
            x: normal_left_x * (1.0 - overview_alignment),
            y: origin_y + FACE_INSET,
            width: left_width,
            height: (design_height - FACE_INSET).max(1.0),
        },
        Rect {
            x: normal_right_x + (width - right_width - normal_right_x) * overview_alignment,
            y: origin_y + FACE_INSET,
            width: right_width,
            height: (design_height - FACE_INSET).max(1.0),
        },
    ];
    for edge in [Edge::Left, Edge::Right] {
        let index = edge as usize;
        let adaptive_overview = viewport.canvas_scale < 0.999 && !snapshot.open[index];
        if !options.origami && !adaptive_overview {
            continue;
        }
        let top_owns = if options.origami {
            horizontal_owns(snapshot, options, Edge::Top, edge)
        } else {
            adapts(options, Edge::Top)
        };
        let bottom_owns = if options.origami {
            horizontal_owns(snapshot, options, Edge::Bottom, edge)
        } else {
            adapts(options, Edge::Bottom)
        };
        let rect = &mut result[index];
        let fit_top = if top_owns {
            rect.y.max(top_y + top_height)
        } else {
            rect.y
        };
        let fit_bottom = if bottom_owns {
            (rect.y + rect.height).min(bottom_y)
        } else {
            rect.y + rect.height
        };
        rect.y = fit_top;
        rect.height = (fit_bottom - fit_top).max(1.0);
    }
    for edge in Edge::ALL {
        let index = edge as usize;
        let rect = &mut result[index];
        match edge {
            Edge::Top => rect.y -= origin_y * snapshot.overview_hide[index],
            Edge::Bottom => rect.y += origin_y * snapshot.overview_hide[index],
            Edge::Left => rect.x -= (rect.x + rect.width).max(0.0) * snapshot.overview_hide[index],
            Edge::Right => rect.x += (width - rect.x).max(0.0) * snapshot.overview_hide[index],
        }
        // Reflect bounds, never the geometry or upright panel contents.
        if options.swap_sides {
            rect.x = width - rect.x - rect.width;
        }
        if options.swap_horizontal {
            rect.y = height - rect.y - rect.height;
        }
    }
    // Blend from the ordinary Origami layout. The selected panel grows into
    // the viewport while its neighbors park outside it; open/size preferences
    // survive the return. Do this after swaps so the full view stays upright.
    let total=snapshot.fullscreen.iter().sum::<f64>();
    if total>0. {
        let full=Rect{x:0.,y:0.,width,height};
        for i in 0..4 {
            let base=result[i];
            let physical=match Edge::ALL[i] {
                Edge::Left if options.swap_sides=>Edge::Right,
                Edge::Right if options.swap_sides=>Edge::Left,
                Edge::Top if options.swap_horizontal=>Edge::Bottom,
                Edge::Bottom if options.swap_horizontal=>Edge::Top,
                e=>e,
            };
            let mut parked=base;
            match physical {
                Edge::Top=>parked.y=-base.height-32.,
                Edge::Bottom=>parked.y=height+32.,
                Edge::Left=>parked.x=-base.width-32.,
                Edge::Right=>parked.x=width+32.,
            }
            let own=snapshot.fullscreen[i];
            let other=total-own;
            // The intermediate stop fills three quarters of the viewport on
            // the panel's growth axis. Swaps determine its anchored edge.
            let extended=match physical {
                Edge::Top=>Rect{x:0.,y:0.,width,height:height*0.75},
                Edge::Bottom=>Rect{x:0.,y:height*0.25,width,height:height*0.75},
                Edge::Left=>Rect{x:0.,y:0.,width:width*0.75,height},
                Edge::Right=>Rect{x:width*0.25,y:0.,width:width*0.75,height},
            };
            let (from,to,t)=if own<=0.75 {(base,extended,own/0.75)}
                else {(extended,full,(own-0.75)/0.25)};
            let park=(other/0.75).min(1.);
            result[i]=Rect {
                x:from.x+(to.x-from.x)*t+(parked.x-base.x)*park,
                y:from.y+(to.y-from.y)*t+(parked.y-base.y)*park,
                width:from.width+(to.width-from.width)*t,
                height:from.height+(to.height-from.height)*t,
            };
        }
    }
    if snapshot.mobile_page.is_none() && snapshot.fullscreen.iter().all(|p|*p==0.) {
        let progress=snapshot.preamp_split;
        let left=(viewport.width-viewport.width*viewport.canvas_scale)*0.5+FACE_INSET;
        let shift=(result[1].x-left)*progress;
        result[1].x-=shift;result[1].width+=shift;
        // The browser gives its lower corner to the new preamp as it opens.
        let browser=&mut result[if options.swap_sides {3}else{2}];
        if !options.swap_horizontal {let end=(browser.y+browser.height).min(bottom_y);browser.height+=(end-browser.y-browser.height)*progress;}
    }
    let amount=split_width(viewport,snapshot,result[1].width);
    result[1].x+=amount;
    result[1].width-=amount;
    Ok(result)
}

fn split_width(view:Viewport,motion:&Snapshot,width:f64)->f64 {
    if motion.mobile_page.is_some() || motion.fullscreen.iter().any(|p|*p>0.) {return 0.;}
    ((view.width*view.canvas_scale*SIDE_FRACTION*motion.left_width_scale+8.*view.canvas_scale)*motion.preamp_split.clamp(0.,1.)).min((width-160.*view.canvas_scale).max(0.))
}
pub(crate) fn preamp_rect(view:Viewport,motion:&Snapshot,options:Options)->Result<Rect,String>{
    let mut unsplit=*motion;unsplit.preamp_split=0.;
    let mut bottom=layout(view,&unsplit,options)?[1];
    if motion.mobile_page.is_none() && motion.fullscreen.iter().all(|p|*p==0.) {let left=(view.width-view.width*view.canvas_scale)*0.5+FACE_INSET;let shift=(bottom.x-left)*motion.preamp_split;bottom.x-=shift;bottom.width+=shift;}
    let width=split_width(view,motion,bottom.width);
    Ok(Rect{x:bottom.x,y:bottom.y,width:(width-8.*view.canvas_scale*motion.preamp_split).max(0.),height:bottom.height})
}

/// Positive-sized offscreen slots keep every existing authored control/layout
/// transform finite. Render admission separately disables hidden solids.
fn mobile_rects(view: Viewport, page: super::panel_motion::MobilePage, progress:f64) -> [Rect;4] {
    std::array::from_fn(|i| {
        if page==super::panel_motion::MobilePage::Panel(Edge::ALL[i]) {
            let travel=1.-progress.clamp(0.,1.);
            let (x,y)=match Edge::ALL[i] {Edge::Top=>(0.,-view.height*travel),Edge::Bottom=>(0.,view.height*travel),Edge::Left=>(-view.width*travel,0.),Edge::Right=>(view.width*travel,0.)};
            Rect{x,y,width:view.width,height:view.height}
        } else {
            Rect{x:-(i as f64+2.)*view.width,y:-2.*view.height,width:view.width,height:view.height}
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GestureAction {
    Open,
    Close,
    ToggleExpanded,
}

/// Cardinal's iOS panel gesture selection (Platform/iOS/TouchControls.h):
/// begin within 20 logical points of a screen edge or visible inner rim;
/// the blank 3D showcase also accepts the whole visible panel face.
/// finish with at least 36 points of travel and a 1.5:1 dominant panel axis.
/// The selected logical panel follows swaps; its real rendered bounds supply
/// the rim. Two-finger swipes use the same region/travel test and additionally
/// toggle left/bottom expansion, the mobile equivalent of Alt+edge arrow.
/// That two-finger expansion is an adaptation; Cardinal used one-finger
/// open/close and reserved size changes for its desktop modifier shortcut.
pub(crate) fn gesture(
    viewport: Viewport,
    snapshot: &Snapshot,
    options: Options,
    start: [f64; 2],
    delta: [f64; 2],
    touches: u32,
) -> Option<(Edge, GestureAction)> {
    let style = super::user_style::selected().ok()?;
    gesture_with_style(viewport, snapshot, options, start, delta, touches, style.as_deref())
}

// Explicit style injection keeps package-preview checks independent of global
// selection, including tests that run concurrently with ordinary gem tests.
pub(super) fn gesture_with_style(
    viewport: Viewport,
    snapshot: &Snapshot,
    options: Options,
    start: [f64; 2],
    delta: [f64; 2],
    touches: u32,
    style: Option<&super::user_style::Kit>,
) -> Option<(Edge, GestureAction)> {
    if !(1..=2).contains(&touches)
        || start.iter().chain(&delta).any(|value| !value.is_finite())
        || !(0.0..=viewport.width).contains(&start[0])
        || !(0.0..=viewport.height).contains(&start[1])
    {
        return None;
    }
    let rects = layout(viewport, snapshot, options).ok()?;
    let mut best_distance = 20.0;
    let mut selected = None;
    // Match the source's left/right/top/bottom ordering, including corner ties.
    for physical in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let logical = match physical {
            Edge::Left if options.swap_sides => Edge::Right,
            Edge::Right if options.swap_sides => Edge::Left,
            Edge::Top if options.swap_horizontal => Edge::Bottom,
            Edge::Bottom if options.swap_horizontal => Edge::Top,
            _ => physical,
        };
        let index = logical as usize;
        let custom = style.and_then(|kit|kit.surface(index,viewport,snapshot,options).ok().flatten());
        let rect = custom.map_or(rects[index],|surface|surface.visual_rect);
        let horizontal = matches!(physical, Edge::Left | Edge::Right);
        let coordinate = if horizontal { start[0] } else { start[1] };
        let edge_coordinate = match physical {
            Edge::Left | Edge::Top => 0.0,
            Edge::Right => viewport.width,
            Edge::Bottom => viewport.height,
        };
        let within = if horizontal {
            (rect.y..=rect.y + rect.height).contains(&start[1])
        } else {
            (rect.x..=rect.x + rect.width).contains(&start[0])
        };
        let rim = match physical {
            Edge::Left => rect.x + rect.width,
            Edge::Right => rect.x,
            Edge::Top => rect.y + rect.height,
            Edge::Bottom => rect.y,
        };
        let mut distance = (coordinate - edge_coordinate).abs();
        // Cardinal publishes regions while their animated extension exceeds .1,
        // even after the open target has changed during a closing animation.
        // A snap model can already be fully visible while the raw Origami
        // extension is still near zero. Its actual mesh determines coverage.
        if (custom.is_some() || snapshot.extensions[index] > 0.1) && within {
            // A concave custom mesh has no face in the empty parts of its AABB.
            if custom.is_none() { distance = distance.min((coordinate - rim).abs()); }
            // Blank 3D panels have no embedded controls competing for swipes.
            // Prefer the visible face over a nearby screen-edge activation zone.
            if (rect.x..=rect.x + rect.width).contains(&start[0])
                && (rect.y..=rect.y + rect.height).contains(&start[1])
                && custom.is_none_or(|_| style.is_some_and(|kit|
                    kit.hit_panel_point(index,viewport,snapshot,options,start).unwrap_or(false)))
            {
                distance = -1.0;
            }
        }
        if distance < best_distance {
            best_distance = distance;
            selected = Some((physical, logical));
        }
    }
    let (physical, logical) = selected?;
    let (along, across) = if matches!(physical, Edge::Left | Edge::Right) {
        (delta[0], delta[1])
    } else {
        (delta[1], delta[0])
    };
    if along.abs() < 36.0 || along.abs() <= across.abs() * 1.5 {
        return None;
    }
    if touches == 2 {
        return matches!(logical, Edge::Left | Edge::Bottom)
            .then_some((logical, GestureAction::ToggleExpanded));
    }
    let inward = match physical {
        Edge::Top | Edge::Left => along > 0.0,
        Edge::Bottom | Edge::Right => along < 0.0,
    };
    Some((
        logical,
        if inward {
            GestureAction::Open
        } else {
            GestureAction::Close
        },
    ))
}

type V = [f64; 3];
fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: V) -> V {
    let length = dot(v, v).sqrt();
    v.map(|component| component / length)
}
fn normal(plane: [f64; 4]) -> V {
    [plane[0], plane[1], plane[2]]
}

/// Complete panel outlines for the central canvas and surrounding appearances.
/// Raw Origami slots remain the input to model fitting, never this resolved view.
pub(crate) fn visual_bounds(view: Viewport, motion: &Snapshot, options: Options)
    -> Result<[Rect; 4], String>
{
    let style = super::user_style::selected()?;
    visual_bounds_with_style(view, motion, options, style.as_deref())
}

pub(super) fn visual_bounds_with_style(view: Viewport, motion: &Snapshot, options: Options,
    style: Option<&super::user_style::Kit>) -> Result<[Rect; 4], String>
{
    let mut result = layout(view, motion, options)?;
    if let Some(style) = style {
        for (index, rect) in result.iter_mut().enumerate() {
            if let Some(surface) = style.surface(index, view, motion, options)? {
                *rect = surface.visual_rect;
            }
        }
    }
    Ok(result)
}

/// One mounted content surface for rendering and interaction, independent of style.
pub(crate) fn resolved_surface(view: Viewport, motion: &Snapshot, options: Options, index: usize)
    -> Result<super::user_style::Surface, String>
{
    if let Some(kit) = super::user_style::selected()? {
        if let Some(surface) = kit.surface(index, view, motion, options)? { return Ok(surface); }
    }
    let rect = *layout(view, motion, options)?.get(index).ok_or("invalid panel index")?;
    let mpp = 1.3 / view.height;
    let safe = panel_shape(view, motion, options, index)?.safe_rectangle_m;
    let center = rect.center();
    Ok(super::user_style::Surface {
        origin: [(center[0]-view.width*0.5)*mpp, (view.height*0.5-center[1])*mpp, 0.],
        size: safe, scale: [1.;3], visual_rect: rect,
    })
}

/// Actual convex gemstone in table-origin metres. Unit outward plane normals
/// make optical intersections, flat shading and deformed mesh geometry agree.
pub(crate) struct GemShape {
    pub planes: [[f64; 4]; 26],
    pub vertices: Vec<Vertex>,
    pub dimensions_m: [f64; 3],
    pub safe_rectangle_m: [f64; 2],
    /// Blender's absorption density is expressed per source local unit.
    pub local_unit_m: f64,
}

/// Plane-only representation used by the per-pixel optical integrator. It is
/// the same solid as GemShape, without constructing vertices/triangles that
/// the ray tracer never consumes. Evaluation is bounded to 26 planes.
#[derive(Clone, Copy)]
pub(crate) struct OpticalShape {
    pub planes: [[f64; 4]; 26],
    pub dimensions_m: [f64; 3],
    pub safe_rectangle_m: [f64; 2],
    pub local_unit_m: f64,
}

/// Evaluate the shared Blender/Cardinal cut at its requested physical size.
/// The pavilion normals depend on the new aspect; simply stretching the
/// exported normals or existing vertex positions does not reproduce this cut.
/// Full-view expansion extends the face while retaining the normal panel cut
/// thickness. Contents and optical intersections consume this same shape.
pub(crate) fn panel_shape(view:Viewport, motion:&Snapshot, options:Options, index:usize) -> Result<OpticalShape,String> {
    let rect=*layout(view,motion,options)?.get(index).ok_or("invalid panel index")?;
    let mpp=1.3/view.height;
    let mut normal=*motion;normal.fullscreen=[0.;4];
    let baseline=layout(view,&normal,options)?[index];
    let unit=rect.width.min(rect.height).min(baseline.width.min(baseline.height))*mpp*0.5;
    optical_shape_with_unit(rect.width*mpp,rect.height*mpp,unit)
}

pub(crate) fn optical_shape(width_m: f64, height_m: f64) -> Result<OpticalShape, String> {
    optical_shape_with_unit(width_m,height_m,width_m.min(height_m)*0.5)
}

fn optical_shape_with_unit(width_m:f64,height_m:f64,local_unit_m:f64)->Result<OpticalShape,String> {
    if !width_m.is_finite() || !height_m.is_finite() || width_m<=0. || height_m<=0.
        || !local_unit_m.is_finite() || local_unit_m<=0. || local_unit_m>width_m.min(height_m)*0.5+1e-12 {
        return Err("gem dimensions and cut thickness must be positive and finite".into());
    }
    let minor=local_unit_m*2.;
    let half = [width_m / minor, height_m / minor];
    let mut planes = [[0.0; 4]; 26];
    planes[0] = [0.0, 0.0, 1.0, 0.30];
    planes[1] = [0.0, 0.0, -1.0, 0.85];
    for ring in 0..3 {
        for side in 0..8 {
            let angle = side as f64 * std::f64::consts::FRAC_PI_4
                + if ring == 2 {
                    std::f64::consts::FRAC_PI_8
                } else {
                    0.0
                };
            let (y, x) = angle.sin_cos();
            let extent = x.abs() * half[0] + y.abs() * half[1] - 0.18 * x.abs().min(y.abs());
            let slope = extent * 0.86 / (0.85 - 0.07);
            planes[2 + ring * 8 + side] = [
                x,
                y,
                match ring {
                    0 => 0.0,
                    1 => 0.31 / 0.30,
                    _ => -slope,
                },
                if ring == 2 {
                    extent + slope * 0.07
                } else {
                    extent
                },
            ];
        }
    }
    for plane in &mut planes {
        let length = dot(normal(*plane), normal(*plane)).sqrt();
        plane[3] = (plane[3] - 0.30 * plane[2]) * local_unit_m / length;
        for component in &mut plane[..3] {
            *component /= length;
        }
    }
    if !planes
        .iter()
        .flatten()
        .all(|component| component.is_finite())
    {
        return Err("gem dimensions exceed finite cut precision".into());
    }
    Ok(OpticalShape {
        planes,
        dimensions_m: [width_m, height_m, 1.15 * local_unit_m],
        safe_rectangle_m: [width_m - 0.31 * minor, height_m - 0.31 * minor],
        local_unit_m,
    })
}

pub(crate) fn shape(width_m: f64, height_m: f64) -> Result<GemShape, String> {
    let optical = optical_shape(width_m, height_m)?;
    let planes = optical.planes;
    let local_unit_m = optical.local_unit_m;
    let epsilon = local_unit_m * 2.0e-6;
    let mut points = Vec::<V>::new();
    for a in 0..24 {
        for b in a + 1..25 {
            for c in b + 1..26 {
                let [na, nb, nc] = [normal(planes[a]), normal(planes[b]), normal(planes[c])];
                let bc = cross(nb, nc);
                let determinant = dot(na, bc);
                if determinant.abs() < 1.0e-10 {
                    continue;
                }
                let ca = cross(nc, na);
                let ab = cross(na, nb);
                let p = std::array::from_fn(|i| {
                    (planes[a][3] * bc[i] + planes[b][3] * ca[i] + planes[c][3] * ab[i])
                        / determinant
                });
                if planes
                    .iter()
                    .any(|plane| dot(normal(*plane), p) > plane[3] + epsilon)
                {
                    continue;
                }
                if !points.iter().any(|q| {
                    let d = std::array::from_fn(|i| p[i] - q[i]);
                    dot(d, d) < (local_unit_m * 2.0e-5).powi(2)
                }) {
                    points.push(p);
                }
            }
        }
    }
    let mut vertices = Vec::with_capacity(76 * 3);
    for plane in &planes {
        let n = normal(*plane);
        let mut face: Vec<V> = points
            .iter()
            .copied()
            .filter(|point| (dot(n, *point) - plane[3]).abs() < local_unit_m * 3.0e-5)
            .collect();
        if face.len() < 3 {
            continue;
        }
        let center: V =
            std::array::from_fn(|i| face.iter().map(|p| p[i]).sum::<f64>() / face.len() as f64);
        let tangent = unit(cross(
            n,
            if n[2].abs() < 0.9 {
                [0., 0., 1.]
            } else {
                [0., 1., 0.]
            },
        ));
        let bitangent = cross(n, tangent);
        let angle = |p: V| {
            let d = std::array::from_fn(|i| p[i] - center[i]);
            dot(d, bitangent).atan2(dot(d, tangent))
        };
        face.sort_by(|a, b| angle(*a).total_cmp(&angle(*b)));
        for index in 1..face.len() - 1 {
            for position in [face[0], face[index], face[index + 1]] {
                vertices.push(Vertex {
                    position,
                    normal: n,
                });
            }
        }
    }
    if vertices.is_empty()
        || vertices
            .iter()
            .any(|v| v.position.iter().any(|n| !n.is_finite()))
    {
        return Err("requested gem dimensions did not produce a finite closed cut".into());
    }
    Ok(GemShape {
        planes,
        vertices,
        dimensions_m: optical.dimensions_m,
        safe_rectangle_m: optical.safe_rectangle_m,
        local_unit_m,
    })
}

/// Retain the authored material identity while resizing the actual solid.
/// The Blender material expresses density relative to the cut's local unit;
/// update attenuation distance alongside that unit, rather than changing ruby
/// concentration accidentally as an Origami panel changes its short side.
pub(crate) fn reshape(mesh: &PanelMesh, width_m: f64, height_m: f64) -> Result<PanelMesh, String> {
    let shape = shape(width_m, height_m)?;
    let old_local_unit = mesh.dimensions_m[0].min(mesh.dimensions_m[1]) * 0.5;
    let mut material = mesh.material.clone();
    material.thickness_m = shape.dimensions_m[2];
    material.attenuation_distance_m *= shape.local_unit_m / old_local_unit;
    Ok(PanelMesh {
        name: mesh.name.clone(),
        vertices: shape.vertices,
        dimensions_m: shape.dimensions_m,
        safe_rectangle_m: shape.safe_rectangle_m,
        material,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Viewport {
        Viewport {
            width: 1000.0,
            height: 600.0,
            canvas_scale: 1.0,
        }
    }

    #[test]
    fn workstation_origami_matches_cardinal_default_open_rectangles() {
        let state = Snapshot {
            extensions: [1.0; 4],
            open: [true; 4],
            opening_order: [3, 4, 1, 2],
            ..Snapshot::default()
        };
        let r = layout(viewport(), &state, Options::default()).unwrap();
        assert_eq!(
            r,
            [
                Rect {
                    x: 218.0,
                    y: 18.0,
                    width: 582.0,
                    height: 180.0
                },
                Rect {
                    x: 218.0,
                    y: 420.0,
                    width: 582.0,
                    height: 180.0
                },
                Rect {
                    x: 18.0,
                    y: 18.0,
                    width: 200.0,
                    height: 582.0
                },
                Rect {
                    x: 800.0,
                    y: 18.0,
                    width: 200.0,
                    height: 582.0
                },
            ]
        );
    }

    #[test]
    fn workstation_origami_older_bottom_keeps_corners_when_sides_open_later() {
        let state = Snapshot {
            extensions: [1.0; 4],
            open: [true; 4],
            opening_order: [4, 1, 2, 3],
            ..Snapshot::default()
        };
        let r = layout(viewport(), &state, Options::default()).unwrap();
        assert_eq!(r[0].width, 582.0); // fixed-top variant does not claim corners
        assert_eq!(
            r[1],
            Rect {
                x: 18.0,
                y: 420.0,
                width: 982.0,
                height: 180.0
            }
        );
        assert_eq!(r[2].height, 402.0);
        assert_eq!(r[3].height, 402.0);
    }

    #[test]
    fn workstation_origami_closing_side_continuously_resizes_bottom() {
        let state = Snapshot {
            extensions: [1.0, 1.0, 0.5, 1.0],
            open: [true, true, false, true],
            opening_order: [3, 4, 1, 2],
            ..Snapshot::default()
        };
        let r = layout(viewport(), &state, Options::default()).unwrap();
        assert_eq!(r[0].width, 582.0);
        assert_eq!(r[1].x, 118.0);
        assert_eq!(r[1].width, 682.0);
    }

    #[test]
    fn workstation_origami_overview_reclaims_outer_width_and_swaps_upright_bounds() {
        let state = Snapshot {
            extensions: [1.0; 4],
            open: [true; 4],
            opening_order: [4, 1, 2, 3],
            ..Snapshot::default()
        };
        let v = Viewport {
            canvas_scale: 0.5,
            ..viewport()
        };
        let r = layout(v, &state, Options::default()).unwrap();
        assert_eq!(r[0].x, 368.0); // top remains inside the design canvas
        assert_eq!(r[1].x, 18.0);
        assert_eq!(r[1].width, 982.0); // bottom reclaims the entire visible width
        assert_eq!(r[2].x, 0.0); // overview reaches the physical screen edge
        assert_eq!(r[2].width, 100.0);
        assert_eq!(r[3].x, 900.0);
        assert_eq!(r[3].x + r[3].width, r[1].x + r[1].width);
        let swapped = layout(
            v,
            &state,
            Options {
                swap_sides: true,
                swap_horizontal: true,
                ..Options::default()
            },
        )
        .unwrap();
        for i in 0..4 {
            assert_eq!(swapped[i].x, 1000.0 - r[i].x - r[i].width);
            assert_eq!(swapped[i].y, 600.0 - r[i].y - r[i].height);
            assert_eq!(swapped[i].width, r[i].width);
            assert_eq!(swapped[i].height, r[i].height);
        }
    }

    #[test]
    fn overview_side_alignment_preserves_normal_widths() {
        for scale in [1.0, 0.61, 0.5] {
            let view = Viewport { canvas_scale: scale, ..viewport() };
            let alignment = ((1.0 - scale) / (1.0 - 0.61)).clamp(0.0, 1.0);
            for extension in [0.0, 0.25, 0.5, 1.0] {
                let state = Snapshot {
                    extensions: [extension; 4],
                    open: [extension > 0.0; 4],
                    ..Snapshot::default()
                };
                for swapped in [false, true] {
                    let rects = layout(view, &state, Options {
                        swap_sides: swapped, ..Options::default()
                    }).unwrap();
                    let side_width = view.width * scale * SIDE_FRACTION;
                    assert_eq!(rects[2].width, side_width);
                    assert_eq!(rects[3].width, side_width);
                    let left_x = (-side_width + (side_width + FACE_INSET) * extension)
                        * (1.0-alignment);
                    let normal_right = view.width - side_width * extension;
                    let right_x = normal_right + (view.width-side_width-normal_right)*alignment;
                    let expected = if swapped {
                        [view.width-left_x-side_width, view.width-right_x-side_width]
                    } else { [left_x, right_x] };
                    assert!((rects[2].x-expected[0]).abs() < 1e-9);
                    assert!((rects[3].x-expected[1]).abs() < 1e-9);
                    if scale <= 0.61 {
                        let mut hidden = state;
                        hidden.overview_hide = [1.0;4];
                        let parked=layout(view,&hidden,Options {swap_sides:swapped,..Options::default()}).unwrap();
                        for r in [parked[2],parked[3]] {
                            assert!(r.x+r.width <= 1e-9 || r.x >= view.width-1e-9);
                        }
                    }
                    if scale <= 0.61 {
                        for rect in [rects[2], rects[3]] {
                            assert!(rect.x >= -1e-9);
                            assert!(rect.x + rect.width <= view.width + 1e-9);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn workstation_gesture_accepts_edges_rims_and_blank_faces() {
        let closed = Snapshot::default();
        assert_eq!(
            gesture(
                viewport(),
                &closed,
                Options::default(),
                [10., 300.],
                [36., 0.],
                1
            ),
            Some((Edge::Left, GestureAction::Open))
        );
        assert_eq!(
            gesture(
                viewport(),
                &closed,
                Options::default(),
                [10., 300.],
                [35., 0.],
                1
            ),
            None
        );
        assert_eq!(
            gesture(
                viewport(),
                &closed,
                Options::default(),
                [10., 300.],
                [36., 24.],
                1
            ),
            None
        );
        assert_eq!(
            gesture(
                viewport(),
                &closed,
                Options::default(),
                [500., 300.],
                [80., 0.],
                1
            ),
            None
        );
        let opened = Snapshot {
            extensions: [1.; 4],
            open: [true; 4],
            opening_order: [3, 4, 1, 2],
            ..closed
        };
        assert_eq!(
            gesture(
                viewport(),
                &opened,
                Options::default(),
                [500., 198.],
                [0., -36.],
                1
            ),
            Some((Edge::Top, GestureAction::Close))
        );
        assert_eq!(
            gesture(
                viewport(),
                &opened,
                Options::default(),
                [500., 100.],
                [0., -80.],
                1
            ),
            Some((Edge::Top, GestureAction::Close))
        );
        assert_eq!(
            gesture(
                viewport(),
                &opened,
                Options::default(),
                [f64::NAN, 0.],
                [0., 80.],
                1
            ),
            None
        );
    }

    #[test]
    fn workstation_gesture_maps_physical_edges_after_swaps_and_two_finger_expansion() {
        let options = Options {
            swap_sides: true,
            swap_horizontal: true,
            ..Options::default()
        };
        let closed = Snapshot::default();
        assert_eq!(
            gesture(viewport(), &closed, options, [995., 300.], [-40., 0.], 1),
            Some((Edge::Left, GestureAction::Open))
        );
        assert_eq!(
            gesture(viewport(), &closed, options, [995., 300.], [-40., 0.], 2),
            Some((Edge::Left, GestureAction::ToggleExpanded))
        );
        assert_eq!(
            gesture(viewport(), &closed, options, [500., 5.], [0., 40.], 2),
            Some((Edge::Bottom, GestureAction::ToggleExpanded))
        );
        assert_eq!(
            gesture(viewport(), &closed, options, [500., 595.], [0., -40.], 2),
            None
        );
    }

    #[test]
    fn face_swipes_close_and_edge_swipes_reopen_all_origami_panels() {
        use super::super::panel_motion::State;
        for scale in [1.0, 0.5] {
            for swapped in [false, true] {
                let view = Viewport {
                    canvas_scale: scale,
                    ..viewport()
                };
                let options = Options {
                    swap_sides: swapped,
                    swap_horizontal: swapped,
                    ..Options::default()
                };
                for edge in Edge::ALL {
                    let mut motion = State::default();
                    motion.set_all_open(true);
                    motion.advance(1.0);
                    let rect = layout(view, &motion.snapshot(), options).unwrap()[edge as usize];
                    let mut outward = match edge {
                        Edge::Left => [-60., 0.],
                        Edge::Right => [60., 0.],
                        Edge::Top => [0., -60.],
                        Edge::Bottom => [0., 60.],
                    };
                    if swapped {
                        outward = outward.map(|n| -n);
                    }
                    assert_eq!(
                        gesture(view, &motion.snapshot(), options, rect.center(), outward, 1),
                        Some((edge, GestureAction::Close))
                    );
                    motion.set_open(edge, false);
                    motion.advance(0.1);
                    assert!(motion.extension(edge) > 0. && motion.extension(edge) < 1.);
                    motion.advance(1.0);
                    assert_eq!(motion.extension(edge), 0.);
                    let start = if outward[0] < 0. {
                        [5., 300.]
                    } else if outward[0] > 0. {
                        [995., 300.]
                    } else if outward[1] < 0. {
                        [500., 5.]
                    } else {
                        [500., 595.]
                    };
                    assert_eq!(
                        gesture(
                            view,
                            &motion.snapshot(),
                            options,
                            start,
                            outward.map(|n| -n),
                            1
                        ),
                        Some((edge, GestureAction::Open))
                    );
                    motion.set_open(edge, true);
                    motion.advance(1.0);
                    assert_eq!(motion.extension(edge), 1.);
                }
            }
        }
    }

    #[test]
    fn workstation_optical_shape_matches_closed_mesh_halfspaces() {
        for [w, h] in [[0.96, 0.12], [0.3, 0.66], [0.96, 0.45]] {
            let optical = optical_shape(w, h).unwrap();
            let mesh = shape(w, h).unwrap();
            assert_eq!(optical.planes, mesh.planes);
            assert_eq!(optical.dimensions_m, mesh.dimensions_m);
            assert_eq!(optical.safe_rectangle_m, mesh.safe_rectangle_m);
            assert_eq!(optical.local_unit_m, mesh.local_unit_m);
        }
    }

    #[test]
    fn workstation_resized_gems_are_closed_with_outward_facets_at_phone_aspects() {
        use std::collections::BTreeMap;
        for [width, height] in [
            [0.96, 0.45],
            [0.3, 0.66],
            [0.4, 0.3],
            [0.96, 0.12],
            [0.13, 1.2],
            [2.8, 0.2],
            [0.3, 0.3],
            [0.001, 0.01],
        ] {
            let gem = shape(width, height).unwrap();
            let mut edges = BTreeMap::<([u64; 3], [u64; 3]), usize>::new();
            let mut volume = 0.0;
            for triangle in gem.vertices.chunks_exact(3) {
                let [a, b, c] = [
                    triangle[0].position,
                    triangle[1].position,
                    triangle[2].position,
                ];
                let ab = std::array::from_fn(|i| b[i] - a[i]);
                let ac = std::array::from_fn(|i| c[i] - a[i]);
                assert!(dot(cross(ab, ac), triangle[0].normal) > 0.0);
                volume += dot(a, cross(b, c)) / 6.0;
                for (a, b) in [(a, b), (b, c), (c, a)] {
                    let (a, b) = (a.map(f64::to_bits), b.map(f64::to_bits));
                    let edge = if a < b { (a, b) } else { (b, a) };
                    *edges.entry(edge).or_default() += 1;
                }
                for vertex in triangle {
                    assert!(
                        gem.planes
                            .iter()
                            .all(|p| dot(normal(*p), vertex.position) <= p[3] + 1.0e-9)
                    );
                }
            }
            assert!(volume > 0.0);
            assert!(
                edges.values().all(|n| *n == 2),
                "open or nonmanifold resized gem {width}x{height}"
            );
            assert!((gem.dimensions_m[2] - width.min(height) * 0.575).abs() < 1.0e-12);
        }
    }
}

#[cfg(test)] mod mobile_layout_tests {
    use super::*;use super::super::panel_motion::MobilePage;
    #[test] fn one_fullscreen_panel_in_portrait_and_landscape_without_hidden_rims() {
        for [width,height] in [[390.,844.],[844.,390.],[1024.,1366.]] {
            let view=Viewport{width,height,canvas_scale:0.5};
            for page in [MobilePage::Timeline,MobilePage::Panel(Edge::Top),MobilePage::Panel(Edge::Bottom),MobilePage::Panel(Edge::Left),MobilePage::Panel(Edge::Right)] {
                let s=Snapshot{mobile_page:Some(page),extensions:[1.;4],open:[true;4],..Snapshot::default()};
                let r=layout(view,&s,Options{swap_sides:true,swap_horizontal:true,..Options::default()}).unwrap();
                for (i,p) in r.iter().enumerate() {assert!(p.width>0.&&p.height>0.);if page==MobilePage::Panel(Edge::ALL[i]) {
                    assert_eq!(*p,Rect{x:0.,y:0.,width,height});
                }else{assert!(p.x+p.width<0.&&p.y+p.height<0.);}}
            }
        }
    }
    #[test] fn desktop_layout_is_unchanged_when_mobile_selection_is_absent() {
        let s=Snapshot{extensions:[1.;4],open:[true;4],opening_order:[3,4,1,2],..Snapshot::default()};
        let r=layout(Viewport{width:1000.,height:600.,canvas_scale:1.},&s,Options::default()).unwrap();
        assert_eq!(r[0],Rect{x:218.,y:18.,width:582.,height:180.});
        assert_eq!(r[1],Rect{x:218.,y:420.,width:582.,height:180.});
    }
}

#[cfg(test)]
mod fullscreen_tests {
    use super::*;
    #[test]
    fn fullscreen_cycle_has_exact_three_quarter_extent_for_all_edges_and_swaps() {
        for (width,height) in [(1280.,720.),(720.,1280.)] {
            for swap_sides in [false,true] { for swap_horizontal in [false,true] {
                let view=Viewport{width,height,canvas_scale:1.};
                let options=Options{swap_sides,swap_horizontal,..Default::default()};
                let mut motion=super::super::panel_motion::State::default();
                motion.set_all_open(true);motion.advance(1.);
                let original=layout(view,&motion.snapshot(),options).unwrap();
                for edge in Edge::ALL {
                    motion.cycle_panel_view(edge);motion.advance(1.);
                    assert_eq!(motion.panel_view(),"extended");
                    let r=layout(view,&motion.snapshot(),options).unwrap()[edge as usize];
                    if matches!(edge,Edge::Top|Edge::Bottom) {
                        assert_eq!((r.width,r.height),(width,height*0.75));
                        let bottom=(edge==Edge::Bottom)!=swap_horizontal;
                        assert_eq!(r.y,if bottom {height*0.25}else{0.});
                    } else {
                        assert_eq!((r.width,r.height),(width*0.75,height));
                        let right=(edge==Edge::Right)!=swap_sides;
                        assert_eq!(r.x,if right {width*0.25}else{0.});
                    }
                    motion.cycle_panel_view(edge);motion.advance(1.);
                    assert_eq!(layout(view,&motion.snapshot(),options).unwrap()[edge as usize],Rect{x:0.,y:0.,width,height});
                    motion.cycle_panel_view(edge);motion.advance(1.);
                    assert_eq!(motion.panel_view(),"normal");
                    assert_eq!(layout(view,&motion.snapshot(),options).unwrap(),original);
                }
            }}
        }
    }
    #[test]
    fn full_gem_views_fill_resized_viewports_and_restore_exact_layout(){
        for (width,height) in [(1280.,720.),(720.,1280.),(2400.,1000.)] {
            for swap in [false,true] {
                let view=Viewport{width,height,canvas_scale:1.};
                let options=Options{swap_sides:swap,swap_horizontal:swap,..Default::default()};
                let mut s=super::super::panel_motion::State::default();s.set_all_open(true);
                for _ in 0..300{s.advance(1./60.);}
                s.advance(1.);
                let original=layout(view,&s.snapshot(),options).unwrap();
                for edge in Edge::ALL {
                    s.set_fullscreen(edge,true);for _ in 0..300{s.advance(1./60.);}
                    let r=layout(view,&s.snapshot(),options).unwrap();
                    assert_eq!(r[edge as usize],Rect{x:0.,y:0.,width,height});
                    for other in Edge::ALL.into_iter().filter(|e|*e!=edge) {
                        let r=r[other as usize];
                        assert!(r.x+r.width<=0. || r.x>=width || r.y+r.height<=0. || r.y>=height);
                    }
                }
                s.set_fullscreen(Edge::Right,false);for _ in 0..300{s.advance(1./60.);}
                assert_eq!(layout(view,&s.snapshot(),options).unwrap(),original);
            }
        }
    }
}

#[cfg(test)] mod fullscreen_quality_tests {
 use super::*;
 #[test] fn full_views_keep_normal_bevel_depth_and_content_margins(){
  for (width,height) in [(1280.,720.),(720.,1280.)] {
   let view=Viewport{width,height,canvas_scale:1.};let options=Options::default();
   let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
   for edge in Edge::ALL {
    let index=edge as usize;
    let normal=panel_shape(view,&motion.snapshot(),options,index).unwrap();
    motion.set_fullscreen(edge,true);motion.advance(1.);
    let full=panel_shape(view,&motion.snapshot(),options,index).unwrap();
    assert!((full.local_unit_m-normal.local_unit_m).abs()<1e-12);
    assert_eq!(full.dimensions_m[2],normal.dimensions_m[2]);
    let surface=resolved_surface(view,&motion.snapshot(),options,index).unwrap();
    assert_eq!(surface.size,full.safe_rectangle_m);
    assert!((full.dimensions_m[0]-full.safe_rectangle_m[0]-(normal.dimensions_m[0]-normal.safe_rectangle_m[0])).abs()<1e-12);
    motion.set_fullscreen(edge,false);motion.advance(1.);
   }
  }
 }
}
