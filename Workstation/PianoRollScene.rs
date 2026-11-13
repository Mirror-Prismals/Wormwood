//! Projection/input adapter for the open air between the visible panel models.
//! This helper is owned by AudioWorkstationSystem; it never mutates the document
//! or invokes another System. All positions are logical pixels until projection.
use super::{panel_layout::{self, Options, Viewport}, panel_motion::Snapshot,
    piano_roll::{Editor, Input, Rect}};
use serde_json::{Value, json};

pub(crate) fn canvas(view: Viewport, motion: &Snapshot, options: Options) -> Result<Rect, String> {
    if !motion.timeline_visible() && motion.mobile_page.is_none() {return Ok(Rect{left:0.,top:0.,width:0.,height:0.});}
    if let Some(page)=motion.mobile_page {
        return Ok(if page==super::panel_motion::MobilePage::Timeline {
            Rect{left:8.,top:8.,width:(view.width-16.).max(0.),height:(view.height-16.).max(0.)}
        } else {Rect{left:0.,top:0.,width:0.,height:0.}});
    }
    let panels = panel_layout::visual_bounds(view, motion, options)?;
    Ok(canvas_from_bounds(view, panels, options))
}

/// Keep the timeline projection fixed while its mobile cover slides. Input
/// continues using `canvas`, which stays empty for every active panel page.
pub(crate) fn render_canvas(view: Viewport, motion: &Snapshot, options: Options) -> Result<Rect, String> {
    if matches!(motion.mobile_page, Some(super::panel_motion::MobilePage::Panel(_))) && motion.mobile_progress < 1. {
        return Ok(Rect{left:8.,top:8.,width:(view.width-16.).max(0.),height:(view.height-16.).max(0.)});
    }
    canvas(view,motion,options)
}

pub(super) fn canvas_from_bounds(view: Viewport, panels: [panel_layout::Rect;4], options: Options) -> Rect {
    let (top, bottom) = if options.swap_horizontal {(1,0)} else {(0,1)};
    let (left, right) = if options.swap_sides {(3,2)} else {(2,3)};
    let x = (panels[left].x + panels[left].width).max(0.).min(view.width);
    let end = panels[right].x.max(0.).min(view.width);
    let y = (panels[top].y + panels[top].height).max(0.).min(view.height);
    let lower = panels[bottom].y.max(0.).min(view.height);
    // A fully expanded panel can cover the editor. Keep a finite transform but
    // return a zero-area hit region instead of drawing through a panel.
    Rect {left:x+8.,top:y+8.,width:(end-x-16.).max(0.),height:(lower-y-16.).max(0.)}
}
pub(crate) fn contains(rect: Rect, p: [f64;2]) -> bool {
    rect.width>1. && rect.height>1. && p[0]>=rect.left && p[0]<rect.left+rect.width
        && p[1]>=rect.top && p[1]<rect.top+rect.height
}
pub(crate) fn input(value: &Value) -> Input {
    let pointer = value["pointer"].as_array().filter(|a|a.len()==2).and_then(|a| {
        let p=[a[0].as_f64()?,a[1].as_f64()?];
        p.iter().all(|x|x.is_finite()).then_some(p)
    });
    let wheel = serde_json::from_value::<[f64;2]>(value["wheel"].clone()).ok()
        .filter(|v|v.iter().all(|x|x.is_finite())).unwrap_or([0.;2]);
    Input {pointer,left:value["left"]==true,right:value["right"]==true,
        shift:value["shift"]==true,alt:value["alt"]==true,command:value["command"]==true,
        wheel,delete:value["delete"]==true,escape:value["escape"]==true,
        draw:value["draw"]==true,paint:value["paint"]==true,snap_toggle:value["snap_toggle"]==true,
        tab:value["tab"]==true,enter:value["enter"]==true,home:value["home"]==true,
        key_events:value["key_events"]==true,enter_pressed:value["enter_pressed"]==true,
        split_clip_pressed:value["split_clip_pressed"]==true,
        create_range_pressed:value["create_range_pressed"]==true,
        loop_range_pressed:value["loop_range_pressed"]==true,
        enter_modifiers:value["enter_modifiers"].as_u64().unwrap_or(0).min(255) as u8,
        undo:value["undo"]==true,redo:value["redo"]==true,
        arrows:std::array::from_fn(|i|value["arrows"][i]==true),..Input::default()}
}
pub(crate) fn presentation(editor: &Editor, rect: Rect, view: Viewport) -> Value {
    if !editor.is_open() || rect.width<=1. || rect.height<=1. {return Value::Null;}
    let clock=editor.viewport();
    let mpp=1.3/view.height;
    let height=(rect.height/clock.key_span())*super::piano_key_layout::WHITE_HEIGHT*mpp;
    let world=|p:[f64;2]|[(p[0]-view.width*0.5)*mpp,(view.height*0.5-p[1])*mpp];
    let lo=world([rect.left,rect.top+rect.height]);
    let hi=world([rect.left+rect.width,rect.top]);
    let edo=editor.status()["edo"].as_f64().unwrap_or(12.);
    let notes:Vec<Value>=editor.notes().into_iter().filter_map(|note| {
        let note_height=clock.note_height(note.pitch,rect)*mpp;
        let p=clock.project(note.quarter,note.pitch as f64,rect);
        let end=clock.project(note.quarter+note.length_quarters,note.pitch as f64,rect);
        let end_y=clock.project(note.quarter,note.pitch as f64+note.bend*edo/12.,rect)[1];
        let bend_y=(p[1]-end_y)*mpp;
        // Cull whole models only. Duration remains exact at the viewport edges;
        // the native triangle query clips the shared open-air editing volume.
        if end[0]<rect.left || p[0]>rect.left+rect.width
            || p[1].max(end_y)+height/mpp<rect.top || p[1].min(end_y)-height/mpp>rect.top+rect.height {return None;}
        let w=world(p);
        // Window geometry only beyond the visible field plus two note heights.
        // This keeps extreme pan/zoom numerically local; every visible onset
        // and end retains its exact projected position and source timing.
        let x=w[0].max(lo[0]-2.*height);
        let original_end=world(end)[0];
        let end_x=original_end.min(hi[0]+2.*height);
        let full=(original_end-w[0]).max(1e-12);
        let u=((x-w[0])/full).clamp(0.,1.);let d=((end_x-x)/full).clamp(0.,1.);
        let bend_curve=[bend_y*u*u*(3.-2.*u),bend_y*6.*u*(1.-u)*d,
            bend_y*(3.-6.*u)*d*d,-2.*bend_y*d*d*d];
        Some(json!({"id":note.id,"x":x,"y":w[1],"length":(end_x-x).max(1e-12),
            "bend_curve":bend_curve,"height_m":note_height,"age":note.placement_age,"selected":note.selected,"pitch":note.pitch}))
    }).collect();
    json!({"height_m":height,"notes":notes,"clip_rect":[lo[0],lo[1],hi[0],hi[1]]})
}

#[cfg(test)] mod mobile_canvas_tests {
    use super::*;use super::super::panel_motion::{MobilePage,Edge};
    #[test] fn mobile_timeline_uses_fullscreen_and_panel_pages_hide_editor_hits() {
        let view=Viewport{width:390.,height:844.,canvas_scale:1.};
        let mut motion=Snapshot{mobile_page:Some(MobilePage::Timeline),..Snapshot::default()};
        let r=canvas(view,&motion,Options::default()).unwrap();assert_eq!([r.left,r.top,r.width,r.height],[8.,8.,374.,828.]);
        for edge in Edge::ALL {motion.mobile_page=Some(MobilePage::Panel(edge));let r=canvas(view,&motion,Options::default()).unwrap();assert_eq!([r.width,r.height],[0.,0.]);assert!(!contains(r,[195.,422.]));}
    }
}

/// The native bounded queue preserves complete clicks when a slow game/camera
/// frame spans both button callbacks. Reject the whole malformed batch.
pub(crate) fn pointer_edges(value:&Value)->Result<Vec<([f64;2],bool)>,String>{
    if value.is_null(){return Ok(Vec::new());}
    let events=value.as_array().filter(|v|v.len()<=16).ok_or("invalid piano pointer event batch")?;
    events.iter().map(|e|{let p=serde_json::from_value::<[f64;2]>(e["pointer"].clone()).map_err(|_|"invalid piano pointer coordinates")?;
        if !p.iter().all(|x|x.is_finite()){return Err("invalid piano pointer coordinates".into());}
        Ok((p,e["left"].as_bool().ok_or("invalid piano pointer button")?))}).collect()
}
