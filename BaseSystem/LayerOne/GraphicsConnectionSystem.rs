//! Connection to the explicitly selected native graphics API.
use crate::{base_entity::Entity, host::Host};
use serde_json::{Value, json};

#[cfg(not(target_os = "ios"))]
#[path = "../../Render/mod.rs"]
mod renderer;
#[cfg(target_os = "ios")]
#[path = "../../Render/ios.rs"]
mod renderer;

struct Suspended {
    session:Option<renderer::Session>,
    entity: u64,
    spec: Value,
    texture_owner: Option<u64>,
    texture: Option<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct State {
    session: Option<renderer::Session>,
    prewarmed_workstation: Option<renderer::Session>,
    entity: Option<u64>,
    texture_owner:Option<u64>,
    texture: Option<Vec<u8>>,
    suspended: Option<Suspended>,
    resident:Option<(u64,Option<renderer::Session>)>,
    #[cfg(test)]
    pub(crate) test_driver: bool,
    #[cfg(test)]
    pub(crate) fail_present: bool,
    #[cfg(test)]
    pub(crate) fail_prepare: bool,
    #[cfg(test)]
    pub(crate) close_after: Option<u32>,
    #[cfg(test)]
    pub(crate) workstation_frame: Option<Vec<[f32; 4]>>,
    #[cfg(test)] pub(crate) scene_frame:Option<crate::base_system::layer_four::appearance_system::Scene>,
    #[cfg(test)]
    pub(crate) cursor_label: Value,
    #[cfg(test)]pub(crate) selection_range:Value,
    #[cfg(test)]pub(crate) plugin_surface:Value,
}

impl State {
    pub(crate) fn suspended_entity(&self)->Option<u64>{self.suspended.as_ref().map(|s|s.entity)}
    pub(crate) fn suspended_texture_owner(&self)->Option<u64>{self.suspended.as_ref().and_then(|s|s.texture_owner)}
    pub(crate) fn texture_owner(&self)->Option<u64>{self.texture_owner}
    pub(crate) fn prepared_entity(&self) -> Option<u64> {
        self.entity
    }
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    if hook=="PrewarmWorkstation" {
        if host.backend()!="WebGPU" || !cfg!(target_os="macos") {return Ok(Value::Null);}
        #[cfg(test)] if host.graphics.test_driver {return Ok(Value::Null);}
        #[cfg(target_os="macos")]
        if host.graphics.prewarmed_workstation.is_none(){
            let session=renderer::Session::prewarm_workstation(host.root(),host.output_root())?;
            host.graphics.prewarmed_workstation=Some(session);
        }
        return Ok(Value::Null);
    }
    let id = if hook == "Prepare" {
        input["id"].as_u64()
    } else {
        input["entity"].as_u64()
    }
    .ok_or("Entity ID required")?;
    match hook {
        "ParkWorkstation" => {
            if host.graphics.entity!=Some(id)||host.graphics.resident.is_some(){return Err("park requires the focused workstation".into());}
            if let Some(session)=&mut host.graphics.session {session.finish()?;#[cfg(not(target_os="ios"))] session.park()?;}
            host.graphics.resident=Some((id,host.graphics.session.take()));
            host.graphics.entity=None;
            Ok(Value::Null)
        }
        "FocusWorkstation" => {
            if host.graphics.entity.is_some()||host.graphics.resident.as_ref().map(|r|r.0)!=Some(id){return Err("focus requires the parked workstation".into());}
            let (_,session)=host.graphics.resident.take().unwrap();
            host.graphics.session=session;host.graphics.entity=Some(id);
            Ok(Value::Null)
        }
        "Suspend" => {
            if host.graphics.suspended.is_some() || host.graphics.entity != Some(id) {
                return Err("graphics suspend requires the active scene and no suspended scene".into());
            }
            let spec = host.invoke("EntitySystem.Get", json!({"entity":id}))?;
            let retain_native=cfg!(target_os="macos") && host.backend()=="WebGPU";
            if let Some(session)=&mut host.graphics.session {
                #[cfg(not(target_os="ios"))]
                if retain_native {session.park()?;}else{session.input_mode(true)?;}
                session.finish()?;
            }
            let session=host.graphics.session.take();
            let session=if retain_native {session}else{drop(session);None};
            host.graphics.suspended = Some(Suspended {entity:id, spec,session,
                texture_owner:host.graphics.texture_owner.take(), texture:host.graphics.texture.take()});
            host.graphics.entity = None;
            Ok(Value::Null)
        }
        "Resume" => {
            if host.graphics.entity.is_some() {return Err("release the focused scene before resuming graphics".into());}
            let retained = host.graphics.suspended.as_ref().filter(|s|s.entity==id)
                .ok_or("graphics resume requires the suspended scene")?;
            let entity = Entity::from_spec(id, &retained.spec)?;
            #[cfg(test)]
            if host.graphics.test_driver {
                if host.graphics.fail_prepare {return Err("injected preparation failure".into());}
                let retained = host.graphics.suspended.take().unwrap();
                host.graphics.entity = Some(id);
                host.graphics.texture_owner = retained.texture_owner;
                host.graphics.texture = retained.texture;
                return Ok(Value::Null);
            }
            let parked=host.graphics.suspended.as_mut().unwrap().session.take();
            let reused=parked.is_some();
            let mut session=if let Some(session)=parked{session}else{renderer::Session::prepare(host.root(),host.output_root(),host.backend(),&entity)?};
            let retained=host.graphics.suspended.as_ref().unwrap();
            session.environment(retained.spec["shader"].as_str().unwrap_or("vertex_color"));
            if !reused {if let Some(bytes)=&retained.texture {session.texture(bytes)?;}}

            #[cfg(not(target_os="ios"))]
            if let Some(image) = &host.theatre.monitor_image { session.monitor_image(image)?; }
            let retained = host.graphics.suspended.take().unwrap();
            host.graphics.session = Some(session);
            host.graphics.entity = Some(id);
            host.graphics.texture_owner = retained.texture_owner;
            host.graphics.texture = retained.texture;
            Ok(Value::Null)
        }
        "Prepare" => {
            if host.graphics.entity.is_some() {
                return Err("initial graphics connection already has a prepared Entity".into());
            }
            let entity = Entity::from_spec(id, input)?;
            #[cfg(test)]
            if host.graphics.test_driver {
                if host.graphics.fail_prepare {
                    return Err("injected preparation failure".into());
                }
                host.graphics.entity = Some(id);
                host.emit(json!({"event":"native_prepared","entity":id,"backend":"TestDriver","visible":false}))?;
                return Ok(Value::Null);
            }
            let mut session = if entity.shader()==Some("workstation_panel") && host.graphics.prewarmed_workstation.is_some(){
                host.graphics.prewarmed_workstation.take().unwrap()
            }else{renderer::Session::prepare(host.root(),host.output_root(),host.backend(),&entity)?};
            session.environment(input["shader"].as_str().unwrap_or("vertex_color"));
            if let Ok(bytes) = std::fs::read(host.root().join("Project.json")) {
                if let Ok(project) = serde_json::from_slice::<Value>(&bytes) {
                    if let Some(name) = project["display_name"].as_str() {
                        session.title(name, false)?;
                    }
                }
            }
            let info = session.info();
            host.graphics.session = Some(session);
            host.graphics.entity = Some(id);
            host.emit(json!({"event":"native_prepared","entity":id,"backend":host.backend(),"visible":false,"device":info}))?;
            Ok(Value::Null)
        }
        "Present" => {
            if host.graphics.entity != Some(id) {
                return Err("Entity has not been prepared by this connection".into());
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                if host.graphics.fail_present {
                    return Err("injected presentation failure".into());
                }
                let frames = host.graphics.close_after.unwrap_or(1);
                if frames > 0 {
                    host.emit(json!({"event":"frame_presented","entity":id,"backend":"TestDriver","frame":1}))?;
                }
                let reason = if host.graphics.close_after.is_some() {
                    "window_closed"
                } else {
                    "frame_limit"
                };
                host.emit(json!({"event":"presentation_ended","entity":id,"reason":reason,"frames":frames}))?;
                return Ok(json!({"backend":"TestDriver","frames":frames,"end_reason":reason}));
            }
            let mut session = host
                .graphics
                .session
                .take()
                .ok_or("native session missing")?;
            let frames = host.frame_limit();
            let result=session.present(if host.live(){None}else{Some(frames)},host.frame_period_us(),host.close_after_frames(),|frame|host.emit(json!({"event":"frame_presented","entity":id,"backend":host.backend(),"frame":frame})));

            host.graphics.session = Some(session);
            if let Ok(report) = &result {
                host.emit(json!({"event":"presentation_ended","entity":id,"reason":report["end_reason"],"close_source":report["close_source"],"frames":report["frames"]}))?;
            }
            result
        }
        "Texture"|"ReleaseTexture"=>{
            if hook=="ReleaseTexture" && host.graphics.suspended_entity()==Some(id) {
                let retained=host.graphics.suspended.as_mut().unwrap();
                if retained.texture_owner.is_some_and(|owner|Some(owner)!=input["owner"].as_u64()) {
                    return Err("texture belongs to another retained Entity".into());
                }
                retained.texture_owner=None;
                retained.texture=None;
                return Ok(Value::Null);
            }
            if host.graphics.entity!=Some(id){return Err("texture target not prepared".into());}
            let owner=input["owner"].as_u64().unwrap();
            let data=host.invoke("EntitySystem.Get",json!({"entity":owner}))?;
            if data["shader"]!="player_body" {return Err("texture owner requires a player Entity".into());}
            if hook=="ReleaseTexture"&&host.graphics.texture_owner.is_some_and(|id|id!=owner){return Err("texture belongs to another Entity".into());}
            if hook=="Texture"&&host.graphics.texture_owner.is_some(){return Err("texture already prepared".into());}
            let bytes=if hook=="Texture"{std::fs::read(host.root().join(input["path"].as_str().unwrap())).map_err(|e|e.to_string())?}else{vec![1,0,0,0,1,0,0,0,255,255,255,255]};
            if bytes.len()<12{return Err("truncated RGBA texture".into());}
            let w=u32::from_le_bytes(bytes[..4].try_into().unwrap());let h=u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            if w==0||h==0||w>2048||h>2048||bytes.len()!=8+w as usize*h as usize*4{return Err("invalid RGBA texture dimensions".into());}
            #[cfg(test)] if host.graphics.test_driver {host.graphics.texture_owner=if hook=="Texture"{Some(owner)}else{None};host.graphics.texture=(hook=="Texture").then_some(bytes);return Ok(Value::Null);}
            host.graphics.session.as_mut().ok_or("native session missing")?.texture(&bytes)?;
            #[cfg(not(target_os="ios"))]
            if host.graphics.suspended.is_none() {
                if let Some(image) = &host.theatre.monitor_image {
                    host.graphics.session.as_mut().unwrap().monitor_image(image)?;
                }
            }
            host.graphics.texture_owner=if hook=="Texture"{Some(owner)}else{None};
            host.graphics.texture=(hook=="Texture").then_some(bytes);Ok(Value::Null)
        }
        "InputMode" => {
            if host.graphics.entity != Some(id) {
                return Err("input Entity is not prepared".into());
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                return Ok(Value::Null);
            }
            host.graphics
                .session
                .as_mut()
                .ok_or("native session missing")?
                .input_mode(input["pointer"].as_bool().unwrap())?;
            #[cfg(target_os="macos")]
            host.graphics.session.as_mut().ok_or("native session missing")?
                .pause_menu(input["pointer"]==true && input["pause_menu"].is_object(), &input["pause_menu"])?;
            Ok(Value::Null)
        }
        "PluginSurface"=>{
            if host.graphics.entity!=Some(id){return Err("plugin surface Entity is not prepared".into());}
            #[cfg(test)]if host.graphics.test_driver{host.graphics.plugin_surface=input["surface"].clone();return Ok(Value::Null);}
            #[cfg(any(target_os="macos",target_os="ios"))] {
                let font=host.root().join("Procedures/Fonts/AlegreyaSans-Regular.ttf");
                host.graphics.session.as_mut().ok_or("native session missing")?.plugin_surface(&input["surface"],&font)?;Ok(Value::Null)
            }
            #[cfg(not(any(target_os="macos",target_os="ios")))]{Err("native plugin labels are implemented on macOS".into())}
        }
        "CcParameterPicker"=>{
            if host.graphics.entity!=Some(id){return Err("CC picker Entity is not prepared".into());}
            #[cfg(test)]if host.graphics.test_driver{return Ok(Value::Null);}
            #[cfg(any(target_os="macos",target_os="ios"))] {host.graphics.session.as_mut().ok_or("native session missing")?.cc_parameter_picker(&input["picker"])?;Ok(Value::Null)}
            #[cfg(not(any(target_os="macos",target_os="ios")))]{Err("native CC picker is currently implemented on macOS".into())}
        }
        "SelectionRange"=>{
            if host.graphics.entity!=Some(id){return Err("selection Entity is not prepared".into());}
            #[cfg(test)]if host.graphics.test_driver{host.graphics.selection_range=input["range"].clone();return Ok(Value::Null);}
            #[cfg(any(target_os="macos",target_os="ios"))] {host.graphics.session.as_mut().ok_or("native session missing")?.selection_range(&input["range"])?;Ok(Value::Null)}
            #[cfg(not(any(target_os="macos",target_os="ios")))]{Err("native selection overlay is currently implemented on macOS".into())}
        }
        "CursorLabel" => {
            if host.graphics.entity!=Some(id){return Err("cursor label Entity is not prepared".into());}
            #[cfg(test)] if host.graphics.test_driver {host.graphics.cursor_label=input["label"].clone();return Ok(Value::Null);}
            #[cfg(any(target_os="macos",target_os="ios"))] {
                let font=host.root().join("Procedures/Fonts/AlegreyaSans-Regular.ttf");
                host.graphics.session.as_mut().ok_or("native session missing")?.cursor_label(&input["label"],&font)?;
                Ok(Value::Null)
            }
            #[cfg(not(any(target_os="macos",target_os="ios")))] {Err("native cursor labels are currently implemented on macOS".into())}
        }
        "Caption" => {
            if host.graphics.entity != Some(id) {
                return Err("caption Entity is not prepared".into());
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                return Ok(Value::Null);
            }
            host.graphics
                .session
                .as_mut()
                .ok_or("native session missing")?
                .title(input["text"].as_str().unwrap(), input["hud"] == true)?;
            Ok(Value::Null)
        }
        "SetView" => {
            let triangles: Vec<[f32; 24]> =
                serde_json::from_value(input["triangles"].clone()).map_err(|e| e.to_string())?;
            let plants = if input["vegetation"].is_null() { None } else {
                Some(crate::vegetation::packet::Packet::from_geometry(&input["vegetation"],
                    crate::vegetation::packet::Scene::from_view(&input["view"])?)?)
            };
            set_view_numeric(host, input, &triangles, plants)
        }
        "Pump" => {
            if input["hidden_scene"]==true {
                if host.graphics.entity!=Some(id){return Err("hidden draw requires the active preload scene".into());}
                #[cfg(test)] if host.graphics.test_driver {return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true,"hidden_scene":true,"window_state_observed":false}));}
                #[cfg(not(target_os="ios"))]
                host.graphics.session.as_mut().ok_or("preload renderer missing")?.hidden_scene()?;
                return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true,"hidden_scene":true,"window_state_observed":false}));
            }
            if input["background"]==true {
                if host.graphics.entity!=Some(id) && host.graphics.resident.as_ref().map(|r|r.0)!=Some(id){return Err("background draw requires the resident workstation".into());}
                #[cfg(test)] if host.graphics.test_driver {return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true,"background":true,"window_state_observed":false}));}
                let session=if host.graphics.entity==Some(id){host.graphics.session.as_mut()}else{host.graphics.resident.as_mut().filter(|r|r.0==id).and_then(|r|r.1.as_mut())}.ok_or("resident renderer missing")?;
                #[cfg(not(target_os="ios"))]
                let image=session.background_capture()?;
                #[cfg(target_os="ios")]
                let image:Vec<u8>=return Err("resident monitor is unavailable on iOS".into());
                host.theatre.monitor_image=Some(image);
                if host.graphics.entity!=Some(id) {
                    if let (Some(game),Some(image))=(&mut host.graphics.session,&host.theatre.monitor_image){game.monitor_image(image)?;}
                }
                return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true,"background":true,"window_state_observed":false}));
            }
            if host.graphics.entity != Some(id) {
                return Err("Entity not prepared".into());
            }
            if !input["force_draw"].is_null() && !input["force_draw"].is_boolean() {
                return Err("force_draw must be boolean".into());
            }
            if !input["finish_only"].is_null() && !input["finish_only"].is_boolean() {
                return Err("finish_only must be boolean".into());
            }
            if input["finish_only"]==true && input["force_draw"]==true {
                return Err("finish_only cannot request a new draw".into());
            }
            #[cfg(not(target_os="ios"))]
            if input["capture_on_exit"]==true {
                let session = if host.graphics.entity==Some(id){host.graphics.session.as_mut()}else{host.graphics.resident.as_mut().filter(|r|r.0==id).and_then(|r|r.1.as_mut())}.ok_or("native session missing")?;
                let (frame, image) = session.capture_exit_frame()?;
                host.theatre.monitor_image = Some(image);
                return Ok(frame);
            }
            if input["finish_only"]==true {
                #[cfg(test)] if host.graphics.test_driver {
                    return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":false,"completion_only":true,"completed":true,"window_state_observed":false}));
                }
                // A measurement/ownership boundary, with no input consumption,
                // new draw or invalidation acknowledgement. Release uses this
                // same existing session completion operation.
                let session=host.graphics.session.as_mut().ok_or("native session missing")?;
                session.finish()?;
                #[cfg(not(target_os="ios"))] let offset=session.gpu_draw_count();
                #[cfg(target_os="ios")] let offset=0;
                return Ok(json!({"gpu_draw_offset":offset,"closed":false,"keys":[false,false,false],"drawn":false,
                    "completion_only":true,"completed":true,"window_state_observed":false}));
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true}));
            }
            {
                if let Some(path) = std::env::var_os("WORMWOOD_STOP_FILE") {
                    let path = std::path::PathBuf::from(path);
                    if path.parent() != Some(host.output_root())
                        || !path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| s.starts_with("session-stop-"))
                    {
                        return Err("invalid session stop path".into());
                    }
                    if path.exists() {
                        host.graphics
                            .session
                            .as_ref()
                            .ok_or("native session missing")?
                            .request_close()?;
                    }
                }
                #[cfg(not(target_os = "ios"))]
                let report = host
                    .graphics
                    .session
                    .as_mut()
                    .ok_or("native session missing")?
                    .pump(input["force_draw"] == true, false)?;
                #[cfg(target_os = "ios")]
                let report = host
                    .graphics
                    .session
                    .as_mut()
                    .ok_or("native session missing")?
                    .pump()?;
                let mut event=json!({"event":"window_pumped","entity":id,"drawn":report["drawn"],"closed":report["closed"],"readback_verified":report["device"]["readback_verified"]});
                if std::env::var("WORMWOOD_WORKSTATION_INPUT_VALIDATION").as_deref()==Ok("1") {
                    event["input_validation"]=json!({
                        "unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_millis()),
                        "focused":report["flight"]["focused"],"panels":report["flight"]["workstation"]["panels"],
                        "escape":report["keys"][1],"quit":report["flight"]["quit"],"preempted_draw":report["input_preempts_draw"],
                        "physical_key_presses":report["flight"]["workstation"]["physical_key_presses"],
                        "native_panel_actions":report["flight"]["workstation"]["native_panel_actions"],
                        "native_focus":report["flight"]["workstation"]["native_focus"],
                        "draw_status":report["draw_status"],"incremental":report["device"]["workstation_shader"]["prerender"]["incremental_bake"]});
                }
                host.emit(event)?;
                Ok(report)
            }
        }
        "Release" => {
            if host.graphics.resident.as_ref().map(|r|r.0)==Some(id) {
                if let Some(session)=&mut host.graphics.resident.as_mut().unwrap().1 {session.finish()?;}
                host.graphics.resident=None;return Ok(Value::Null);
            }
            // Failed focus restoration still permits explicit shutdown of the
            // retained scene without accidentally releasing another window.
            if host.graphics.suspended_entity()==Some(id) {
                host.graphics.suspended=None;
                return Ok(Value::Null);
            }
            if let Some(prepared) = host.graphics.entity {
                if prepared != id {
                    return Err("cannot release another Entity's resources".into());
                }
                if let Some(mut session) = host.graphics.session.take() {
                    if let Err(error) = session.finish() {
                        host.graphics.session = Some(session);
                        return Err(error);
                    }
                }
                host.graphics.entity = None;
                #[cfg(test)] {host.graphics.cursor_label=Value::Null;host.graphics.selection_range=Value::Null;host.graphics.plugin_surface=Value::Null;}
                host.graphics.texture_owner=None;
                host.graphics.texture=None;
                host.emit(json!({"event":"native_released","entity":id}))?;
            }
            Ok(Value::Null)
        }
        _ => Err("unknown graphics hook".into()),
    }
}

/// Shared validation and native submission for JSON and numeric SetView callers.
/// The Host owns route permissions, tracing and the application Audicle.
pub(crate) fn set_view_numeric(host: &mut Host, input: &Value, triangles: &[[f32;24]],
    plants: Option<crate::vegetation::packet::Packet>) -> Result<Value,String> {
    let workstation = if input["workstation"].is_null() {
        None
    } else {
        Some(serde_json::from_value::<Vec<[f32;4]>>(input["workstation"].clone())
            .map_err(|e| format!("invalid workstation optical frame: {e}"))?)
    };
    set_view_packet(host, input, triangles, plants, workstation.as_deref(),None)
}

/// The same SetView operation with a borrowed optical packet. No JSON number
/// allocation, cloning or reparsing is needed for presentation-owned rows.
pub(crate) fn set_workstation_view_numeric(host: &mut Host, input: &Value,
    triangles: &[[f32;24]], rows: &[[f32;4]]) -> Result<Value,String> {
    set_view_packet(host, input, triangles, None, Some(rows),None)
}

pub(crate) fn set_workstation_scene_numeric(host:&mut Host,input:&Value,triangles:&[[f32;24]],plants:Option<crate::vegetation::packet::Packet>,rows:&[[f32;4]],water:&[crate::water::presentation::Packet])->Result<Value,String> {
    set_view_packet(host,input,triangles,plants,Some(rows),Some(water))
}

pub(crate) fn set_view_water_numeric(host:&mut Host,input:&Value,triangles:&[[f32;24]],plants:Option<crate::vegetation::packet::Packet>,water:&[crate::water::presentation::Packet])->Result<Value,String>{
    set_view_packet(host,input,triangles,plants,None,Some(water))
}

fn set_view_packet(host: &mut Host, input: &Value, triangles: &[[f32;24]],
    plants: Option<crate::vegetation::packet::Packet>, workstation: Option<&[[f32;4]]>, numeric_water:Option<&[crate::water::presentation::Packet]>) -> Result<Value,String> {
    let id = input["entity"].as_u64().filter(|id| *id > 0).ok_or("Entity ID required")?;
    if host.graphics.entity != Some(id) && host.graphics.resident.as_ref().map(|r|r.0)!=Some(id) { return Err("view Entity is not prepared".into()); }
    if let Err(error)=crate::scene_limits::validate(triangles,input["world_backdrop"]==true) {
        host.emit(json!({"event":"scene_rejected","triangles":triangles.len(),"view":input["view"],"error":error}))?;
        return Err(error);
    }
    let parsed_water;
    let water=if let Some(water)=numeric_water{water}else{parsed_water=crate::water::presentation::parse_collection(&input["water"])?;&parsed_water};
    if water.len()>8{return Err("water packet collection exceeds budget".into());}
    for packet in water{crate::water_render_data::validate_numeric(packet)?;}
    if let Some(p)=&plants { p.validate()?; }
    if let Some(rows)=workstation { renderer::validate_workstation_frame(rows)?; }
    let reuse_world=input["reuse_world"]==true;
    if reuse_world&&(workstation.is_none()||input["world_backdrop"]!=true||!triangles.is_empty()||plants.is_some()){
        return Err("retained world update accepts only workstation display rows".into());
    }
    #[cfg(test)] if host.graphics.test_driver {
        host.graphics.workstation_frame=workstation.map(<[[f32;4]]>::to_vec);
        if reuse_world&&host.graphics.scene_frame.is_none(){return Err("retained game world is not prepared".into());}
        if !reuse_world && host.graphics.entity==Some(id){host.graphics.scene_frame=(input["world_backdrop"]==true).then(||crate::base_system::layer_four::appearance_system::Scene{view:input["view"].clone(),water:water.to_vec(),triangles:triangles.to_vec(),vegetation:plants});}
        return Ok(Value::Null);
    }
    let root = host.root().to_path_buf();
    let session = if host.graphics.entity==Some(id){host.graphics.session.as_mut()}else{host.graphics.resident.as_mut().filter(|r|r.0==id).and_then(|r|r.1.as_mut())}.ok_or("native session missing")?;
    if input["capture"] == true { session.capture_next(); }
    if let Some(rows)=workstation {
        session.set_workstation(rows)?;
        if host.graphics.entity!=Some(id) {return Ok(Value::Null);}
        let overlay:Vec<[f32;24]>=if input["audio_overlay"].is_null(){Vec::new()}else{serde_json::from_value(input["audio_overlay"].clone()).map_err(|e|format!("audio overlay: {e}"))?};
        let separate_overlay=session.set_audio_overlay(&overlay)?;
        if reuse_world{session.require_world_backdrop()?;return Ok(Value::Null);}
        if input["world_backdrop"]==true {
            let observer_changed=session.set_world_backdrop(&root,true)?;
            #[cfg(not(target_os="ios"))]
            if observer_changed {if let Some(image)=&host.theatre.monitor_image {session.monitor_image(image)?;}}
            let mut batch=triangles.to_vec();
            if !separate_overlay{batch.extend_from_slice(&overlay);}
            batch.extend(crate::base_system::layer_four::workstation_panel_system::fullscreen());
            session.set_batch(&batch)?;
            session.set_water_numeric(&root,water,&input["view"],triangles)?;
            session.set_vegetation(&root,plants)?;
        }else{
            session.set_world_backdrop(&root,false)?;
            session.table_eye(input)?;
            session.set_batch(triangles)?;
            session.set_vegetation(&root,None)?;
        }
    } else {
        session.set_batch(triangles)?;
        session.set_water_numeric(&root,water,&input["view"],triangles)?;
        session.set_vegetation(&root,plants)?;
    }
    host.emit(json!({"event":"view_updated","entity":id,"triangles":triangles.len()}))?;
    Ok(Value::Null)
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    fn host(name:&str)->Host {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("wormwood-completion-{name}-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        host.graphics.entity=Some(7);
        host.graphics.test_driver=true;
        host
    }
    #[test]
    fn completion_boundary_does_not_draw_or_release_the_active_scene() {
        let mut host=host("preserve");
        let completed=dispatch(&mut host,"Pump",&json!({"entity":7,"finish_only":true})).unwrap();
        assert_eq!(completed["completed"],true);
        assert_eq!(completed["drawn"],false);
        assert_eq!(completed["window_state_observed"],false);
        assert_eq!(host.graphics.entity,Some(7));
        let next=dispatch(&mut host,"Pump",&json!({"entity":7,"force_draw":true})).unwrap();
        assert_eq!(next["drawn"],true);
        assert!(next["completion_only"].is_null());
    }
    #[test]
    fn completion_boundary_rejects_wrong_scene_and_contradictory_requests() {
        let mut host=host("reject");
        for input in [json!({"entity":8,"finish_only":true}),
            json!({"entity":7,"finish_only":"true"}),
            json!({"entity":7,"finish_only":true,"force_draw":true})] {
            assert!(dispatch(&mut host,"Pump",&input).is_err());
            assert_eq!(host.graphics.entity,Some(7));
        }
    }
    #[test]
    fn ordinary_pump_keeps_its_existing_behavior() {
        let mut host=host("ordinary");
        for input in [json!({"entity":7}),json!({"entity":7,"finish_only":false})] {
            let frame=dispatch(&mut host,"Pump",&input).unwrap();
            assert_eq!(frame["drawn"],true);
            assert!(frame["completion_only"].is_null());
        }
    }
}

#[cfg(not(target_os="ios"))]
pub(crate) fn check_theatre_exterior(root:&std::path::Path,output:&std::path::Path,backend:&str)->Result<(),String>{
    renderer::check_theatre_exterior(root,output,backend)
}

#[cfg(not(target_os="ios"))]
pub(crate) fn check_flashlight(root:&std::path::Path,output:&std::path::Path,backend:&str)->Result<(),String>{renderer::check_flashlight(root,output,backend)}

#[cfg(not(target_os="ios"))]
pub(crate) fn check_trees(root:&std::path::Path,output:&std::path::Path,backend:&str)->Result<(),String>{renderer::check_trees(root,output,backend)}
