//! The authored handset, vector display and current skinned-player selfie.
use crate::{audio::*, host::Host, phone::Pose};
use super::appearance_system::projection;
use serde_json::{Value,json};
use std::{collections::BTreeMap,path::Path,sync::Arc};

struct Glyph { advance:f64, vertices:Vec<[f64;2]> }
pub(crate) struct Asset {
    vertices:Vec<[f32;6]>, glyphs:BTreeMap<char,Glyph>,
    pub min:[f64;2], pub max:[f64;2], pub z:f64,
}
#[derive(Default)]
pub(crate) struct State { pub asset:Option<Arc<Asset>> }
impl Asset {
    fn load(root:&Path)->Result<Self,String> {
        let bytes=std::fs::read(root.join("Procedures/Assets/Phone/phone_mesh.json")).map_err(|e|format!("phone model: {e}"))?;
        if bytes.len()>16*1024*1024{return Err("phone asset exceeds budget".into());}
        let data:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        if data["schema_version"]!=1{return Err("unsupported phone mesh".into());}
        let vertices:Vec<[f32;6]>=serde_json::from_value(data["vertices"].clone()).map_err(|_|"phone vertices required")?;
        if vertices.is_empty()||vertices.len()%3!=0||vertices.len()>15000||vertices.iter().flatten().any(|n|!n.is_finite()){return Err("invalid phone mesh budget/values".into());}
        let min:[f64;2]=serde_json::from_value(data["screen"]["min"].clone()).map_err(|_|"phone screen min required")?;
        let max:[f64;2]=serde_json::from_value(data["screen"]["max"].clone()).map_err(|_|"phone screen max required")?;
        let z=data["screen"]["z"].as_f64().filter(|n|n.is_finite()).ok_or("phone screen plane required")?;
        if (0..2).any(|i|!min[i].is_finite()||!max[i].is_finite()||max[i]<=min[i]||max[i]-min[i]>0.5){return Err("invalid phone screen bounds".into());}
        let mut glyphs=BTreeMap::new();
        for (key,value) in data["glyphs"].as_object().ok_or("phone font required")? {
            let advance=value["advance"].as_f64().filter(|n|n.is_finite()&&(0.0..=2.).contains(n)).ok_or("invalid glyph advance")?;
            let points:Vec<[f64;2]>=serde_json::from_value(value["vertices"].clone()).map_err(|_|"glyph triangles required")?;
            if key.len()!=1||points.len()%3!=0||points.len()>12000||points.iter().flatten().any(|n|!n.is_finite()||n.abs()>3.){return Err("invalid phone glyph".into());}
            glyphs.insert(key.chars().next().unwrap(),Glyph{advance,vertices:points});
        }
        Ok(Self{vertices,glyphs,min,max,z})
    }
    pub fn hit(&self,pose:Pose,scale:f64,eye:V,direction:V,margin:f64,max_distance:f64)->Option<[f64;2]> {
        let denominator=dot(direction,pose.front);
        if denominator>=-1e-7{return None;}
        let origin=add(pose.position,mul(pose.front,self.z*scale));
        let distance=dot(sub(origin,eye),pose.front)/denominator;
        if !(0.01..=max_distance).contains(&distance){return None;}
        let p=sub(add(eye,mul(direction,distance)),pose.position);
        let x=dot(p,pose.right)/scale;let y=dot(p,pose.up)/scale;
        if x<self.min[0]-margin||x>self.max[0]+margin||y<self.min[1]-margin||y>self.max[1]+margin{return None;}
        Some([(x-self.min[0])/(self.max[0]-self.min[0]),(self.max[1]-y)/(self.max[1]-self.min[1])])
    }
}

fn pose(value:&Value)->Result<Pose,String>{
    let p=Pose{position:vector(&value["position"])?,right:vector(&value["right"])?,up:vector(&value["up"])?,front:vector(&value["front"])?};p.validate()?;Ok(p)
}
fn transform(p:V,pose:Pose,scale:f64,eye:V)->V {
    add(sub(pose.position,eye),add(mul(pose.right,p[0]*scale),add(mul(pose.up,p[1]*scale),mul(pose.front,p[2]*scale))))
}
struct Display<'a>{asset:&'a Asset,vertices:Vec<[f32;6]>}
impl<'a> Display<'a> {
    fn point(&self,x:f64,y:f64,layer:f64)->V {
        [self.asset.min[0]+x/800.*(self.asset.max[0]-self.asset.min[0]),
         self.asset.max[1]-y/1200.*(self.asset.max[1]-self.asset.min[1]),self.asset.z+0.00002+layer*0.00001]
    }
    fn triangle(&mut self,p:[[f64;2];3],color:[f32;3],layer:f64){
        if self.vertices.len()>=45000{return;}
        for [x,y] in p {let v=self.point(x,y,layer);self.vertices.push([v[0]as f32,v[1]as f32,v[2]as f32,color[0],color[1],color[2]]);}
    }
    fn rect(&mut self,x:f64,y:f64,w:f64,h:f64,color:[f32;3],layer:f64){
        self.triangle([[x,y],[x+w,y],[x+w,y+h]],color,layer);
        self.triangle([[x,y],[x+w,y+h],[x,y+h]],color,layer);
    }
    fn text(&mut self,text:&str,cx:f64,baseline:f64,size:f64,color:[f32;3]){
        let asset=self.asset;
        let text:String=text.chars().take(60).filter(|c|c.is_ascii()).collect();
        let width=text.chars().filter_map(|c|asset.glyphs.get(&c)).map(|g|g.advance*size).sum::<f64>();
        let mut x=cx-width*0.5;
        for c in text.chars(){if let Some(g)=asset.glyphs.get(&c){
            for tri in g.vertices.chunks_exact(3){self.triangle(std::array::from_fn(|i|[x+tri[i][0]*size,baseline-tri[i][1]*size]),color,3.);}
            x+=g.advance*size;
        }}
    }
    fn button(&mut self,b:[f64;4],color:[f32;3]){
        self.rect(b[0]-4.,b[1]-4.,b[2]-b[0]+8.,b[3]-b[1]+8.,color.map(|c|(c+0.16).min(1.)),1.);
        self.rect(b[0],b[1],b[2]-b[0],b[3]-b[1],color,2.);
    }
    fn screen(&mut self,phone:&Value,view:&Value){
        let white=[0.94,0.98,1.];let muted=[0.60,0.75,0.79];
        let screen=phone["screen"].as_str().unwrap_or("home");
        let bg=match screen{"home"=>[0.,0.35,0.36],"flashlight"=>[0.88,0.90,0.91],"meter"=>[0.055,0.11,0.095],_=>[0.035,0.07,0.11]};
        self.rect(0.,0.,800.,1200.,bg,0.);
        self.rect(0.,0.,800.,126.,[0.02,0.16,0.18],1.);
        let title=match screen{"home"=>"PHONE","flashlight"=>"LIGHT","camera"=>"FRONT CAMERA","meter"=>"DECIBEL METER",_=>"WEATHER"};
        self.text(title,if screen=="home"{400.}else{335.},84.,78.,white);
        if screen!="home"{
            self.button([688.,16.,780.,108.],[0.65,0.16,0.22]);self.text("+",734.,83.,104.,white);
        }
        match screen {
            "home"=>{
                for (b,c,label) in [([72.,170.,322.,420.],[0.07,0.60,0.60],"LIGHT"),([478.,170.,728.,420.],[0.17,0.33,0.53],"CAMERA"),([72.,620.,322.,870.],[0.14,0.42,0.25],"dB METER"),([478.,620.,728.,870.],[0.15,0.40,0.52],"WEATHER")] {
                    self.button(b,c);self.text(label,(b[0]+b[2])*0.5,b[3]+75.,76.,white);
                }
                self.rect(179.,218.,36.,118.,white,3.);self.rect(153.,203.,88.,32.,white,3.);self.rect(165.,335.,63.,17.,white,3.);
                self.rect(535.,244.,138.,94.,white,3.);self.rect(568.,226.,72.,23.,white,3.);self.rect(579.,260.,50.,57.,[0.17,0.33,0.53],4.);
                for bar in 0..5 {self.rect(125.+bar as f64*29.,795.-bar as f64*24.,18.,30.+bar as f64*24.,[0.75,1.,0.80],3.);}
                self.text("SUN",604.,755.,86.,[1.,0.87,0.50]);
                let fraction=view["calendar"]["fraction"].as_f64().unwrap_or(0.);let minutes=(fraction*1440.)as u32;
                self.text(&format!("{:02}:{:02}",minutes/60,minutes%60),400.,1115.,94.,white);
            },
            "flashlight"=>{
                let on=phone["flashlight_on"]==true;
                self.button([154.,414.,646.,786.],if on{[1.,0.95,0.63]}else{[0.44,0.48,0.50]});
                self.text(if on{"FLASHLIGHT ON"}else{"FLASHLIGHT OFF"},400.,620.,86.,if on{[0.13,0.13,0.11]}else{white});
            },
            "meter"=>{
                self.text("WHAT YOU HEAR",400.,280.,78.,muted);
                let available=phone["meter"]["available"]==true;
                let db=phone["meter"]["db"].as_f64().unwrap_or(-96.);
                self.text(&if available{format!("{db:.1}")}else{"--".into()},400.,510.,230.,[0.85,1.,0.89]);
                self.text("dBFS",400.,610.,90.,muted);
                for bar in 0..24{let lit=available&&(db+96.)/96.*24.>bar as f64;let c=if !lit{[0.13,0.21,0.17]}else if bar>=23{[0.92,0.31,0.27]}else if bar>=21{[0.93,0.76,0.31]}else{[0.39,0.87,0.54]};self.rect(88.+bar as f64*26.,710.,20.,80.,c,2.);}
                self.text("-96                         0",400.,863.,55.,muted);
                self.text(&if available{format!("MAX {:.1} dBFS",phone["meter"]["max_db"].as_f64().unwrap_or(-96.))}else{"AUDIO UNAVAILABLE".into()},400.,975.,72.,white);
                self.button([230.,1030.,570.,1130.],[0.14,0.42,0.25]);self.text("RESET MAX",400.,1095.,66.,white);
            },
            "weather"=>{
                let weather=&phone["weather"];
                if let Some(temperature)=weather["temperature_c"].as_f64(){
                    self.text(&format!("{temperature:.1} C"),400.,360.,170.,white);
                    self.text(&format!("WIND {:.1} km/h",weather["wind_kmh"].as_f64().unwrap_or(0.)),400.,485.,82.,muted);
                    self.text(&format!("CLOUD {:.0}%",weather["cloud_fraction"].as_f64().unwrap_or(0.)*100.),400.,590.,82.,muted);
                    self.text("NEXT 24 HOURS",400.,735.,72.,white);
                    for (i,row) in weather["forecast"].as_array().into_iter().flatten().take(4).enumerate(){
                        self.text(&format!("+{} h     {:.1} C",row["lead_hours"],row["temperature_c"].as_f64().unwrap_or(0.)),400.,830.+i as f64*70.,66.,muted);
                    }
                    self.text("LOCAL GAME WEATHER",400.,1170.,49.,muted);
                }else{self.text("WEATHER UNAVAILABLE",400.,620.,66.,muted);}
            },
            "camera"=>if phone["camera_live"]!=true{self.text("LIVE WHEN HELD",400.,620.,78.,muted);},
            _=>{}
        }
    }
}

#[derive(Clone,Copy)]
struct CameraVertex {p:V,uv:[f64;2]}
fn clip_polygon(points:&[CameraVertex],plane:impl Fn(V)->f64)->Vec<CameraVertex>{
    let mut out=Vec::new();
    for i in 0..points.len(){let a=points[i];let b=points[(i+1)%points.len()];let da=plane(a.p);let db=plane(b.p);
        if da>=0.{out.push(a);}if (da>=0.)!=(db>=0.){let t=da/(da-db);out.push(CameraVertex{p:add(a.p,mul(sub(b.p,a.p),t)),uv:std::array::from_fn(|j|a.uv[j]+(b.uv[j]-a.uv[j])*t)});}
    }out
}
fn selfie(asset:&Asset,body:&Value,pose:Pose,scale:f64,view:&Value)->Result<Vec<[f32;24]>,String>{
    let data=crate::base_entity::Entity::from_spec(body["id"].as_u64().ok_or("selfie body ID missing")?,body)?;
    let crate::base_entity::Appearance::Visual{vertices,shader,..}=&data.appearance else{return Err("selfie body must be visual".into());};
    let procedural=data.visual_procedural().ok_or("selfie player metadata missing")?;
    if shader!="player_body"||vertices.len()>150000{return Err("invalid selfie player geometry".into());}
    let forward=vector(&view["forward"])?;let right=vector(&view["right"])?;let up=vector(&view["up"])?;let eye=vector(&view["position"])?;
    let mut head=[0.;3];let mut count=0.;
    for (i,tri)in vertices.chunks_exact(3).enumerate(){if procedural["head_triangles"][i]==true{for v in tri{head=add(head,[v[0]as f64,v[1]as f64,v[2]as f64]);count+=1.;}}}
    if count==0.{return Ok(Vec::new());}head=mul(head,1./count);
    let lens=add(mul(forward,0.72),mul(right,0.10));let target=sub(head,mul(up,0.22));
    let f=unit(sub(target,lens));let r=unit(cross(f,up));let u=cross(r,f);
    let tan=32f64.to_radians().tan();let horizontal=tan*2./3.;
    let mut local=view.clone();local["position"]=json!([0.,0.,0.]);local["two_sided"]=json!(true);local["far"]=json!(length(eye)+2.*crate::terrain::RADIUS);
    let brightness=super::atmosphere::Lighting::from_view(view).brightness().max(0.25)as f32;
    let mut out=Vec::new();
    for (i,tri)in vertices.chunks_exact(3).enumerate(){
        let mut p=Vec::new();for (j,v) in tri.iter().enumerate(){let delta=sub([v[0]as f64,v[1]as f64,v[2]as f64],lens);let uv:[f64;2]=serde_json::from_value(procedural["uvs"][i*3+j].clone()).map_err(|_|"selfie UV missing")?;p.push(CameraVertex{p:[dot(delta,r),dot(delta,u),dot(delta,f)],uv});}
        p=clip_polygon(&p,|p|p[2]-0.03);p=clip_polygon(&p,|p|8.-p[2]);
        p=clip_polygon(&p,|p|p[2]*horizontal-p[0]);p=clip_polygon(&p,|p|p[2]*horizontal+p[0]);
        p=clip_polygon(&p,|p|p[2]*tan*0.79-p[1]);p=clip_polygon(&p,|p|p[2]*tan+p[1]);
        for j in 1..p.len().saturating_sub(1){
            let tri=[p[0],p[j],p[j+1]];let mut mesh=[[0f32;6];3];
            for (k,v)in tri.iter().enumerate(){
                let x=(v.p[0]/(v.p[2]*horizontal)+1.)*0.5;let y=(v.p[1]/(v.p[2]*tan)+1.)*0.5;
                let point=[asset.min[0]+x*(asset.max[0]-asset.min[0]),asset.min[1]+y*(asset.max[1]-asset.min[1]),asset.z+0.00004+0.00002/v.p[2]];
                let point=transform(point,pose,scale,eye);mesh[k]=[point[0]as f32,point[1]as f32,point[2]as f32,1.,1.,1.];
            }
            let mut projected=projection::project(&mesh,&local)[0];
            for k in 0..3{
                // Preserve selfie-camera perspective interpolation while placing
                // its image on the physical phone's projected screen plane.
                for c in 0..4{projected[k*4+c]*=tri[k].p[2]as f32;}
                projected[12+k*4..16+k*4].copy_from_slice(&[tri[k].uv[0]as f32,tri[k].uv[1]as f32,brightness,-2.]);
            }
            out.push(projected);if out.len()>=30000{return Err("selfie triangle budget exceeded".into());}
        }
    }Ok(out)
}

pub(crate) fn frame(host:&mut Host,view:&Value)->Result<Vec<[f32;24]>,String>{
    let phone=&view["phone"];
    if phone["visible"]!=true{return Ok(Vec::new());}
    let asset=host.phone_render.asset.clone().ok_or("phone model not loaded")?;
    let pose=pose(&phone["pose"])?;let scale=phone["scale"].as_f64().filter(|n|(0.1..=4.).contains(n)).ok_or("phone scale required")?;
    let eye=vector(&view["position"])?;if length(sub(pose.position,eye))>80.{return Ok(Vec::new());}
    let lighting=super::atmosphere::Lighting::from_view(view);
    let mut vertices=Vec::with_capacity(asset.vertices.len()+15000);
    for tri in asset.vertices.chunks_exact(3){
        let p:[V;3]=std::array::from_fn(|i|transform([tri[i][0]as f64,tri[i][1]as f64,tri[i][2]as f64],pose,scale,eye));
        let normal=unit(cross(sub(p[1],p[0]),sub(p[2],p[0])));let shade=0.4+0.45*dot(normal,unit(mul(p[0],-1.))).abs()+0.15*dot(normal,lighting.primary).max(0.);
        for i in 0..3{vertices.push([p[i][0]as f32,p[i][1]as f32,p[i][2]as f32,tri[i][3]*shade as f32,tri[i][4]*shade as f32,tri[i][5]*shade as f32]);}
    }
    let mut display=Display{asset:&asset,vertices:Vec::new()};display.screen(phone,view);
    for v in display.vertices{let p=transform([v[0]as f64,v[1]as f64,v[2]as f64],pose,scale,eye);vertices.push([p[0]as f32,p[1]as f32,p[2]as f32,v[3],v[4],v[5]]);}
    let mut local=view.clone();local["position"]=json!([0.,0.,0.]);local["two_sided"]=json!(true);local["far"]=json!(length(eye)+2.*crate::terrain::RADIUS);
    let mut out=projection::project(&vertices,&local);
    if phone["camera_live"]==true {if let Some(id)=view["body"].as_u64(){let body=host.invoke("EntitySystem.Get",json!({"entity":id}))?;out.extend(selfie(&asset,&body,pose,scale,view)?);}}
    Ok(out)
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{match hook{
    "Load"=>{host.phone_render.asset=Some(Arc::new(Asset::load(host.root())?));Ok(Value::Null)},
    "Frame"=>Ok(json!({"triangles":frame(host,input)?})),
    _=>Err("unknown phone render hook".into())
}}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn phone_model_matches_live_screen_and_all_four_apps_have_visible_text(){
        let a=Asset::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();assert!(a.vertices.len()>3000);assert_eq!(a.glyphs.len(),95);
        let pose=Pose::new([0.,0.,-1.],[1.,0.,0.],[0.,1.,0.]).unwrap();
        let uv=a.hit(pose,1.,[0.;3],[0.,0.,-1.],0.,3.25).unwrap();assert!((uv[0]-0.5).abs()<1e-9&&(uv[1]-0.5).abs()<1e-9);
        assert!(a.hit(pose,1.,[0.;3],[1.,0.,0.],0.,3.25).is_none());
        for app in ["home","flashlight","camera","meter","weather"]{let mut d=Display{asset:&a,vertices:Vec::new()};d.screen(&json!({"screen":app}),&json!({}));assert!(d.vertices.len()>100);assert!(d.vertices.len()<=45000);assert!(d.vertices.iter().flatten().all(|v|v.is_finite()));}
    }
    #[test]fn selfie_clips_near_plane_and_preserves_interpolated_uv(){
        let p=[CameraVertex{p:[-1.,0.,-1.],uv:[0.,0.]},CameraVertex{p:[1.,0.,1.],uv:[1.,0.]},CameraVertex{p:[0.,1.,1.],uv:[0.,1.]}];
        let clipped=clip_polygon(&p,|p|p[2]-0.03);assert_eq!(clipped.len(),4);assert!(clipped.iter().all(|v|v.p[2]>=0.03-1e-10));
    }
}
