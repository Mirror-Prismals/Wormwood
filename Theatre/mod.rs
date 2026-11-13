#[path="Laboratory.rs"]
pub(crate) mod laboratory;
#[path="Monastery.rs"]
pub(crate) mod monastery;
#[path="RegionalCities.rs"]
pub(crate) mod regional_cities;
#[path="OfficeChair.rs"]
pub(crate) mod office_chair;
#[path="Nightworld.rs"]
pub(crate) mod nightworld;
#[path="Waterpark.rs"]
pub(crate) mod waterpark;
#[path="Lighting.rs"]
pub(crate) mod lighting;
#[path="Cameras.rs"]
pub(crate) mod cameras;
// Metric theatre architecture and authored cabinet meshes, independent of terrain.
// Local +Y is up; stage is +Z and the loudspeakers face the audience toward -Z.
use crate::audio::{add, dot, geometry::{Geometry, Hit, Snapshot}, length, mul, sub, unit};
use serde_json::{json, Value};
use std::path::Path;

pub(crate) type V = [f64; 3];
const MESH: &str = "Procedures/Assets/Theatre/venue_mesh.json";
// The expanded authored town may use seven eighths of existing scene storage.
// Keep 32,768 slots for terrain/actors; the compositor validates the real total.
// GPU buffers and the complete 262,144-triangle scene limit are unchanged.
pub(crate) const MAX_TRIANGLES: usize = crate::scene_limits::MAX_SCENE_TRIANGLES * 7 / 8;
const BODY_RADIUS: f64 = 0.32;

#[derive(Clone, Debug)]
pub(crate) struct Speaker {
    pub bus: u8, pub position: V, pub size: V,
    pub yaw: f64, pub tilt: f64, pub gain: f64,
    pub horizontal: V, pub vertical: V, pub bands: V,
    pub flown: bool, pub top_pivot: V, pub bottom_pivot: V,
}
impl Speaker {
    pub fn forward(&self) -> V { orient([0., 0., 1.], self.yaw, self.tilt) }
    /// An emitter is on the front aperture, outside its own physical cabinet.
    pub fn acoustic_position(&self) -> V { add(self.position, mul(self.forward(), self.size[2] * 0.51)) }
}
#[derive(Clone)]
struct Solid { center: V, size: V, yaw: f64 }
impl Solid {
    fn local(&self, p: V) -> V { orient(sub(p, self.center), -self.yaw, 0.) }
    fn world(&self, p: V) -> V { add(self.center, orient(p, self.yaw, 0.)) }
    fn interval(&self, origin: V, direction: V, max: f64) -> Option<(f64, f64)> {
        let o = self.local(origin); let d = orient(direction, -self.yaw, 0.);
        let (mut a, mut b) = (0f64, max);
        for i in 0..3 {
            let h = self.size[i] * 0.5;
            if d[i].abs() < 1e-10 { if o[i].abs() > h { return None; } }
            else { let t = [(-h-o[i])/d[i], (h-o[i])/d[i]]; a=a.max(t[0].min(t[1])); b=b.min(t[0].max(t[1])); }
            if a >= b { return None; }
        }
        Some((a,b))
    }
}
#[derive(Clone)]
struct Ramp { x: [f64;2], z: [f64;2], y: [f64;2] }
impl Ramp {
    fn height(&self, x:f64,z:f64) -> Option<f64> {
        (x>=self.x[0] && x<=self.x[1] && z>=self.z[0] && z<=self.z[1])
            .then(|| self.y[0]+(self.y[1]-self.y[0])*(z-self.z[0])/(self.z[1]-self.z[0]))
    }
}
#[derive(Clone, Copy)]
struct Surface { color: [f32; 3], tag: f32 }
impl From<[f32;3]> for Surface {
    fn from(color: [f32;3]) -> Self { Self { color, tag: 0.0 } }
}
const CARPET: Surface = Surface { color: [0.25,0.025,0.05], tag: 100.0 };
const STAIR_CARPET: Surface = Surface { tag: 101.0, ..CARPET };
const STONE: Surface = Surface { color: [0.34,0.32,0.30], tag: 102.0 };
const WOOD: Surface = Surface { color: [0.23,0.15,0.11], tag: 103.0 };
const CLOTH: Surface = Surface { color: [0.19;3], tag: 104.0 };

pub(crate) const MICROPHONE_BASE: V = [0.,5.,24.];
pub(crate) const MICROPHONE_CAPSULE: V = [0.,6.55,24.];

pub(crate) struct Venue {
    pub vertices: Vec<[f32;7]>,
    pub monastery: Option<monastery::Site>,
    pub laboratory: Option<std::sync::Arc<laboratory::Site>>,
    #[cfg(test)] plan_roads:Vec<(Vec<V>,f64)>,
    pub speakers: Vec<Speaker>,
    pub computer_position: V,
    /// These are copied verbatim from the authoritative metre-based rig data.
    pub eq_profiles: Value,
    pub park: Option<waterpark::Park>,
    implicit_floor: bool,
    solids: Vec<Solid>, ramps: Vec<Ramp>,
    acoustic_triangles: Vec<[V;3]>, acoustic: Snapshot, revision: u64,
}
pub(crate) fn orient(p:V,yaw:f64,tilt:f64)->V {
    let (st,ct)=tilt.to_radians().sin_cos(); let (sy,cy)=yaw.to_radians().sin_cos();
    let q=[p[0],ct*p[1]-st*p[2],st*p[1]+ct*p[2]];
    [cy*q[0]+sy*q[2],q[1],-sy*q[0]+cy*q[2]]
}
fn number(v:&Value,key:&str)->Result<f64,String> {v[key].as_f64().filter(|x|x.is_finite()).ok_or_else(||format!("theatre: invalid {key}"))}
fn vector(v:&Value)->Result<V,String> {crate::audio::vector(v)}
fn read(root:&Path,path:&str)->Result<Value,String> {
    let bytes=std::fs::read(root.join(path)).map_err(|e|format!("{path}: {e}"))?;
    if bytes.len()>12*1024*1024 {return Err(format!("{path}: asset budget exceeded"));}
    serde_json::from_slice(&bytes).map_err(|e|format!("{path}: {e}"))
}
pub(crate) fn load(root:&Path)->Result<Venue,String> {
    let rig=read(root,"Procedures/theatre_line_array.json")?;
    if rig["version"]!=2 || rig["bus_order"]!=json!(["L","S","FF","R"]) {return Err("unsupported theatre rig/bus order".into());}
    let asset=read(root,MESH)?;
    if asset["schema_version"]!=1 || asset["coordinate_system"]!="metres_x_right_y_up_z_front" {return Err("unsupported authored theatre mesh".into());}
    let mut revision=0xcbf29ce484222325u64;
    for b in serde_json::to_vec(&json!([rig,asset])).map_err(|e|e.to_string())? {revision=(revision^u64::from(b)).wrapping_mul(0x100000001b3);}
    let empty=Snapshot::parse(&json!({"revision":revision,"origin":[0,0,0],"triangles":[],"solids":[]}))?;
    let mut venue=Venue {monastery:None,laboratory:None,#[cfg(test)] plan_roads:Vec::new(),vertices:Vec::new(),speakers:Vec::new(),computer_position:[0.,8.,-47.],eq_profiles:rig["eq_profiles"].clone(),park:None,implicit_floor:true,solids:Vec::new(),ramps:Vec::new(),acoustic_triangles:Vec::new(),acoustic:empty,revision};
    venue.architecture();
    // Retain the established coarse contact/acoustic envelopes, while the
    // complete visible room comes from the editable Blender master.
    venue.vertices.clear();
    venue.place_mesh(&asset,"architecture",[0.;3],0.,0.)?;

    let microphone=read(root,"Procedures/Assets/VirtualMicrophone/microphone_mesh.json")?;
    let vertices:Vec<[f32;6]>=serde_json::from_value(microphone["vertices"].clone()).map_err(|e|format!("microphone mesh: {e}"))?;
    if vertices.len()%3!=0 || vertices.len()>15000 || vertices.iter().flatten().any(|x|!x.is_finite()) {return Err("invalid microphone mesh".into());}
    for v in vertices {venue.vertices.push([v[0]+MICROPHONE_BASE[0] as f32,v[1]+MICROPHONE_BASE[1] as f32,v[2]+MICROPHONE_BASE[2] as f32,v[3],v[4],v[5],0.]);}

    let cabinet=|kind:&str,bus:u8,gain:f64|->Result<Speaker,String>{
        let c=&rig["cabinet_types"][kind];
        let s=Speaker {bus,position:[0.;3],size:vector(&c["size"])?,yaw:180.,tilt:0.,gain,horizontal:vector(&c["horizontal"])?,vertical:vector(&c["vertical"])?,bands:vector(&c["bands"])?,flown:false,top_pivot:[0.;3],bottom_pivot:[0.;3]};
        if bus>3 || gain<0. || s.size.iter().any(|v|*v<=0.||*v>4.) || s.horizontal.iter().chain(s.vertical.iter()).any(|v|*v<=0.||*v>360.) || s.bands.iter().any(|v|*v<0.) {return Err("invalid theatre cabinet".into());} Ok(s)
    };
    for array in rig["arrays"].as_array().ok_or("theatre arrays required")? {
        let mut pivot=vector(&array["top_rear_pivot"])?; let mut tilt=number(array,"tilt")?;
        let yaw=number(array,"yaw")?;
        let bus=array["bus"].as_u64().filter(|v|*v<4).ok_or("invalid array bus")? as u8;
        for (_i,angle) in array["splay_degrees"].as_array().ok_or("array splay required")?.iter().enumerate() {
            let splay=angle.as_f64().filter(|v|v.is_finite()&&(0.0..=14.).contains(v)).ok_or("invalid joint splay")?;
            tilt+=splay;
            let mut s=cabinet(array["cabinet"].as_str().ok_or("array cabinet required")?,bus,number(array,"gain")?)?;
            s.flown=true;s.yaw=yaw;s.tilt=tilt;s.top_pivot=pivot;
            s.position=sub(pivot,orient([0.,s.size[1]*0.5,-s.size[2]*0.5],yaw,tilt));
            s.bottom_pivot=add(s.position,orient([0.,-s.size[1]*0.5,-s.size[2]*0.5],yaw,tilt));
            pivot=s.bottom_pivot;
            venue.place_mesh(&asset,"array_element",s.position,yaw,tilt)?;
            venue.speakers.push(s);
        }
    }
    for stack in rig["ground_stacks"].as_array().ok_or("theatre ground stacks required")? {
        let p=vector(&stack["position"])?;let yaw=number(stack,"yaw")?;
        let mut s=cabinet(stack["sub_cabinet"].as_str().ok_or("sub cabinet required")?,stack["sub_bus"].as_u64().filter(|v|*v<4).ok_or("sub bus required")? as u8,number(stack,"sub_gain")?)?;
        s.position=[p[0],s.size[1]*0.5,p[2]];s.yaw=yaw;
        venue.place_mesh(&asset,"sub",s.position,yaw,0.)?;
        venue.solids.push(Solid{center:s.position,size:s.size,yaw});
        let mut f=cabinet(stack["fill_cabinet"].as_str().ok_or("fill cabinet required")?,stack["fill_bus"].as_u64().filter(|v|*v<4).ok_or("fill bus required")? as u8,number(stack,"fill_gain")?)?;
        f.position=add(s.position,[0.,(s.size[1]+f.size[1])*0.5,0.]); f.yaw=yaw;f.tilt=number(stack,"fill_tilt")?;
        venue.place_mesh(&asset,"array_element",f.position,yaw,f.tilt)?;
        venue.speakers.push(s);venue.speakers.push(f);
    }
    if venue.speakers.len()!=24 {return Err("theatre reference requires 24 cabinets".into());}
    venue.place_mesh(&asset,"computer",venue.computer_position,180.,0.)?;
    lighting::install(root,&mut venue.vertices)?;
    if length(sub(vector(&microphone["capsule_local"])?,sub(MICROPHONE_CAPSULE,MICROPHONE_BASE)))>1e-6 || microphone["forward"]!=json!([0,0,-1]) {return Err("microphone asset/listener pose mismatch".into());}
    venue.acoustic=Snapshot::parse(&venue.geometry_snapshot([0.;3]))?;
    if venue.vertices.len()/3>MAX_TRIANGLES {return Err(format!("theatre triangle budget exceeded: {}",venue.vertices.len()/3));}
    Ok(venue)
}
impl Venue {
    /// Join the foundation to the actual continuous terrain at the entrance.
    pub fn add_approach(&mut self, outside_height:f64)->Result<(),String> {
        if !outside_height.is_finite() || !(-100.0..=0.).contains(&outside_height) {return Err("invalid foundation approach height".into());}
        self.ramp(Ramp{x:[-5.,5.],z:[-94.,-82.],y:[outside_height,0.]},STONE);
        self.revision=self.revision.wrapping_add(1);
        self.acoustic=Snapshot::parse(&self.geometry_snapshot([0.;3]))?;
        Ok(())
    }
    fn triangle(&mut self,p:[V;3],surface:impl Into<Surface>,acoustic:bool) {
        let Surface {color,tag} = surface.into();
        for v in p {self.vertices.push([v[0] as f32,v[1] as f32,v[2] as f32,color[0],color[1],color[2],tag]);}
        if acoustic {self.acoustic_triangles.push(p);}
    }
    fn box_mesh(&mut self,center:V,size:V,surface:impl Into<Surface>,yaw:f64,solid:bool) {
        let color=surface.into();
        let b=Solid{center,size,yaw};
        let p:[V;8]=std::array::from_fn(|i|b.world(std::array::from_fn(|a|if i&(1<<a)==0 {-size[a]*0.5}else{size[a]*0.5})));
        for f in [[0,4,6,2],[1,3,7,5],[0,1,5,4],[2,6,7,3],[0,2,3,1],[4,5,7,6]] {for t in [[0,1,2],[0,2,3]] {self.triangle(t.map(|i|p[f[i]]),color,solid);}}
        if solid {self.solids.push(b);}
    }
    fn ramp(&mut self,r:Ramp,surface:impl Into<Surface>) {
        let color=surface.into();
        let p=[[r.x[0],r.y[0],r.z[0]],[r.x[1],r.y[0],r.z[0]],[r.x[1],r.y[1],r.z[1]],[r.x[0],r.y[1],r.z[1]]];
        self.triangle([p[0],p[3],p[2]],color,true);self.triangle([p[0],p[2],p[1]],color,true);
        // Keep the skirt below both ends, including the outdoor approach whose
        // terrain end is below the theatre's local floor height.
        let bottom=r.y[0].min(r.y[1]).min(0.)-0.1;
        for i in 0..4 {let j=(i+1)%4;let a=[p[i][0],bottom,p[i][2]];let b=[p[j][0],bottom,p[j][2]];self.triangle([p[i],b,a],color,true);self.triangle([p[i],p[j],b],color,true);}
        self.ramps.push(r);
    }
    fn architecture(&mut self) {
        let stone=STONE; let ivory=STONE; let crimson=CARPET; let gold=[0.69,0.47,0.18]; let wood=WOOD;
        // Smooth round envelope; this is conventional mesh architecture, not terrain cells.
        for i in 0..96 {let a=std::f64::consts::TAU*i as f64/96.;let b=std::f64::consts::TAU*(i+1) as f64/96.;
            let p=[76.*a.cos(),0.,76.*a.sin()];let q=[76.*b.cos(),0.,76.*b.sin()];
            self.triangle([[0.,0.,0.],q,p],crimson,true);
            self.triangle([[0.,18.,0.],[p[0],18.,p[2]],[q[0],18.,q[2]]],stone,true);
            let middle=(a+b)*0.5;let entrance=middle.sin()<0. && (76.*middle.cos()).abs()<5.;
            let h=if entrance {12.}else{18.};let y=if entrance {12.}else{9.};
            self.box_mesh([76.*middle.cos(),y,76.*middle.sin()],[length(sub(q,p))+0.025,h,0.55],stone,90.-middle.to_degrees(),true);
        }
        self.box_mesh([0.,-0.3,0.],[106.,0.6,106.],crimson,0.,true);
        // Ceiling across the main room also supplies a volumetric acoustic roof.
        self.box_mesh([0.,18.25,-3.],[69.,0.5,111.],stone,0.,true);
        for x in [-34.,34.] {self.box_mesh([x,9.,-3.],[0.6,18.,110.],CLOTH,0.,true);}
        for x in [-19.,19.] {self.box_mesh([x,9.,-58.],[30.,18.,0.6],CLOTH,0.,true);}
        self.box_mesh([0.,1.5,35.],[48.,3.,30.],wood,0.,true);
        for x in [-25.5,25.5] {self.box_mesh([x,10.5,23.],[17.,15.,0.6],ivory,0.,true);}
        self.box_mesh([0.,16.,23.],[34.,4.,0.6],ivory,0.,true);
        self.box_mesh([0.,10.5,50.],[68.,15.,0.5],wood,0.,true);
        // Acoustic slats are distinct mesh faces, with a warm finish on the backdrop.
        for i in 0..86 {let x=-33.5+i as f64*0.78;self.box_mesh([x,10.5,49.68],[0.12,14.5,0.16],gold,0.,false);}
        // Preserve the five original audience terraces; the center is a continuous aisle.
        for step in 0..6 {let z0=(-54f64).max(12.-12.*(step+1) as f64);let z1=12.-12.*step as f64;
            if step>0 {for x in [-16.7,16.7] {self.box_mesh([x,step as f64*0.5,(z0+z1)*0.5],[28.6,step as f64,z1-z0],STAIR_CARPET,0.,true);}}
        }
        self.ramp(Ramp{x:[-2.4,2.4],z:[-54.,12.],y:[5.5,0.]},crimson);
        // FOH starts at six metres; short central access meets the auditorium ramp.
        self.box_mesh([0.,3.,-49.5],[14.,6.,11.],ivory,0.,true);
        self.ramp(Ramp{x:[-2.4,2.4],z:[-44.,-40.],y:[6.,52./12.]},crimson);
        // Open rear approach: the lobby ramp permits entering and leaving the theatre on foot.
        self.ramp(Ramp{x:[-3.5,3.5],z:[-72.,-55.],y:[0.,6.]},crimson);
        self.box_mesh([0.,3.,-54.5],[7.,6.,1.],ivory,0.,true);
        for x in [-7.,7.] {self.box_mesh([x,6.5,-49.5],[0.15,1.,11.],gold,0.,true);}
        // Desk top at 7.45m leaves a one metre authored monitor resting on it.
        self.box_mesh([0.,7.35,-47.],[8.5,0.2,1.3],wood,0.,true);
        for x in [-3.7,3.7] {self.box_mesh([x,6.65,-47.],[0.12,1.3,1.1],gold,0.,true);}
        for x in [-26.5,26.5] {
            self.ramp(Ramp{x:[x-1.5,x+1.5],z:[11.,20.],y:[0.,3.]},ivory);
            self.box_mesh([x,1.5,21.5],[5.,3.,3.],ivory,0.,true);
        }
        for x in [-32.8,32.8] {for i in 0..9 {self.box_mesh([x,15.8,-52.+i as f64*8.],[0.2,0.24,1.], [1.,0.77,0.34],0.,false);}}
        // Exterior entry path meets the continuous base floor without a step.
        self.box_mesh([0.,-0.2,-78.],[10.,0.4,8.],stone,0.,true);
    }
    fn place_mesh(&mut self,asset:&Value,name:&str,position:V,yaw:f64,tilt:f64)->Result<(),String> {
        let mesh=&asset["meshes"][name];
        let rows=mesh["vertices"].as_array().ok_or_else(||format!("missing Blender mesh {name}"))?;
        if rows.is_empty() || rows.len()%3!=0 || rows.len()>match name {"architecture"=>90_000,"computer"=>18_000,_=>12_000} {return Err(format!("invalid Blender mesh budget {name}"));}
        let expected=match name {"array_element"=>[1.05,0.34,0.55],"sub"=>[1.8,1.3,1.6],"architecture"=>[154.,38.,170.],_=>[1.05,1.1,0.7]};
        for row in rows {
            let v:Vec<f64>=serde_json::from_value(row.clone()).map_err(|e|format!("{name}: {e}"))?;
            let textured=v.len()==7 && (106.0..=120.0).contains(&v[6]) && v[6].fract()==0.;
            if !(v.len()==6 || textured) || !v.iter().all(|x|x.is_finite()) || (0..3).any(|i|v[i].abs()>expected[i]*0.65) ||
                (!textured && v[3..].iter().any(|x|*x<0.||*x>1.)) || (textured && (name!="architecture" || v[3..6].iter().any(|x|x.abs()>200.))) {
                return Err(format!("invalid authored {name} vertex"));
            }
            let p=add(position,orient([v[0],v[1],v[2]],yaw,tilt));
            self.vertices.push([p[0] as f32,p[1] as f32,p[2] as f32,v[3] as f32,v[4] as f32,v[5] as f32,if textured {v[6] as f32}else{0.}]);
        }
        if self.vertices.len()/3>MAX_TRIANGLES {return Err("theatre authored mesh exceeds scene budget".into());}Ok(())
    }
    /// Meshes and axis-aligned volumes are local to the supplied world origin.
    /// Oriented wall solids are also exposed, for clients that retain exact volume queries.
    pub fn geometry_snapshot(&self,origin:V)->Value {
        let solids:Vec<_>=self.solids.iter().filter(|b|b.yaw.abs()<1e-8).map(|b|json!({"min":sub(b.center,mul(b.size,0.5)),"max":add(b.center,mul(b.size,0.5))})).collect();
        let oriented:Vec<_>=self.solids.iter().filter(|b|b.yaw.abs()>=1e-8).map(|b|json!({"center":b.center,"size":b.size,"yaw_degrees":b.yaw})).collect();
        json!({"revision":self.revision,"origin":origin,"triangles":self.acoustic_triangles,"solids":solids,"oriented_solids":oriented,"surface_scope":"authored_theatre_architecture","solid_scope":["theatre_floor_stage_walls_roof_foh","oriented_shell_walls"]})
    }
    pub fn trace_local(&self,origin:V,direction:V,max_distance:f64)->Option<Hit> {self.acoustic.trace(origin,direction,max_distance)}
    /// Swept bounded substeps prevent walking through narrow walls. Surface support
    /// respects step height; deliberate ramps connect all required destinations.
    pub fn resolve_body(&self,previous:V,desired:V,eye_height:f64)->(V,Vec<V>) {
        if !previous.iter().chain(desired.iter()).all(|v|v.is_finite()) || !eye_height.is_finite() || eye_height<=0. {return (previous,vec![]);}
        let delta=sub(desired,previous);let steps=(length(delta)/0.12).ceil().clamp(1.,512.) as usize;
        let mut p=previous;let mut normals=Vec::new();
        for _ in 0..steps {
            let before=p;p=add(p,mul(delta,1./steps as f64));
            // The wedge is solid below its walking surface. Prevent entering
            // through a high side edge instead of teleporting onto the ramp.
            for r in &self.ramps {if let Some(h)=r.height(p[0],p[2]) {
                if p[1]>0. && p[1]-eye_height<h-0.36 && before[1]-eye_height<h-0.36 {
                    let dx=p[0]-before[0];let dz=p[2]-before[2];
                    if dx.abs()+dz.abs()>1e-8 {p[0]=before[0];p[2]=before[2];push_normal(&mut normals,unit([-dx,0.,-dz]));}
                }
            }}
            // Floor support covers the round envelope and entry path.
            let mut support=if self.implicit_floor && (p[0]*p[0]+p[2]*p[2]<=76.*76. || (p[0].abs()<5. && (-82.0..=-74.).contains(&p[2]))) {Some(0.)}else{None};
            for r in &self.ramps {if let Some(h)=r.height(p[0],p[2]) {if before[1]-eye_height>=h-0.36 {support=Some(support.map_or(h,|old:f64|old.max(h)));}}}
            for b in &self.solids {let q=b.local(p);let top=b.center[1]+b.size[1]*0.5;
                // Use the same body footprint as the side collision below:
                // a ramp reaches a landing with the body's leading edge before
                // its center crosses the box. The step-height limit still
                // prevents climbing a high vertical face.
                if q[0].abs()<b.size[0]*0.5+BODY_RADIUS && q[2].abs()<b.size[2]*0.5+BODY_RADIUS && before[1]-eye_height>=top-0.36 {support=Some(support.map_or(top,|old|old.max(top)));}}
            if let Some(h)=support {if p[1]-eye_height<=h+1e-5 && before[1]-eye_height>=h-0.36 {p[1]=h+eye_height;push_normal(&mut normals,[0.,1.,0.]);}}
            for b in &self.solids {
                let mut q=b.local(p);let old=b.local(before);let half=mul(b.size,0.5);
                if p[1]-eye_height>=b.center[1]+half[1]-0.015 || p[1]+0.12<=b.center[1]-half[1] {continue;}
                let hx=half[0]+BODY_RADIUS;let hz=half[2]+BODY_RADIUS;
                if q[0].abs()>=hx || q[2].abs()>=hz {continue;}
                // Contact with the underside of roofs/desks when jumping.
                if before[1]+0.12<=b.center[1]-half[1]+0.02 {p[1]=b.center[1]-half[1]-0.12;push_normal(&mut normals,[0.,-1.,0.]);continue;}
                let axis=if old[0].abs()>=hx {0}else if old[2].abs()>=hz {2}else if hx-q[0].abs()<hz-q[2].abs(){0}else{2};
                let sign=if q[axis]>=0. {1.}else{-1.};q[axis]=sign*if axis==0 {hx}else{hz};
                let mut normal=[0.;3];normal[axis]=sign;push_normal(&mut normals,orient(normal,b.yaw,0.));
                let resolved=b.world(q);p[0]=resolved[0];p[2]=resolved[2];
            }
        }
        if let Some(park)=&self.park {
            let (q,contacts)=park.resolve(previous,p,eye_height);p=q;
            for n in contacts {push_normal(&mut normals,n);}
        }
        (p,normals)
    }
}
fn push_normal(normals:&mut Vec<V>,n:V) {if normals.len()<16 && !normals.iter().any(|v|dot(*v,n)>0.999) {normals.push(n);}}
impl Geometry for Venue {
    fn revision(&self)->u64 {self.revision}
    fn trace(&self,origin:V,direction:V,max_distance:f64)->Option<Hit> {self.trace_local(origin,direction,max_distance)}
    fn wall_distance(&self,start:V,end:V,max_distance:f64)->f64 {
        let d=unit(sub(end,start));let limit=length(sub(end,start)).min(max_distance);
        let mut spans:Vec<_>=self.solids.iter().filter_map(|b|b.interval(start,d,limit)).collect();
        spans.sort_by(|a,b|a.0.total_cmp(&b.0));let(mut sum,mut last)=(0f64,0f64);
        for(a,b)in spans {if b>last {sum+=b-a.max(last);last=b;}}sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn room_uses_authored_geometry_without_image_finish_tags() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let asset=read(root,MESH).unwrap();let v=load(root).unwrap();
        let room=asset["meshes"]["architecture"]["vertices"].as_array().unwrap();
        assert!(room.len()>10_000,"room must contain physical finish detail");
        assert_eq!(v.vertices.len(),super::read(Path::new(env!("CARGO_MANIFEST_DIR")),"Procedures/Assets/VirtualMicrophone/microphone_mesh.json").unwrap()["vertices"].as_array().unwrap().len()+(7680-128)*3+super::read(Path::new(env!("CARGO_MANIFEST_DIR")),"Procedures/Assets/Theatre/stage_lighting_mesh.json").unwrap()["vertices"].as_array().unwrap().len()+room.len()+20*asset["meshes"]["array_element"]["vertices"].as_array().unwrap().len()+4*asset["meshes"]["sub"]["vertices"].as_array().unwrap().len()+asset["meshes"]["computer"]["vertices"].as_array().unwrap().len());
        assert!(v.vertices.iter().all(|p|p[6]==0. || (106.0..=120.0).contains(&p[6]) || (201.0..=206.0).contains(&p[6])),"room must not sample the retired image finishes");
        for tag in [112.,113.,114.,115.,116.,117.,118.,119.,120.] {
            assert!(v.vertices.iter().any(|p|p[6]==tag),"missing authored theatre material {tag}");
        }
        assert!(!v.solids.is_empty() && !v.acoustic_triangles.is_empty());
    }
    #[test] fn foundation_approach_accepts_a_walker_from_lower_terrain() {
        let mut venue=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();venue.add_approach(-0.5).unwrap();
        let mut p=[0.,1.5,-94.];
        for _ in 0..130 {p=venue.resolve_body(p,add(p,[0.,-0.025,0.1]),2.).0;}
        assert!(p[2]>-82. && (p[1]-2.).abs()<0.02,"foundation entry: {p:?}");
    }
    #[test] fn outdoor_approach_sides_stay_below_the_walking_surface() {
        let mut venue=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let start=venue.vertices.len();
        venue.add_approach(-5.).unwrap();
        for p in &venue.vertices[start..] {
            let surface=-5.+5.*(p[2] as f64+94.)/12.;
            assert!(p[1] as f64<=surface+1e-5,"raised approach wall: {p:?}");
        }
    }
    #[test] fn connected_rear_rails_and_reference_layout() {
        let v=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(v.speakers.len(),24);assert!(v.vertices.len()/3<=MAX_TRIANGLES);
        for start in [0,8] {for i in start..start+7 {assert!(length(sub(v.speakers[i].bottom_pivot,v.speakers[i+1].top_pivot))<1e-9);}}
        assert_eq!(v.speakers[7].tilt,35.);assert_eq!(v.speakers[15].bus,3);
        assert!(v.speakers[0].forward()[2]<0.);
    }
    #[test] fn walking_routes_connect_foh_audience_and_stage() {
        let v=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();let eye=2.;
        let mut p=[5.,8.,-52.];
        for _ in 0..70 {let goal=add(p,[0.,-0.025,0.1]);p=v.resolve_body(p,goal,eye).0;}
        for _ in 0..50 {let goal=add(p,[-0.1,-0.025,0.]);p=v.resolve_body(p,goal,eye).0;}
        for _ in 0..620 {let goal=add(p,[0.,-0.025,0.1]);p=v.resolve_body(p,goal,eye).0;}
        assert!(p[2]>16. && p[1]<2.1,"FOH to front aisle: {p:?}");
        p=[26.5,2.,10.];for _ in 0..120 {let goal=add(p,[0.,-0.025,0.1]);p=v.resolve_body(p,goal,eye).0;}
        assert!(p[2]>21. && (p[1]-5.).abs()<0.05,"wing ramp: {p:?}");
        for _ in 0..40 {let goal=add(p,[-0.1,-0.025,0.]);p=v.resolve_body(p,goal,eye).0;}
        assert!(p[0]<23. && (p[1]-5.).abs()<0.05,"wing joins stage: {p:?}");
        p=[0.,2.,-73.];for _ in 0..190 {let goal=add(p,[0.,-0.025,0.1]);p=v.resolve_body(p,goal,eye).0;}
        assert!(p[2]>-55. && (p[1]-8.).abs()<0.05,"lobby reaches FOH: {p:?}");
        let hit=v.trace_local([0.,8.,0.],[1.,0.,0.],100.).unwrap();assert!((hit.distance-33.7).abs()<0.01);
    }
    #[test] fn landing_support_matches_body_contact_without_climbing_high_faces() {
        let v=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();let eye=2.;
        let p=v.resolve_body([26.5,4.9,19.6],[26.5,4.875,19.7],eye).0;
        assert!(p[2]>19.68 && (p[1]-5.).abs()<1e-9,"step onto wing landing: {p:?}");
        let mut p=[0.,2.,19.4];
        for _ in 0..20 {p=v.resolve_body(p,add(p,[0.,-0.025,0.1]),eye).0;}
        assert!(p[2]<=19.68+1e-9 && (p[1]-eye).abs()<1e-9,"high stage face remains blocked: {p:?}");
    }
}

#[cfg(test)] mod carpet_tests {
 #[test] fn authored_carpet_tags_follow_floor_geometry_and_keep_metric_positions(){
  use crate::audio::{sub,cross,unit,dot};
  let venue=super::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  let mut counts=[0;4];
  for tri in venue.vertices.chunks_exact(3){
   let tag=tri[0][6];if !(117.0..=120.0).contains(&tag){continue;}
   assert!(tri.iter().all(|v|v[6]==tag && v[..3]==v[3..6]));
   let points:[[f64;3];3]=std::array::from_fn(|i|std::array::from_fn(|j|tri[i][j] as f64));
   let n=unit(cross(sub(points[1],points[0]),sub(points[2],points[0])));
   let slope=match tag as i32{118=>5.5/66.,119=>5./12.,120=>-6./17.,_=>0.};
   let expected=unit([0.,1.,slope]);assert!(dot(n,expected)>0.99999,"wrong ramp frame {tag}: {n:?}");
   counts[(tag-117.) as usize]+=1;
  }
  assert_eq!(counts,[20,2,2,2]);
 }
}

impl Venue {
    pub(crate) fn mod_seed(&self)->Value {
        json!({"vertices":self.vertices,"acoustic_triangles":self.acoustic_triangles,
            "solids":self.solids.iter().map(|s|json!({"center":s.center,"size":s.size,"yaw":s.yaw})).collect::<Vec<_>>(),
            "ramps":self.ramps.iter().map(|r|json!({"x":r.x,"y":r.y,"z":r.z})).collect::<Vec<_>>()})
    }
    pub(crate) fn apply_mod_scene(&mut self,scene:&Value)->Result<(),String>{
        let vertices:Vec<[f32;7]>=serde_json::from_value(scene["vertices"].clone()).map_err(|e|e.to_string())?;
        if vertices.len()%3!=0||vertices.len()/3>MAX_TRIANGLES||vertices.iter().flatten().any(|v|!v.is_finite()){return Err("mod scene exceeds geometry budget".into());}
        let triangles:Vec<[V;3]>=serde_json::from_value(scene["acoustic_triangles"].clone()).map_err(|e|e.to_string())?;
        if triangles.len()>MAX_TRIANGLES||triangles.iter().flatten().flatten().any(|v|!v.is_finite()){return Err("invalid mod acoustic geometry".into());}
        let solids=scene["solids"].as_array().filter(|v|v.len()<=65536).ok_or("invalid mod solid list")?.iter().map(|s|{
            let center=vector(&s["center"])?;let size=vector(&s["size"])?;let yaw=number(s,"yaw")?;
            if size.iter().any(|x|*x<=0.){return Err("invalid mod solid dimensions".into());}Ok(Solid{center,size,yaw})
        }).collect::<Result<Vec<_>,String>>()?;
        let pair=|v:&Value|->Result<[f64;2],String>{let a:[f64;2]=serde_json::from_value(v.clone()).map_err(|e|e.to_string())?;if a.iter().any(|v|!v.is_finite()){return Err("nonfinite mod ramp".into());}Ok(a)};
        let ramps=scene["ramps"].as_array().filter(|v|v.len()<=65536).ok_or("invalid mod ramp list")?.iter().map(|r|Ok(Ramp{x:pair(&r["x"])?,y:pair(&r["y"])?,z:pair(&r["z"])?})).collect::<Result<Vec<_>,String>>()?;
        let park=if scene["park"].is_null(){None}else{Some(waterpark::Park::from_mod_packet(&scene["park"])?)};
        let monastery=if scene["monastery"].is_null(){None}else{Some(monastery::Site{center:vector(&scene["monastery"]["center"])?,road:vector(&scene["monastery"]["road"]) ?})};
        let laboratory=if scene["laboratory"].is_null(){None}else{let v=&scene["laboratory"];Some(std::sync::Arc::new(laboratory::Site{lab:vector(&v["lab"])?,zoo:vector(&v["zoo"])?,lab_portal:vector(&v["lab_portal"])?,zoo_portal:vector(&v["zoo_portal"])?,lab_vertices:serde_json::from_value(v["lab_vertices"].clone()).map_err(|e|e.to_string())?,zoo_vertices:serde_json::from_value(v["zoo_vertices"].clone()).map_err(|e|e.to_string())?}))};
        // Validate before replacing the live scene; the plugin never receives pointers into it.
        let revision=self.revision.wrapping_add(1);
        let mut candidate=Self{vertices,speakers:self.speakers.clone(),computer_position:self.computer_position,eq_profiles:self.eq_profiles.clone(),park,monastery,laboratory,implicit_floor:self.implicit_floor,solids,ramps,acoustic_triangles:triangles,acoustic:Snapshot::default(),revision,#[cfg(test)]plan_roads:Vec::new()};
        candidate.acoustic=Snapshot::parse(&candidate.geometry_snapshot([0.;3]))?;*self=candidate;Ok(())
    }
}

pub(crate) fn foundation_origin()->V {
    let mut highest=crate::terrain::height([0.,1.,0.]);
    for x in -8..=8 {for z in -8..=8 {if x*x+z*z<=64 {highest=highest.max(crate::terrain::height(unit([x as f64*10.,crate::terrain::RADIUS,z as f64*10.])));}}}
    [0.,crate::terrain::RADIUS+highest+0.5,0.]
}
impl Venue {
    fn empty_mod_scene()->Self{Self{vertices:vec![],speakers:vec![],computer_position:[0.;3],eq_profiles:Value::Null,park:None,monastery:None,laboratory:None,implicit_floor:false,solids:vec![],ramps:vec![],acoustic_triangles:vec![],acoustic:Snapshot::default(),revision:1,#[cfg(test)]plan_roads:vec![]}}
    fn mod_stage(&mut self,root:&Path,stage:&str,origin:V,value:f64,offset:V)->Result<f64,String>{
        let mut mods=crate::modding::Manager::from_root(root)?;
        let result=mods.call("prismals","Prismals.BuildStage",&json!({"stage":stage,"origin":origin,"value":value,"offset":offset,"seed":self.mod_seed()}))?;
        self.apply_mod_scene(&result["scene"])?;Ok(result["result"].as_f64().unwrap_or(0.))
    }
    pub fn add_waterpark(&mut self,root:&Path,offset:V,outside:f64,origin:V)->Result<(),String>{self.mod_stage(root,"waterpark",origin,outside,offset).map(|_|())}
    pub fn add_town(&mut self,root:&Path,origin:V)->Result<(),String>{self.mod_stage(root,"town",origin,0.,[0.;3]).map(|_|())}
    pub fn add_civic_buildings(&mut self,root:&Path,origin:V,value:f64)->Result<f64,String>{self.mod_stage(root,"civic",origin,value,[0.;3])}
    pub fn add_marsh(&mut self,root:&Path,origin:V,value:f64)->Result<f64,String>{self.mod_stage(root,"marsh",origin,value,[0.;3])}
    pub fn add_plan(&mut self,root:&Path,origin:V,value:f64)->Result<(),String>{self.mod_stage(root,"plan",origin,value,[0.;3]).map(|_|())}
    pub fn add_laboratory(&mut self,root:&Path,origin:V)->Result<(),String>{self.mod_stage(root,"laboratory",origin,0.,[0.;3]).map(|_|())}
    pub fn add_monastery(&mut self,origin:V)->Result<(),String>{let root=std::env::var_os("WORMWOOD_ASSET_ROOT").map(std::path::PathBuf::from).unwrap_or_else(||Path::new(env!("CARGO_MANIFEST_DIR")).into());self.mod_stage(&root,"monastery",origin,0.,[0.;3]).map(|_|())}
}
