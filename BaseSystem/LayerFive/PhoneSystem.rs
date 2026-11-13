//! Application phone ownership, input arbitration and live sensor publication.
use crate::{audio::*,host::Host,phone::{self,Command,Location,Pose,AudioSample}};
use serde_json::{Value,json};
#[cfg(target_os="ios")]
unsafe extern "C" {fn ww_ios_phone_state(held:bool,focused:bool);}

#[derive(Default)]
pub(crate) struct State {
    handset:Option<phone::State>, previous:[bool;4], interact_owned:bool,
    weather:Value, weather_elapsed:f64,
    diagnostic_snapshot:Option<Value>,
}
impl State {
    fn status(&self)->Value {self.handset.as_ref().map_or(Value::Null,phone::State::status)}
}
fn pose_json(p:Pose)->Value{json!({"position":p.position,"right":p.right,"up":p.up,"front":p.front})}
fn world_point(p:Pose,v:V,scale:f64)->V{add(p.position,add(mul(p.right,v[0]*scale),add(mul(p.up,v[1]*scale),mul(p.front,v[2]*scale))))}
fn current_pose(phone:&phone::State,view:&Value)->Result<Option<(Pose,f64)>,String>{
    Ok(match phone.location(){
        Location::Placed(p)=>Some((p,1.)),Location::Stowed=>None,
        Location::Held=>{let p=phone::held_pose(vector(&view["position"])?,vector(&view["forward"])?,vector(&view["up"])?,phone.focused())?;Some((p.pose,p.scale))}
    })
}
fn pointer_ray(window:&Value,view:&Value,absolute:bool)->Result<V,String>{
    let forward=vector(&view["forward"])?;if !absolute{return Ok(forward);}
    let mobile=window["flight"]["mobile"]==true;
    let pointer=if mobile{
        let p:[f64;2]=serde_json::from_value(window["flight"]["pointer"].clone()).map_err(|_|"phone touch pointer required")?;p
    }else{
        let p:[f64;2]=serde_json::from_value(window["flight"]["workstation"]["editor"]["pointer"].clone()).map_err(|_|"phone pointer required")?;
        let width=window["logical_width"].as_f64().filter(|v|*v>0.).ok_or("phone viewport width required")?;
        let height=window["logical_height"].as_f64().filter(|v|*v>0.).ok_or("phone viewport height required")?;
        [p[0]/width*2.-1.,1.-p[1]/height*2.]
    };
    if pointer.iter().any(|v|!v.is_finite()){return Err("nonfinite phone pointer".into());}
    let tan=35f64.to_radians().tan();let aspect=window["aspect"].as_f64().unwrap_or(4./3.);
    Ok(unit(add(forward,add(mul(vector(&view["right"] )?,pointer[0]*tan*aspect),mul(vector(&view["up"] )?,pointer[1]*tan)))))
}
fn trace(host:&mut Host,eye:V,direction:V,distance:f64)->Result<Value,String>{
    let snapshot=host.invoke("TerrainGeometrySystem.Capture",json!({"position":eye,"time":0.}))?;
    host.invoke("TerrainGeometrySystem.Trace",json!({"snapshot":snapshot,"position":eye,"direction":direction,"distance":distance}))
}
fn placement(host:&mut Host,view:&Value)->Result<Option<Pose>,String>{
    let eye=vector(&view["position"])?;let forward=vector(&view["forward"])?;
    let hit=trace(host,eye,forward,phone::EQUIP_RADIUS)?;
    if hit.is_null(){return Ok(None);}
    let normal=unit(vector(&hit["normal"])?);let point=add(vector(&hit["position"] )?,mul(normal,0.008));
    let mut up=sub(forward,mul(normal,dot(forward,normal)));
    if length(up)<0.1{let view_up=vector(&view["up"])?;up=sub(view_up,mul(normal,dot(view_up,normal)));}
    let up=unit(up);let right=unit(cross(up,normal));Ok(Some(Pose::new(point,right,up)?))
}
fn update_weather(view:&Value)->Value{
    use crate::base_system::layer_four::atmosphere::{Lighting,calendar_at};
    let view=Lighting::enrich(view);let epoch=view["calendar"]["unix_seconds"].as_f64().unwrap_or(0.);
    let altitude=vector(&view["position"]).map_or(200.,|v|length(v)-crate::terrain::RADIUS);
    let forecast:Vec<_>=[6,12,18,24].into_iter().map(|hour|{
        let air=crate::physics::matter::climate::sample(&calendar_at(epoch+hour as f64*3600.),altitude);
        json!({"lead_hours":hour,"temperature_c":air.temperature_c,"cloud_fraction":air.cloud,"wind_kmh":air.wind_m_s*3.6})
    }).collect();
    json!({"temperature_c":view["air"]["temperature_c"],"wind_kmh":view["air"]["wind_m_s"].as_f64().unwrap_or(0.)*3.6,"cloud_fraction":view["air"]["cloud_fraction"],"forecast":forecast,"model":view["air"]["model"],"unavailable":["humidity","pressure","precipitation","wind_direction"]})
}
// Preserve the app choice, but a new game always places the handset at its desk.
fn restore_at_spawn(handset:&mut phone::State,saved:&Value)->Result<(),String>{
    let mut restored=saved.clone();
    restored["location"]=handset.snapshot()["location"].clone();
    restored["flashlight_on"]=json!(false);
    handset.restore(&restored)
}
fn save(host:&Host,phone:&phone::State)->Result<(),String>{
    use std::io::Write;
    let root=host.output_root();std::fs::create_dir_all(root).map_err(|e|e.to_string())?;
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    let pending=root.join(format!("phone-{}-{stamp}.pending",std::process::id()));
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&pending).map_err(|e|e.to_string())?;
    let data=serde_json::to_vec_pretty(&phone.snapshot()).map_err(|e|e.to_string())?;
    file.write_all(&data).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    std::fs::rename(pending,root.join("phone.json")).map_err(|e|e.to_string())
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{
    if hook=="Check" {return crate::phone::diagnostic::run(host);}
    let mut state=std::mem::take(&mut host.phone);
    let result=(||match hook{
        "Configure"=>{
            host.invoke("PhoneRenderSystem.Load",Value::Null)?;
            let position=if host.theatre.venue.is_some(){host.theatre.world([1.1,7.458,-47.45])}else{[0.,crate::terrain::RADIUS+0.008,0.]};
            let mut handset=phone::State::placed(Pose::new(position,[1.,0.,0.],[0.,0.,-1.])?)?;
            match std::fs::read(host.output_root().join("phone.json")){
                Ok(bytes)=>{if bytes.len()>16384{return Err("phone save exceeds budget".into());}let saved=serde_json::from_slice::<Value>(&bytes).map_err(|e|format!("phone save: {e}"))?;
                    restore_at_spawn(&mut handset,&saved)?;},
                Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(format!("phone save: {e}"))
            }
            state=State{handset:Some(handset),..State::default()};Ok(Value::Null)
        },
        "Status"=>Ok(state.status()),
        "Close"=>{if let Some(phone)=&mut state.handset{phone.apply(Command::Blur)?;save(host,phone)?;}state.previous=[false;4];state.interact_owned=false;
            #[cfg(target_os="ios")]unsafe{ww_ios_phone_state(false,false);}
            Ok(Value::Null)},
        "CheckScene"=>{
            let phone=state.handset.as_mut().ok_or("phone not configured")?;
            if input["restore"]==true{
                if let Some(snapshot)=state.diagnostic_snapshot.take(){phone.restore(&snapshot)?;}
                return Ok(phone.status());
            }
            let uv=match input["screen"].as_str(){Some("home")=>None,Some("flashlight")=>Some([0.24,0.24]),Some("camera")=>Some([0.75,0.24]),Some("meter")=>Some([0.24,0.62]),Some("weather")=>Some([0.75,0.62]),_=>return Err("diagnostic phone screen required".into())};
            if state.diagnostic_snapshot.is_none(){state.diagnostic_snapshot=Some(phone.snapshot());}
            phone.restore(&json!({"schema_version":1,"location":{"kind":"held"},"screen":"home","flashlight_on":false}))?;
            phone.apply(Command::ToggleFocus)?;if let Some(uv)=uv{phone.apply(Command::Tap(uv))?;}
            Ok(phone.status())
        },
        "Grip"=>{
            let Some(phone)=state.handset.as_ref()else{return Ok(json!({"active":false}));};
            if phone.location()!=Location::Held{return Ok(json!({"active":false}));}
            let held=phone::held_pose(vector(&input["position"] )?,vector(&input["forward"] )?,vector(&input["up"] )?,phone.focused())?;
            Ok(json!({"active":true,"locked":true,"left":world_point(held.pose,[-0.039,-0.045,-0.003],held.scale),"right":world_point(held.pose,[0.035,-0.032,-0.002],held.scale)}))
        },
        "Frame"=>{
            let Some(phone)=state.handset.as_mut()else{return Ok(Value::Null);};let view=&input["view"];
            let dt=input["dt"].as_f64().unwrap_or(0.).clamp(0.,0.25);
            let audio=if phone.screen()==phone::Screen::Meter&&host.system_enabled("EnvironmentAudioSystem")&&host.invoke("EnvironmentAudioSystem.Status",Value::Null)?["active"]==true{
                host.invoke("ChuckConnectionSystem.Telemetry",json!({})).ok().and_then(|v|Some(AudioSample{rms:v["listener_rms"].as_f64()?,peak:v["listener_peak"].as_f64()?}))
            }else{None};
            phone.tick(dt,audio)?;
            state.weather_elapsed+=dt;
            if phone.screen()==phone::Screen::Weather&&(state.weather.is_null()||state.weather_elapsed>=1.){state.weather=update_weather(view);state.weather_elapsed=0.;}
            let mut packet=phone.status();packet["equipped"]=json!(phone.location()==Location::Held);
            if let Some((pose,scale))=current_pose(phone,view)?{packet["visible"]=json!(phone.available());packet["pose"]=pose_json(pose);packet["scale"]=json!(scale);}else{packet["visible"]=json!(false);}
            packet["weather"]=state.weather.clone();Ok(packet)
        },
        "Input"=>{
            let Some(phone)=state.handset.as_mut()else{return Ok(json!({}));};let window=&input["window"];let view=&input["view"];
            let mobile=window["flight"]["mobile"]==true;
            let held=phone.location()==Location::Held;let was_focused=phone.focused();
            let keys=[window["flight"]["phone"]["interact"]==true||(mobile&&held&&window["flight"]["tool_input"]["drop"]==true),window["flight"]["phone"]["focus"]==true,window["flight"]["select"]==true,input["escape"]==true];
            let edges: [bool;4]=std::array::from_fn(|i|keys[i]&&!state.previous[i]);state.previous=keys;if !keys[0]{state.interact_owned=false;}
            let blocked=input["blocked"]==true||window["flight"]["focused"]==false;
            let mut consumed_select=was_focused&&keys[2];let mut consumed_escape=false;
            if blocked{phone.apply(Command::Blur)?;}else if view["position"].is_array(){
                let eye=vector(&view["position"])?;
                let asset=host.phone_render.asset.clone().ok_or("phone model missing")?;
                let direction=pointer_ray(window,view,mobile||was_focused).ok();
                let hit=current_pose(phone,view)?.and_then(|(p,s)|direction.and_then(|d|asset.hit(p,s,eye,d,if held{0.}else{0.025},phone::EQUIP_RADIUS)));
                let mobile_action=mobile&&edges[2]&&hit.is_some();
                if edges[0]||(!held&&mobile_action){
                    if held||hit.is_some(){
                        state.interact_owned=keys[0];
                        let clear=if held{true}else{let distance=current_pose(phone,view)?.map_or(0.,|(p,_)|length(sub(p.position,eye)));let obstruction=trace(host,eye,direction.unwrap(),distance.min(phone::EQUIP_RADIUS))?;obstruction["distance"].as_f64().is_none_or(|d|d+0.08>=distance)};
                        if clear{
                            let hands_free=held||host.invoke("PhysicsSystem.GameFrame",Value::Null)?["can_stow_tool"]==true;
                            let placement=if held&&!was_focused{placement(host,view)?}else{None};
                            phone.apply(Command::Interact{observer:eye,hands_free,placement})?;
                        }
                        consumed_select|=mobile_action;
                    }
                }else if held&&(edges[1]||(mobile_action&&!was_focused)){
                    phone.apply(Command::ToggleFocus)?;consumed_select|=mobile_action;
                }else if was_focused&&edges[3]{phone.apply(Command::Blur)?;consumed_escape=true;}
                else if was_focused&&edges[2]{if let Some(uv)=hit{phone.apply(Command::Tap(uv))?;}}
            }
            let focus_changed=was_focused!=phone.focused();
            if focus_changed&&!blocked&&!cfg!(target_os="ios"){
                host.invoke("GraphicsConnectionSystem.InputMode",json!({"entity":input["entity"],"pointer":phone.focused()}))?;
            }
            // UIKit deduplicates this state. Publish on the first input frame
            // too, including when startup restored an already-held handset.
            #[cfg(target_os="ios")]
            unsafe{ww_ios_phone_state(phone.location()==Location::Held,phone.focused());}
            Ok(json!({"equipped":phone.location()==Location::Held,"focused":phone.focused(),"focus_changed":focus_changed,"consumed_select":consumed_select,"consumed_escape":consumed_escape,"consumed_reset":held&&keys[1],"consumed_interact":state.interact_owned,"screen":phone.screen().name()}))
        },
        _=>Err("unknown phone hook".into())
    })();host.phone=state;result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn phone_pointer_uses_logical_dimensions_and_rejects_missing_coordinates(){
        let view=json!({"forward":[0.,0.,-1.],"right":[1.,0.,0.],"up":[0.,1.,0.]});
        let window=json!({"logical_width":800.,"logical_height":1200.,"aspect":2./3.,"flight":{"workstation":{"editor":{"pointer":[200.,300.]}}}});
        let ray=pointer_ray(&window,&view,true).unwrap();assert!(ray[0]<0.&&ray[1]>0.&&ray[2]<0.);
        assert!(pointer_ray(&json!({"logical_width":800.,"logical_height":1200.}),&view,true).is_err());
        assert_eq!(pointer_ray(&Value::Null,&view,false).unwrap(),[0.,0.,-1.]);
    }
    #[test]fn phone_forecast_uses_current_game_climate_without_fabricated_sensors(){
        let value=update_weather(&json!({"position":[0.,crate::terrain::RADIUS+200.,0.]}));
        assert!(value["temperature_c"].is_number());assert_eq!(value["forecast"].as_array().unwrap().len(),4);
        assert!(value["humidity"].is_null()&&value["pressure"].is_null()&&value["precipitation"].is_null());
    }
}

#[cfg(test)] mod spawn_tests {
 use super::*;
 #[test] fn phone_new_game_recovers_held_stowed_and_remote_saved_locations() {
  let pose=Pose::new([12.,43.,-7.],[1.,0.,0.],[0.,0.,-1.]).unwrap();
  for location in [json!({"kind":"held"}),json!({"kind":"stowed"}),json!({"kind":"placed","pose":{"position":[999.,888.,777.],"right":[1.,0.,0.],"up":[0.,1.,0.],"front":[0.,0.,1.]}})] {
   let mut handset=phone::State::placed(pose).unwrap();
   restore_at_spawn(&mut handset,&json!({"schema_version":1,"location":location,"screen":"weather","flashlight_on":false})).unwrap();
   assert_eq!(handset.location(),Location::Placed(pose));assert_eq!(handset.screen(),phone::Screen::Weather);assert!(!handset.focused());
  }
 }
}
