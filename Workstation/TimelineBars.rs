//! Cardinal DawLaneGestureSystem.cpp:390–676 and DawLaneTimelineSystem.cpp:427–450.
//! Control-side geometry and captured gestures. No audio work or allocation here.
use super::piano_roll::{Input, Rect};

pub(crate) const ZOOM_HEIGHT: f64 = 26.;
pub(crate) const LOOP_HEIGHT: f64 = 13.;
pub(crate) const GAP: f64 = 10.;
pub(crate) const HEADER_HEIGHT: f64 = ZOOM_HEIGHT + LOOP_HEIGHT + GAP + 14.;
pub(crate) const MIN_SECONDS: f64 = 0.00001;
pub(crate) const MAX_SECONDS: f64 = 3600.;
const DAY: f64 = 86400.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LoopRange {
    pub start: f64,
    pub end: f64,
    pub enabled: bool,
}
impl Default for LoopRange {
    fn default() -> Self {
        // DawTrackSystem.cpp:644–656, original free/multi-clock initialization.
        Self { start: 0., end: 10., enabled: false }
    }
}
#[derive(Clone, Copy, Debug)]
enum Drag {
    Zoom { last: [f64; 2], initial_seconds: f64, dy: f64, direction: f64,
        anchor_fraction: f64, anchor_seconds: f64 },
    Move { origin_y: f64, offset: f64 },
    Start,
    End,
    Loop { offset: f64, length: f64 },
    Playhead { offset: f64 },
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    pub zoom: Rect,
    pub loop_face: Rect,
    pub playhead: Option<Rect>,
    pub lane_left: f64,
    pub lane_width: f64,
}
#[derive(Default)]
pub(crate) struct Bars {
    pub offset_y: f64,
    pub rebase_quantum_seconds:f64,
    pub range: LoopRange,
    drag: Option<Drag>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rebase {
    pub shift:f64,
    pub seek:Option<f64>,
    pub range:LoopRange,
}
/// Cardinal DawLaneShortcutSystem.cpp computeRebaseShiftSamples, free/multi-
/// clock branch: whole sample-rate seconds, plus two quanta before rounding up.
fn rebase_shift(negative:f64,rate:f64,seconds:f64)->f64 {
    let quantum=(rate*if seconds>0.{seconds}else{1.}).round().max(1.);
    ((-negative*rate+2.*quantum)/quantum).ceil()*quantum/rate
}
#[derive(Default, Debug)]
pub(crate) struct Update {
    pub consumed: bool,
    pub seek: Option<f64>,
    pub loop_changed: bool,
    pub rebase:Option<Rebase>,
    pub snap_unavailable:bool,
}
fn contains(r: Rect, p: [f64; 2]) -> bool {
    p[0] >= r.left && p[0] <= r.left + r.width && p[1] >= r.top && p[1] <= r.top + r.height
}
impl Bars {
    pub fn rebased(&mut self,shift:f64,range:LoopRange){
        self.range=range;
        if let Some(Drag::Zoom{anchor_seconds,..})=&mut self.drag{*anchor_seconds+=shift;}
    }
    pub fn cancel(&mut self) { self.drag = None; }
    pub fn active(&self) -> bool { self.drag.is_some() }
    pub fn geometry(&self, canvas: Rect, body: Rect, start: f64, seconds: f64,
        playhead: Option<f64>) -> Geometry {
        let min = canvas.top + LOOP_HEIGHT + GAP;
        let max = (canvas.top + canvas.height - ZOOM_HEIGHT).max(min);
        let zoom_top = (min + self.offset_y).clamp(min, max);
        // Cardinal extends the two bar faces fifteen points beyond lane edges.
        let left = body.left - 15.;
        let right = body.left + body.width + 15.;
        let x = |time: f64| body.left + (time - start) / seconds * body.width;
        let loop_left = x(self.range.start).clamp(left, right);
        let loop_right = x(self.range.end).clamp(left, right);
        Geometry {
            zoom: Rect { left, top: zoom_top, width: right-left, height: ZOOM_HEIGHT },
            loop_face: Rect { left: loop_left, top: zoom_top-GAP-LOOP_HEIGHT,
                width: (loop_right-loop_left).max(1.), height: LOOP_HEIGHT },
            playhead: playhead.map(|time| Rect { left: x(time).clamp(body.left, body.left+body.width)-6.,
                top: body.top-20., width: 12., height: 12. }),
            lane_left: body.left, lane_width: body.width,
        }
    }
    pub fn over_zoom(&self, canvas: Rect, body: Rect, start: f64, seconds: f64, point: [f64;2]) -> bool {
        contains(self.geometry(canvas, body, start, seconds, None).zoom, point)
    }
    pub fn select_loop(&mut self, a: f64, b: f64) -> bool {
        if !a.is_finite() || !b.is_finite() || a == b || a.min(b)<0. || a.max(b)>DAY { return false; }
        self.range=LoopRange {start:a.min(b),end:a.max(b),enabled:true};
        true
    }
    /// `snap` is the owning timeline's actual grid; never infer a new BPM here.
    /// `screen_height` is the window edge (not the movable canvas edge).
    pub fn update(&mut self, input: &Input, pressed: bool, canvas: Rect, body: Rect,
        start: &mut f64, seconds: &mut f64, playhead: Option<f64>, sample_rate: f64,
        dt: f64, screen_height: f64, snap: impl Fn(f64)->f64) -> Update {
        let mut out=Update::default();
        let Some(p)=input.pointer.filter(|p|p.iter().all(|v|v.is_finite())) else {self.cancel();return out;};
        if input.escape || body.width<=0. || !start.is_finite() || !seconds.is_finite() || *seconds<=0. {
            out.consumed=self.active();self.cancel();return out;
        }
        let g=self.geometry(canvas,body,*start,*seconds,playhead);
        let fraction=|x:f64|((x-body.left)/body.width).clamp(0.,1.);
        if pressed && self.drag.is_none() {
            self.drag=if g.playhead.is_some_and(|r|contains(r,p)) {
                Some(Drag::Playhead {offset:g.playhead.unwrap().left+6.-p[0]})
            } else if contains(g.zoom,p) {
                if input.shift { Some(Drag::Move {origin_y:p[1],offset:self.offset_y}) }
                else { let f=fraction(p[0]);Some(Drag::Zoom {last:p,initial_seconds:*seconds,dy:0.,direction:0.,anchor_fraction:f,anchor_seconds:*start+f**seconds}) }
            } else if p[1]>=g.loop_face.top && p[1]<=g.loop_face.top+g.loop_face.height
                && p[0]>=g.zoom.left && p[0]<=g.zoom.left+g.zoom.width {
                let left=g.loop_face.left;let right=left+g.loop_face.width;
                if (p[0]-left).abs()<=8. {Some(Drag::Start)}
                else if (p[0]-right).abs()<=8. {Some(Drag::End)}
                else if p[0]>=left && p[0]<=right {Some(Drag::Loop {
                    offset:((*start+fraction(p[0])**seconds)*sample_rate.max(1.)).round()/sample_rate.max(1.)-self.range.start,
                    length:self.range.end-self.range.start})}
                else {None}
            } else {None};
            if self.active(){out.consumed=true;return out;}
        }
        if let Some(mut drag)=self.drag {
            out.consumed=true;
            // Cardinal ends capture when released. Do not turn release into a seek.
            if !input.left {self.cancel();return out;}
            let rate=if sample_rate.is_finite() && sample_rate>0. {sample_rate}else{44100.};
            let tick=1./rate;
            match &mut drag {
                Drag::Zoom {last,initial_seconds,dy,direction,anchor_fraction,anchor_seconds} => {
                    let dx=p[0]-last[0];let mut delta=p[1]-last[1];*last=p;
                    if delta.abs()>0.001 {*direction=delta.signum();}
                    let dt=if dt.is_finite(){dt.clamp(0.,0.1)}else{0.};
                    if p[1]<=1. && *direction<0. {delta-=420.*dt;}
                    else if p[1]>=screen_height-1. && *direction>0. {delta+=420.*dt;}
                    *dy+=delta;
                    let next=(*initial_seconds*(-*dy*0.01).exp()).clamp(MIN_SECONDS,MAX_SECONDS);
                    *anchor_seconds-=dx*next/body.width;
                    if dx!=0. || delta!=0. {
                        *seconds=next;*start=(*anchor_seconds-*anchor_fraction*next).clamp(-DAY,DAY-next);
                    }
                }
                Drag::Move {origin_y,offset} => {
                    let limit=(canvas.height-ZOOM_HEIGHT-LOOP_HEIGHT-GAP).max(0.);
                    self.offset_y=(*offset+p[1]-*origin_y).clamp(0.,limit);
                }
                Drag::Playhead {offset} => {
                    let raw=((*start+fraction(p[0]+*offset)**seconds)*rate).round()/rate;
                    if raw<0. {
                        let shift=rebase_shift(raw,rate,self.rebase_quantum_seconds);
                        out.rebase=Some(Rebase{shift,seek:Some(raw+shift),range:LoopRange{
                            start:self.range.start+shift,end:self.range.end+shift,enabled:self.range.enabled}});
                    }else{out.seek=Some(raw.min(DAY));}
                }
                mode => {
                    let old=self.range;
                    let raw=(((*start+fraction(p[0])**seconds).clamp(-DAY,DAY)*rate).round())/rate;
                    let mut shift=if raw<0.{rebase_shift(raw,rate,self.rebase_quantum_seconds)}else{0.};
                    let target=if input.command {raw}else{snap(raw)};
                    if target.is_finite() {
                        let target=(target.clamp(-DAY,DAY)*rate).round()/rate;
                        let mut range=old;
                        match mode {
                            Drag::Start => range.start=target.min(range.end-tick),
                            Drag::End => range.end=target.max(range.start+tick).min(DAY),
                            Drag::Loop {offset,length} => {
                                range.start=(target-*offset).min(DAY-*length);
                                range.end=range.start+*length;
                            }
                            _=>unreachable!(),
                        }
                        // Original rebases the raw cursor before snapping and
                        // endpoint constraints, then rebases a still-negative
                        // body start a second time. Preserve that exact padding.
                        if matches!(mode,Drag::Loop{..}) && range.start+shift<0. {
                            shift+=rebase_shift(range.start+shift,rate,self.rebase_quantum_seconds);
                        }
                        if shift>0. {
                            range.start+=shift;range.end+=shift;
                            out.rebase=Some(Rebase{shift,seek:None,range});
                        }else{self.range=range;out.loop_changed=range!=old;}
                    }else{out.snap_unavailable=true;}
                }
            }
            self.drag=Some(drag);return out;
        }
        if contains(g.zoom,p) && input.wheel[1].is_finite() && input.wheel[1]!=0. {
            let f=fraction(p[0]);let anchor=*start+f**seconds;
            // Original uses sign only, not event magnitude, with no modifier.
            *seconds=(*seconds*if input.wheel[1]>0. {1.18}else{1./1.18}).clamp(MIN_SECONDS,MAX_SECONDS);
            *start=(anchor-f**seconds).clamp(-DAY,DAY-*seconds);out.consumed=true;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn canvas()->Rect {Rect{left:0.,top:0.,width:1000.,height:600.}}
    fn body()->Rect {Rect{left:100.,top:HEADER_HEIGHT,width:800.,height:600.-HEADER_HEIGHT}}
    fn input(x:f64,y:f64,left:bool)->Input {Input{pointer:Some([x,y]),left,..Default::default()}}
    fn step(b:&mut Bars,i:Input,pressed:bool,start:&mut f64,span:&mut f64)->Update {
        b.update(&i,pressed,canvas(),body(),start,span,None,48000.,1./60.,600.,|x|(x/2.).round()*2.)
    }
    fn close(a:f64,b:f64){assert!((a-b).abs()<1e-9,"{a} != {b}");}
    #[test] fn stationary_grab_release_never_seeks_or_changes_viewport() {
        let mut b=Bars::default();let(mut a,mut s)=(5.,16.);
        for (held,pressed) in [(true,true),(true,false),(false,false)] {
            let u=step(&mut b,input(500.,35.,held),pressed,&mut a,&mut s);
            assert!(u.consumed);assert!(u.seek.is_none());assert!(!u.loop_changed);close(a,5.);close(s,16.);
        }
        assert!(!b.active());
    }
    #[test] fn horizontal_pan_and_vertical_exponential_zoom_keep_grab_anchor() {
        let mut b=Bars::default();let(mut a,mut s)=(10.,16.);
        step(&mut b,input(500.,35.,true),true,&mut a,&mut s);
        step(&mut b,input(600.,35.,true),false,&mut a,&mut s);close(a,8.);close(s,16.);
        step(&mut b,input(600.,85.,true),false,&mut a,&mut s);
        close(s,16.*(-0.5f64).exp());close(a+s*0.5,16.);
    }
    #[test] fn wheel_is_unmodified_sign_only_and_pointer_anchored() {
        for magnitude in [1.,9.] {let mut b=Bars::default();let(mut a,mut s)=(10.,16.);
            let mut i=input(500.,35.,false);i.wheel=[0.,magnitude];
            assert!(step(&mut b,i,false,&mut a,&mut s).consumed);close(s,16.*1.18);close(a+s/2.,18.);
        }
    }
    #[test] fn shift_moves_both_faces_and_clamps_complete_assembly_inside_canvas() {
        let mut b=Bars::default();let(mut a,mut s)=(0.,16.);let mut i=input(500.,35.,true);i.shift=true;
        step(&mut b,i.clone(),true,&mut a,&mut s);i.pointer=Some([500.,2000.]);step(&mut b,i.clone(),false,&mut a,&mut s);
        let g=b.geometry(canvas(),body(),a,s,None);close(g.zoom.top+g.zoom.height,600.);close(g.zoom.top-(g.loop_face.top+g.loop_face.height),10.);
        i.pointer=Some([500.,-2000.]);step(&mut b,i,false,&mut a,&mut s);close(b.geometry(canvas(),body(),a,s,None).loop_face.top,0.);
        close(a,0.);close(s,16.);
    }
    #[test] fn edge_zoom_continues_at_screen_edge_and_obeys_original_limits() {
        let mut b=Bars::default();let(mut a,mut s)=(10.,16.);
        step(&mut b,input(500.,35.,true),true,&mut a,&mut s);step(&mut b,input(500.,0.,true),false,&mut a,&mut s);
        let before=s;step(&mut b,input(500.,0.,true),false,&mut a,&mut s);assert!(s>before);
        for _ in 0..1000{step(&mut b,input(500.,0.,true),false,&mut a,&mut s);}close(s,MAX_SECONDS);
        step(&mut b,input(500.,600.,true),false,&mut a,&mut s);
        for _ in 0..2000{step(&mut b,input(500.,600.,true),false,&mut a,&mut s);}close(s,MIN_SECONDS);
    }
    #[test] fn loop_handles_use_eight_point_hit_region_and_cannot_cross() {
        let mut b=Bars::default();let(mut a,mut s)=(0.,16.);
        step(&mut b,input(608.,6.,true),true,&mut a,&mut s); // end at 600, +8 hit margin
        assert!(b.active());step(&mut b,input(100.,6.,true),false,&mut a,&mut s);
        close(b.range.end,1./48000.);assert!(!b.range.enabled);
    }
    #[test] fn command_bypasses_loop_snap_and_body_drag_preserves_length() {
        let mut b=Bars::default();b.range=LoopRange{start:2.,end:10.,enabled:false};let(mut a,mut s)=(0.,16.);
        step(&mut b,input(400.,6.,true),true,&mut a,&mut s);
        let mut i=input(463.,6.,true);i.command=true;let u=step(&mut b,i,false,&mut a,&mut s);
        assert!(u.loop_changed);close(b.range.start,3.26);close(b.range.end-b.range.start,8.);assert!(!b.range.enabled);
    }
    #[test] fn loop_drag_snaps_target_without_enabling_transport() {
        let mut b=Bars::default();let(mut a,mut s)=(0.,16.);
        step(&mut b,input(600.,6.,true),true,&mut a,&mut s);
        step(&mut b,input(663.,6.,true),false,&mut a,&mut s);close(b.range.end,12.);assert!(!b.range.enabled);
        assert!(b.select_loop(9.,3.));assert_eq!(b.range,LoopRange{start:3.,end:9.,enabled:true});
    }
    #[test] fn pointer_loss_and_escape_cancel_capture() {
        for escape in [false,true] {
            let mut b=Bars::default();let(mut a,mut s)=(0.,16.);
            step(&mut b,input(500.,35.,true),true,&mut a,&mut s);
            let mut i=input(600.,40.,true);i.escape=escape;
            if !escape {i.pointer=None;}
            step(&mut b,i,false,&mut a,&mut s);assert!(!b.active());close(a,0.);close(s,16.);
        }
    }
    #[test] fn panel_resize_preserves_zoom_and_loop_capture() {
        for y in [35.,6.] {
            let mut b=Bars::default();let(mut a,mut s)=(0.,16.);
            step(&mut b,input(500.,y,true),true,&mut a,&mut s);
            let mut c=canvas();c.width-=100.;
            let mut lane=body();lane.width-=100.;
            let update=b.update(&input(550.,y+2.,true),false,c,lane,&mut a,&mut s,None,48000.,0.01,600.,|x|x);
            assert!(b.active() && update.consumed);
            assert!(if y==6. {update.loop_changed} else {s!=16.});
        }
    }
    #[test] fn playhead_handle_drags_continuously_but_grab_is_inert() {
        let mut b=Bars::default();let(mut a,mut s)=(0.,16.);
        let g=b.geometry(canvas(),body(),a,s,Some(4.));let r=g.playhead.unwrap();
        let mut i=input(r.left+6.,r.top+6.,true);
        assert!(b.update(&i,true,canvas(),body(),&mut a,&mut s,Some(4.),48000.,0.,600.,|x|x).seek.is_none());
        i.pointer=Some([500.,r.top+6.]);close(b.update(&i,false,canvas(),body(),&mut a,&mut s,Some(4.),48000.,0.,600.,|x|x).seek.unwrap(),8.);
    }
    #[test]fn negative_loop_drag_proposes_original_free_clock_rebase_before_mutating(){
        let mut b=Bars::default();let(mut a,mut span)=(-4.,16.);let before=b.range;
        step(&mut b,input(300.,6.,true),true,&mut a,&mut span);
        let mut i=input(200.,6.,true);i.command=true;
        let u=step(&mut b,i,false,&mut a,&mut span);let r=u.rebase.unwrap();
        close(r.shift,4.);close(r.range.start,2.);close(r.range.end,14.);
        assert_eq!(b.range,before);assert!(!u.loop_changed);assert!(b.active());
        b.rebased(r.shift,r.range);a+=r.shift;
        close(b.geometry(canvas(),body(),a,span,None).loop_face.left,200.);
    }
    #[test]fn negative_playhead_rebase_uses_single_clock_four_quarter_quantum(){
        let mut b=Bars::default();b.rebase_quantum_seconds=2.;let(mut a,mut span)=(-4.,16.);
        let g=b.geometry(canvas(),body(),a,span,Some(0.));let h=g.playhead.unwrap();
        let mut i=input(h.left+6.,h.top+6.,true);
        b.update(&i,true,canvas(),body(),&mut a,&mut span,Some(0.),48000.,0.,600.,|x|x);
        i.pointer=Some([200.,h.top+6.]);let u=b.update(&i,false,canvas(),body(),&mut a,&mut span,Some(0.),48000.,0.,600.,|x|x);
        let r=u.rebase.unwrap();close(r.shift,6.);close(r.seek.unwrap(),4.);assert_eq!(b.range,LoopRange::default());
    }

}
