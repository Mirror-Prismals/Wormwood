//! Live cabinet wave nets and ray ribbons. No fabricated signal or audio DSP.
use crate::{audio::{*, geometry::Geometry}, host::Host};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const QUIET: f64 = 0.003981;
const MAX_TRIANGLES: usize = 24576;
const VISUAL_SPEED: f64 = 12.; // Cardinal's intentionally slowed visual front.
#[derive(Clone, Copy, Default)]
struct Beam { origin: V, direction: V, limit: f64, valid: bool }
struct Grid { beams: Vec<Beam>, columns: usize, next: usize, forward: V, up: V }
#[derive(Default)]
pub(crate) struct State { pub(crate) sources:Value, pub(crate) show_speakers:bool, grids: BTreeMap<u64, Grid>, next_source: usize, revision: u64 }
#[derive(Clone, Copy)]
struct Signal { id:u64, origin:V, forward:V, up:V, peak:f64, gain:f64, omni:bool, range:f64, power:f64 }
type Vertex = (V, [f32;4]);
fn color(level: f64) -> [f32;4] {
    const SCALE: [V;6] = [[0.55,0.12,0.9],[0.12,0.3,1.],[0.05,0.9,0.9],[0.2,1.,0.2],[1.,0.9,0.1],[1.,0.15,0.05]];
    let index = ((20.*level.max(QUIET).log10()+48.)/48.).clamp(0.,1.)*5.;
    let low = (index as usize).min(4);
    let fraction = index-low as f64;
    let rgb:V = std::array::from_fn(|i| SCALE[low][i]*(1.-fraction)+SCALE[low+1][i]*fraction);
    [rgb[0] as f32,rgb[1] as f32,rgb[2] as f32,(0.85*((level/QUIET-1.)*2.).clamp(0.,1.)) as f32]
}
fn side(a:V,b:V,camera:V,width:f64)->V {
    let direction=unit(sub(b,a));
    let mut s=cross(direction,sub(camera,mul(add(a,b),0.5)));
    if length(s)<0.001 {s=cross(direction,[0.,1.,0.]);}
    if length(s)<0.001 {s=[1.,0.,0.];}
    mul(unit(s),width)
}
fn edge(out:&mut Vec<Vertex>,a:Vertex,b:Vertex,camera:V,width:f64) {
    if out.len()/3+2>MAX_TRIANGLES || length(sub(b.0,a.0))<0.001 || (a.1[3]<=0. && b.1[3]<=0.) {return;}
    let s=side(a.0,b.0,camera,width);
    let al=(sub(a.0,s),a.1);let ar=(add(a.0,s),a.1);
    let bl=(sub(b.0,s),b.1);let br=(add(b.0,s),b.1);
    out.extend([al,bl,br,al,br,ar]);
}
fn beam_direction(signal:Signal,columns:usize,index:usize)->V {
    let horizontal=if signal.omni {360_f64}else{90_f64};
    let vertical=if signal.omni {360_f64}else{25_f64};
    let h=horizontal.to_radians()*(index as f64%(columns as f64)/(columns-1) as f64-0.5);
    let v=vertical.to_radians()*((index/columns) as f64/4.-0.5);
    let right=unit(cross(signal.up,signal.forward));
    add(add(mul(right,h.sin()*v.cos()),mul(signal.up,v.sin())),mul(signal.forward,h.cos()*v.cos()))
}
fn pressure(signal:Signal,distance:f64)->f64 {signal.peak*signal.gain*(4./distance.max(4.)).powf(signal.power)}
fn parse_signals(input:&Value)->Result<Vec<Signal>,String> {
    let Some(rows)=input["cabinet_signals"].as_array().or_else(||input["cabinets"].as_array()) else{return Ok(Vec::new());};
    if rows.len()>24 {return Err("cabinet visualizer source budget exceeded".into());}
    rows.iter().map(|row| {
        let peak=row["peak"].as_f64().filter(|x|x.is_finite() && *x>=0.).ok_or("finite cabinet peak required")?;
        let gain=row["gain"].as_f64().filter(|x|x.is_finite() && *x>=0.).unwrap_or(1.);
        Ok(Signal{id:row["id"].as_u64().ok_or("cabinet id required")?,origin:vector(&row["position"])?,forward:unit(vector(&row["forward"])?),up:unit(vector(&row["up"])?),peak,gain,omni:row["omni"]==true,range:128.,power:1.})
    }).collect()
}
impl State {
    fn geometry(&mut self,signals:&[Signal],segments:&Value,geometry:&dyn Geometry,camera:V,time:f64)->Vec<Vertex> {
        // Accepted soil/wood can change every capture. Refresh cached limits
        // round-robin instead of continually starving the later emitters.
        self.revision=geometry.revision();
        self.grids.retain(|id,_|signals.iter().any(|s|s.id==*id));
        for signal in signals {
            let columns=if signal.omni {17}else{13};
            let grid=self.grids.entry(signal.id).or_insert_with(||Grid{beams:vec![Beam::default();columns*5],columns,next:0,forward:signal.forward,up:signal.up});
            if grid.columns!=columns || grid.forward!=signal.forward || grid.up!=signal.up {
                *grid=Grid{beams:vec![Beam::default();columns*5],columns,next:0,forward:signal.forward,up:signal.up};
            }
        }
        let active:Vec<_>=signals.iter().copied().filter(|s|s.peak*s.gain>QUIET).collect();
        let mut out=Vec::new();
        if active.is_empty(){return out;}
        for _ in 0..32 {
            let signal=active[self.next_source%active.len()];self.next_source=self.next_source.wrapping_add(1);
            let grid=self.grids.get_mut(&signal.id).unwrap();
            let index=grid.next%grid.beams.len();grid.next=grid.next.wrapping_add(1);
            let direction=beam_direction(signal,grid.columns,index);
            let limit=geometry.trace(signal.origin,direction,signal.range).map_or(signal.range,|h|(h.distance-0.02).max(0.));
            grid.beams[index]=Beam{origin:signal.origin,direction,limit,valid:true};
        }
        // Keep intersected ribbons pinned at both endpoints, even while they wiggle.
        for row in segments.as_array().into_iter().flatten().take(256) {
            let Some(signal)=row["source"].as_u64().and_then(|id|signals.iter().find(|s|s.id==id)).or_else(||(row["kind"]=="reflection").then(||signals.iter().max_by(|a,b|(a.peak*a.gain).total_cmp(&(b.peak*b.gain)))).flatten()).copied() else{continue;};
            if signal.peak*signal.gain<=QUIET{continue;}
            let (Ok(from),Ok(to))=(vector(&row["from"]),vector(&row["to"]))else{continue;};
            let delta=sub(to,from);let distance=length(delta);if distance<0.001{continue;}
            let steps=(distance*4.).ceil().clamp(4.,48.) as usize;
            let axis=side(from,to,camera,1.);
            let point=|i:usize| {
                let t=i as f64/steps as f64;let center=add(from,mul(delta,t));
                let level=pressure(signal,length(sub(center,signal.origin)));
                let phase=std::f64::consts::TAU*(distance*t-time*VISUAL_SPEED)/(distance/12.).max(2.);
                let envelope=(t.min(1.-t)*12.).min(1.)*level.min(1.);
                (add(center,mul(axis,phase.sin()*envelope)),if row["microphone"]==true{[0.65,0.2,0.95,color(level)[3]]}else{color(level)})
            };
            for i in 1..=steps{edge(&mut out,point(i-1),point(i),camera,0.018);}
        }
        for signal in active {
            let grid=&self.grids[&signal.id];
            let reach=(4.*(signal.peak*signal.gain/QUIET).powf(1./signal.power)).min(signal.range);
            let spacing=8_f64.min(reach/3.);
            let phase=(time*VISUAL_SPEED+(signal.id%11)as f64*0.35).rem_euclid(spacing);
            let fronts=((reach-0.25-phase)/spacing).floor().max(0.) as usize+1;
            // Four sampled fronts per emitter bound geometry independently of signal level.
            let stride=fronts.div_ceil(4).max(1);
            for front in (0..fronts).step_by(stride) {
                let radius=0.25+phase+front as f64*spacing;
                let point=|index:usize| -> Option<Vertex> {
                    let beam=grid.beams[index];if !beam.valid{return None;}
                    let column=index%grid.columns;let row=index/grid.columns;
                    let offset=0.45*signal.peak.min(1.)*(time*8.+column as f64*0.65+row as f64*0.8).sin();
                    let d=(radius+offset).max(0.1);if d>beam.limit || d>reach{return None;}
                    let h=2.*column as f64/(grid.columns-1) as f64-1.;let v=2.*row as f64/4.-1.;
                    let polar=if signal.omni{1.}else{(-std::f64::consts::LN_2*(h*h+v*v)).exp()};
                    let c=color(pressure(signal,d)*polar);if c[3]<=0.{return None;}
                    Some((add(beam.origin,mul(beam.direction,d)),c))
                };
                for i in 0..grid.beams.len() {
                    let Some(a)=point(i)else{continue;};
                    if i%grid.columns+1<grid.columns {if let Some(b)=point(i+1){edge(&mut out,a,b,camera,0.025);}}
                    if i+grid.columns<grid.beams.len(){if let Some(b)=point(i+grid.columns){edge(&mut out,a,b,camera,0.025);}}
                }
            }
        }
        out
    }
}
fn visible(enabled:bool,daw:bool,worn:bool)->bool{enabled && (daw || worn)}
fn speaker_geometry(host:&Host,input:&Value)->Result<Vec<[f32;24]>,String> {
    if !host.audio_visualizer.show_speakers {return Ok(Vec::new());}
    static MODEL:std::sync::OnceLock<Vec<[f32;6]>>=std::sync::OnceLock::new();
    let model=MODEL.get_or_init(||serde_json::from_str(include_str!("../../Art/Audio/OmniSpeaker/runtime.json")).expect("authored omni speaker mesh"));
    let camera=vector(&input["position"])?;
    let mut vertices=Vec::new();
    // Published acoustic source coordinates are authoritative. In particular,
    // the wind's authored listener-relative placeholder never drives this model.
    for row in host.audio_visualizer.sources.as_array().into_iter().flatten() {
        let Some(response)=row["id"].as_u64().and_then(|id|host.ray_audio.engine.responses.get(&id))else{continue;};
        let center=sub(response.source,camera);
        if length(center)>160. {continue;}
        for v in model {vertices.push([v[0]+center[0] as f32,v[1]+center[1] as f32,v[2]+center[2] as f32,v[3],v[4],v[5]]);}
    }
    let mut view=input.clone();view["position"]=json!([0.,0.,0.]);view["two_sided"]=json!(true);
    Ok(super::appearance_system::projection::project(&vertices,&view))
}
pub(crate) fn frame(host:&mut Host,input:&Value)->Result<Vec<[f32;24]>,String> {
    let mut speakers=speaker_geometry(host,input)?;
    if input["audio_rays_visible"]==false || !visible(input["audio_rays_visible"]==true || host.audio_workstation.audio_rays,input["audio_rays_visible"]==true,host.glasses.worn()) {return Ok(speakers);}
    let fallback=host.environment_audio.theatre_audio();
    let audio=if input["theatre_audio"].is_object(){&input["theatre_audio"]}else{&fallback};
    let camera=vector(&input["position"])?;
    let mut signals=if audio["enabled"]==true {parse_signals(audio)?}else{Vec::new()};
    let telemetry=if host.chuck.prepared_entity().is_some(){host.invoke("ChuckConnectionSystem.Telemetry",Value::Null)?}else{Value::Null};
    for source in host.audio_visualizer.sources.as_array().into_iter().flatten(){
        let channel=source["channel"].as_u64().unwrap_or(17) as usize;
        let peak=telemetry["source_peaks"][channel].as_f64().unwrap_or(0.);
        signals.push(Signal{id:source["id"].as_u64().ok_or("world source id required")?,origin:vector(&source["position"])?,forward:[0.,0.,1.],up:[0.,1.,0.],peak,gain:source["ambient_gain"].as_f64().unwrap_or(1.),omni:true,range:64.,power:1.5});
    }
    let mut segments=audio["segments"].as_array().into_iter().flatten().take(64).cloned().collect::<Vec<_>>();
    for ray in host.ray_audio.engine.debug.iter().take(128){segments.push(json!({"from":ray.from,"to":ray.to,"source":ray.source,"kind":ray.kind}));}
    for ray in host.ray_audio.microphone.debug.iter().take(64){segments.push(json!({"from":ray.from,"to":ray.to,"source":ray.source,"kind":ray.kind,"microphone":true}));}
    let segments=json!(segments);
    let Some(geometry)=host.ray_audio.terrain_geometry.as_deref()else{return Ok(speakers);};
    let vertices=host.audio_visualizer.geometry(&signals,&segments,geometry,camera,input["time"].as_f64().unwrap_or(0.));
    let local:Vec<[f32;6]>=vertices.iter().map(|(p,c)|[(p[0]-camera[0])as f32,(p[1]-camera[1])as f32,(p[2]-camera[2])as f32,c[0],c[1],c[2]]).collect();
    let mut view=input.clone();view["position"]=json!([0.,0.,0.]);view["two_sided"]=json!(true);view["far"]=json!(length(camera)+2.*crate::terrain::RADIUS);
    // The shared scene pass writes depth. Opaque thin strokes retain correct
    // occlusion with later vegetation/water; fractional alpha here would not.
    speakers.extend(super::appearance_system::projection::project(&local,&view));
    Ok(speakers)
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String> {
    match hook {"Frame"=>Ok(json!({"triangles":frame(host,input)?})),"Clear"=>{host.audio_visualizer=State::default();Ok(Value::Null)},_=>Err("unknown audio visualizer hook".into())}
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Wall;
    impl Geometry for Wall {
        fn revision(&self)->u64{1}
        fn trace(&self,o:V,d:V,_:f64)->Option<geometry::Hit>{Some(geometry::Hit{position:add(o,mul(d,2.)),normal:mul(d,-1.),distance:2.})}
        fn wall_distance(&self,_:V,_:V,_:f64)->f64{0.}
    }
    #[test]
    fn silence_is_empty_and_all_sampled_fronts_stop_at_geometry(){
        let mut state=State::default();let mut signal=Signal{id:1,origin:[0.;3],forward:[0.,0.,1.],up:[0.,1.,0.],peak:0.,gain:1.,omni:false,range:128.,power:1.};
        assert!(state.geometry(&[signal],&Value::Null,&Wall,[0.,0.,-5.],0.).is_empty());
        signal.peak=1.;let mut total=0;
        for i in 0..20{let v=state.geometry(&[signal],&Value::Null,&Wall,[0.,0.,-5.],i as f64*0.01);total+=v.len();assert!(v.iter().all(|(p,_)|length(*p)<2.04));}
        assert!(total>0);
    }
}

#[cfg(test)] mod wearable_ray_tests {
 #[test] fn daw_toggle_and_worn_glasses_gate_world_rays(){for enabled in [false,true]{for daw in [false,true]{for worn in [false,true]{assert_eq!(super::visible(enabled,daw,worn),enabled&&(daw||worn));}}}}
}

#[cfg(test)] mod speaker_tests {
    use super::*;
    #[test] fn omni_model_is_finite_bounded_triangle_mesh() {
        let model:Vec<[f32;6]>=serde_json::from_str(include_str!("../../Art/Audio/OmniSpeaker/runtime.json")).unwrap();
        assert!(!model.is_empty() && model.len()%3==0);
        assert!(model.iter().all(|v|v.iter().all(|x|x.is_finite()) && v[..3].iter().all(|x|x.abs()<0.31)));
    }
}
