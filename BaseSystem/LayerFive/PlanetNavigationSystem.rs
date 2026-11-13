//! Mirror flight and tracked excavation around the editable planet.
use crate::host::Host;
use serde_json::{Value, json};
const RADIUS: f64 = 6_371_000.;
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
fn normalized(a: V) -> V {
    let n = dot(a, a).sqrt().max(1e-12);
    a.map(|x| x / n)
}
fn rotate(v: V, axis: V, angle: f64) -> V {
    let (s, c) = angle.sin_cos();
    let p = cross(axis, v);
    let d = dot(axis, v);
    std::array::from_fn(|i| v[i] * c + p[i] * s + axis[i] * d * (1. - c))
}
#[path = "PlayerControls.rs"]
mod controls;
#[derive(Default)]
pub(crate) struct State {
    player: controls::Player,
    event: Option<Value>,
    scene_event: Option<Value>,
    scene_time: f64,
    next_water_disturbance: f64,
    last_water_edit: Value,
    submerged: bool,
    last_dig: bool,
    cheat_keys: [bool; 3],
    show_sound_speakers: bool,
    night_vision: bool,
    time_override_minutes: Option<u16>,
    safe_position: Option<V>,
    aspect: Option<f64>,
    pending_jump: bool,
    tool_equipped: bool,
    tool_reach: Value,
    view_offset: V,
    view: Value,
    work_bend: f64,
    npc_caption:Option<String>,
    portal_loading:f64,
    portal_destination:Option<V>,
    portal_ready:bool,
}
pub(crate) fn valid(v: &Value) -> bool {
    crate::base_system::layer_five::flight_system::valid(v)
        && v["boost"].is_boolean()
        && (v["water_edit"].is_null()
            || v["water_edit"].is_object()
                && [
                    "place", "connect", "radius", "clear", "material", "source", "select",
                ]
                .contains(&v["water_edit"]["operation"].as_str().unwrap_or("")))
        && (v["tool_input"].is_null() || {
            let t = &v["tool_input"];
            ["held", "rotate", "drop"]
                .iter()
                .all(|k| t[*k].is_boolean())
                && (t["work"].is_null() || t["work"].is_boolean())
                && t["delta"].as_array().is_some_and(|a| {
                    a.len() == 2
                        && a.iter().all(|x| {
                            x.as_f64()
                                .is_some_and(|x| x.is_finite() && x.abs() <= 1000.)
                        })
                })
                && ["twist", "reach"].iter().all(|k| {
                    t[*k]
                        .as_f64()
                        .is_some_and(|x| x.is_finite() && x.abs() <= 1000.)
                })
        })
        && (v["cheat_keys"].is_null()
            || v["cheat_keys"]
                .as_array()
                .is_some_and(|a| a.len() == 3 && a.iter().all(Value::is_boolean)))
        && (v["cheats"].is_null()
            || ["enabled", "flight", "noclip"]
                .iter()
                .all(|k| v["cheats"][*k].is_boolean())
                && v["cheats"]["sequence"].as_u64().is_some())
        && (v["mobile"].is_null() || v["mobile"].is_boolean())
        && (v["time_override_minutes"].is_null() || v["time_override_minutes"].as_i64().is_some_and(|m|(-1..1440).contains(&m)))
        && (v["axis"].is_null()
            || v["axis"].as_array().is_some_and(|a| {
                a.len() == 2
                    && a.iter().all(|x| {
                        x.as_f64()
                            .is_some_and(|x| x.is_finite() && (-1.0..=1.).contains(&x))
                    })
            }))
        && (v["touch"].is_null()
            || ["active", "held", "tap"]
                .iter()
                .all(|k| v["touch"][*k].is_boolean())
                && v["touch"]["seconds"]
                    .as_f64()
                    .is_some_and(|x| x.is_finite() && (0.0..=60.).contains(&x))
                && v["touch"]["pointer"].as_array().is_some_and(|a| {
                    a.len() == 2
                        && a.iter().all(|x| {
                            x.as_f64()
                                .is_some_and(|x| x.is_finite() && (-1.0..=1.).contains(&x))
                        })
                }))
        && (v["mode_request"].is_null()
            || matches!(v["mode_request"].as_str(), Some("walking" | "flight")))
        && (v["aspect"].is_null()
            || v["aspect"]
                .as_f64()
                .is_some_and(|x| x.is_finite() && (0.2..=8.).contains(&x)))
}
impl State {
    fn speaker_visibility(&mut self,selection:Option<bool>,reset:bool)->bool {
        if !self.player.cheats || reset {self.show_sound_speakers=false;}
        else if let Some(selected)=selection {self.show_sound_speakers=selected;}
        self.show_sound_speakers
    }
    pub(crate) fn view(&self)->&Value {&self.view}
    pub(crate) fn observer_needs_step(&self)->bool {dot(self.player.velocity,self.player.velocity)>1e-8 || (!self.player.grounded && !self.player.flying)}
    #[cfg(test)] pub(crate) fn place_portal_test_source(&mut self,p:V){self.player.place_spawn(p,[0.,0.,-1.]);self.portal_ready=true;}
    #[cfg(test)] pub(crate) fn set_camera_test_source(&mut self,view:Value){self.view=view;}
    fn snapshot(&self) -> Value {
        json!({"position":self.player.position,"velocity":self.player.velocity,"right":self.player.right,"up":self.player.up,"forward":self.player.forward,"moving":self.event.is_some(),"medium":"planet","flying":self.player.flying,"cheats":self.player.cheats,"sound_speakers":self.show_sound_speakers,"night_vision":self.night_vision,"noclip":self.player.noclip,"time_override_minutes":self.time_override_minutes,"grounded":self.player.grounded,"submerged":self.submerged,"realm":if crate::terrain::nightworld::active(self.player.position){"nightworld"}else{"overworld"},"portal_loading":self.portal_loading})
    }
}
fn collision_free(host: &mut Host, position: V, eye: f64) -> Result<bool, String> {
    let floor = host.invoke(
        "PlanetSurfaceSystem.Height",
        json!({"position":position,"eye_height":eye}),
    )?["height"]
        .as_f64()
        .ok_or("missing player support")?;
    if dot(position, position).sqrt() < RADIUS + floor + eye - 0.02 {
        return Ok(false);
    }
    if host.system_enabled("CaveSystem") {
        let frame = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?;
        let up = normalized(position);
        for offset in [-eye + 0.26, -eye * 0.5, -0.26] {
            let point: V = std::array::from_fn(|i| position[i] + up[i] * offset);
            let sample = host.invoke(
                "CaveSystem.Sample",
                json!({"position":point,"edits":frame["edits"]}),
            )?;
            if sample["signed_distance"].as_f64().is_some_and(|d| d < 0.25) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn change_cheats(
    host: &mut Host,
    state: &mut State,
    enabled: bool,
    flight: bool,
    noclip: bool,
) -> Result<(), String> {
    if state.player.noclip
        && !(enabled && noclip)
        && !collision_free(
            host,
            state.player.position,
            state.player.settings.eye_height,
        )?
    {
        let safe =
            state
                .safe_position
                .unwrap_or([0., RADIUS + state.player.settings.eye_height, 0.]);
        state.player.position = if collision_free(host, safe, state.player.settings.eye_height)? {
            safe
        } else {
            let normal = normalized(safe);
            let height = crate::terrain::height(normal) + state.player.settings.eye_height + 4.;
            normal.map(|v| v * (RADIUS + height))
        };
    }
    state.player.set_cheats(enabled, flight, noclip);
    state.pending_jump = false;
    Ok(())
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.environment);
    let mut guarded=input.clone();
    if hook=="Step" && !state.player.cheats && input["initialize"]!=true {guarded["reset"]=json!(false);}
    let input=&guarded;
    let result = (|| {
        if hook == "Configure" {
            state.player = controls::Player::new(controls::Settings::load(host.root())?);
            if host.system_enabled("TheatreSystem") {
                if let Some(position)=host.theatre.spawn(state.player.settings.eye_height) {
                    state.player.place_spawn(position,[0.,0.,1.]);
                }
            }
            state.portal_loading=0.;state.portal_destination=None;state.portal_ready=true;
            state.safe_position = Some(state.player.position);
            state.cheat_keys = [false; 3];
            state.show_sound_speakers=false;state.night_vision=false;host.audio_visualizer.show_speakers=false;
            state.time_override_minutes=None;
            state.tool_reach = Value::Null;
            state.view_offset = [0.; 3];
            state.tool_equipped = false;
            return Ok(Value::Null);
        }
        if hook == "Abort" {
            if host.system_enabled("SilverSpiderCrabSystem") {host.invoke("SilverSpiderCrabSystem.Close",Value::Null)?;}
            if host.system_enabled("LaboratorySystem") {host.invoke("LaboratorySystem.Close",Value::Null)?;}
            if host.system_enabled("NpcSystem") {host.invoke("NpcSystem.Close",Value::Null)?;}
            let deer_close = if host.system_enabled("DeerSystem") {host.invoke("DeerSystem.Close",Value::Null).map(|_|())} else {Ok(())};
            if let Some(event) = state.scene_event.clone() {
                host.invoke(
                    "AudicleSystem.Fail",
                    json!({"audicle":event["audicle"],"reason":input["reason"]}),
                )?;
                state.scene_event = None;
            }
            if let Some(event) = state.event.clone() {
                host.invoke(
                    "AudicleSystem.Fail",
                    json!({"audicle":event["audicle"],"reason":input["reason"]}),
                )?;
                state.event = None;
            }
            let tool = host.invoke("PhysicsSystem.GameClose", Value::Null);
            let save = host.invoke("PlanetSurfaceSystem.Save", Value::Null);
            let color = host.invoke("ColorEmotionSystem.Release", Value::Null);
            state.player.pause();
            tool.and(save).and(color).and(deer_close)?;
            return Ok(Value::Null);
        }
        if hook == "Stop" || hook == "Pause" {
            host.invoke("PhysicsSystem.GamePause",Value::Null)?;
            if hook == "Pause" && host.system_enabled("WaterSystem") {
                host.invoke("WaterSystem.Pause", json!({"paused":true}))?;
            }
            #[cfg(target_os = "ios")]
            crate::ios_touch::set_tool_equipped(false);
            if let Some(event) = state.event.take() {
                host.invoke("AudicleSystem.Finish", event)?;
            }
            if let Some(event) = state.scene_event.clone() {
                host.invoke("AudicleSystem.Finish", event)?;
                state.scene_event = None;
            }
            state.player.pause();
            state.last_dig = true; // A held resume click must be released before excavation.
            if hook == "Stop" {
                if host.system_enabled("SilverSpiderCrabSystem") {host.invoke("SilverSpiderCrabSystem.Close",Value::Null)?;}
            if host.system_enabled("LaboratorySystem") {host.invoke("LaboratorySystem.Close",Value::Null)?;}
            if host.system_enabled("NpcSystem") {host.invoke("NpcSystem.Close",Value::Null)?;}
                if host.system_enabled("DeerSystem") {host.invoke("DeerSystem.Close",Value::Null)?;}
                host.invoke("PhysicsSystem.GameClose", Value::Null)?;
                host.invoke("PlanetSurfaceSystem.Save", Value::Null)?;
                host.invoke("ColorEmotionSystem.Release", Value::Null)?;
            }
            return Ok(Value::Null);
        }
        // A submitted loading card precedes the chart change. Countdown is
        // driven by accepted frame time, never a blocking sleep or audio work.
        if input["reset"]==true {state.portal_loading=0.;state.portal_destination=None;state.portal_ready=true;}
        let feet=crate::audio::length(state.player.position)-RADIUS-state.player.settings.eye_height;
        if feet> -93.8 {state.portal_ready=true;}
        if crate::terrain::nightworld::enabled() && state.portal_destination.is_none() && state.portal_ready && feet<=crate::terrain::nightworld::portal_level()+0.05 && !state.player.noclip && input["reset"]!=true {
            let p=state.player.position;let c=crate::terrain::nightworld::shaft();
            let special=(p[0]-c[0]).hypot(p[2]-c[2])<2.25;
            let cell=[(p[0]/8.).round()*8.,(p[2]/8.).round()*8.];
            let ordinary=(p[0]-cell[0]).hypot(p[2]-cell[1])<2.25;
            if special||ordinary {
                let q=[p[0],-95.+state.player.settings.eye_height,p[2]];
                state.portal_destination=Some(if crate::terrain::nightworld::active(p){crate::terrain::underground::world(q)}else{crate::terrain::nightworld::world(q)});
                state.portal_loading=0.8;state.portal_ready=false;
                state.player.pause();host.invoke("PhysicsSystem.GamePause",Value::Null)?;
            }
        }
        if let Some(destination)=state.portal_destination {
            if state.portal_loading>0. {
                if state.view.is_object() && input["defer_presentation"]!=true {
                    state.view["nightworld_loading"]=json!(state.portal_loading);
                    host.invoke("AppearanceSystem.View",state.view.clone())?;
                }
                state.portal_loading=(state.portal_loading-input["dt"].as_f64().unwrap_or(0.).min(0.05)).max(0.);
                return Ok(state.snapshot());
            }
            host.invoke("PhysicsSystem.GameClose",Value::Null)?;
            let mut forward=state.player.forward;forward[1]= -forward[1];
            state.player.place_spawn(destination,forward);
            state.safe_position=Some(destination);state.aspect=None;state.view=Value::Null;
            state.portal_destination=None;state.submerged=false;state.pending_jump=false;
            return Ok(state.snapshot());
        }
        let laboratory = if host.system_enabled("LaboratorySystem") && !crate::terrain::nightworld::active(state.player.position) {
            host.invoke("LaboratorySystem.Step",json!({"position":state.player.position,"dt":input["dt"],"eye_height":state.player.settings.eye_height,"talk":input["npc_talk"]==true}))?
        } else {Value::Null};
        let crab = if host.system_enabled("SilverSpiderCrabSystem") && host.system_enabled("CaveSystem") {
            if input["reset"]==true {host.invoke("SilverSpiderCrabSystem.Close",Value::Null)?;}
            host.invoke("SilverSpiderCrabSystem.Step",json!({"position":state.player.position,"forward":state.player.forward,"up":state.player.up,"move":input["move"],"dt":input["dt"],"immune":state.player.noclip || laboratory["locked"]==true}))?
        } else {Value::Null};
        let mut game_input=input.clone();
        if laboratory["locked"]==true {
            let p=crate::audio::vector(&laboratory["camera_position"])?;
            let focus=crate::audio::vector(&laboratory["camera_focus"])?;
            state.player.place_view(p,crate::audio::unit(crate::audio::sub(focus,p)));
            state.pending_jump=false;state.view_offset=[0.;3];state.aspect=None;
            for key in ["dig","shovel","npc_talk","sit","jump","sprint","boost","reset","park_gate"] {game_input[key]=json!(false);}
            game_input["move"]=json!([false,false,false,false,false,false]);game_input["look"]=json!([0.,0.]);
            game_input["axis"]=json!([0.,0.]);game_input["tools"]=Value::Null;game_input["water_edit"]=Value::Null;game_input["mode_request"]=Value::Null;
            game_input["tool_input"]=Value::Null;game_input["touch"]=Value::Null;game_input["cheat_keys"]=json!([false,false,false]);game_input["cheats"]=Value::Null;
        }
        if crab["locked"]==true {
            state.player.velocity=[0.;3];state.pending_jump=false;state.aspect=None;
            for key in ["dig","shovel","npc_talk","sit","jump","sprint","boost","park_gate"] {game_input[key]=json!(false);}
            game_input["move"]=json!([false,false,false,false,false,false]);game_input["look"]=json!([0.,0.]);game_input["axis"]=json!([0.,0.]);
            game_input["tools"]=Value::Null;game_input["tool_input"]=Value::Null;game_input["water_edit"]=Value::Null;game_input["touch"]=Value::Null;
        }
        if laboratory["destination"].is_array() {
            let destination=crate::audio::vector(&laboratory["destination"])?;
            let forward=crate::base_system::layer_five::laboratory_system::map_direction(state.player.forward);
            let velocity=crate::base_system::layer_five::laboratory_system::map_direction(state.player.velocity);
            state.player.place_view(destination,forward);state.player.velocity=velocity;state.safe_position=Some(destination);state.aspect=None;state.view=Value::Null;
        }
        let input=&game_input;
        let surface = host.invoke(
            "PlanetSurfaceSystem.Height",
            json!({"position":state.player.position,"eye_height":state.player.settings.eye_height}),
        )?;
        let dig = input["dig"] == true && !state.last_dig;
        state.last_dig = input["dig"] == true;
        let aspect = input["aspect"].as_f64().unwrap_or(4. / 3.);
        let rig_active = host.invoke("InverseKinematicsSystem.Active", Value::Null)? == true;
        let tool_active = true;
        let emotion_active = host.invoke("ColorEmotionSystem.Active", Value::Null)? == true;
        let keys: [bool; 3] = std::array::from_fn(|i| input["cheat_keys"][i] == true);
        let previous_cheats = state.player.cheats;
        let cheat_pressed: [bool; 3] = std::array::from_fn(|i| keys[i] && !state.cheat_keys[i]);
        state.cheat_keys = keys;
        // Shared by movement and the later water advection phase.
        let mut surf_applied=false;
        let active = input["cheats"].is_object()
            || cheat_pressed.iter().any(|v| *v)
            || tool_active
            || emotion_active
            || state.pending_jump
            || ["held", "rotate", "drop"]
                .iter()
                .any(|k| input["tool_input"][*k] == true)
            || input["tool_input"]["delta"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x.as_f64().unwrap_or(0.) != 0.))
            || input["touch"]["active"] == true
            || input["touch"]["tap"] == true
            || rig_active
            || dig
            || surface["active"] == true
            || input["reset"] == true
            || input["move"].as_array().unwrap().iter().any(|v| v == true)
            || input["look"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_f64() != Some(0.))
            || input["mode_request"].is_string()
            || state.player.active()
            || state.aspect != Some(aspect);
        if active {
            if state.event.is_none() {
                let event = host.invoke("AudicleSystem.Create", Value::Null)?;
                state.event = Some(event.clone());
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
            }
            host.invoke("AudicleSystem.Resume", state.event.clone().unwrap())?;
            if input["reset"] == true {
                if host.system_enabled("CraftingSystem"){host.invoke("CraftingSystem.Close",Value::Null)?;host.invoke("CraftingSystem.Configure",Value::Null)?;}
                host.invoke("PhysicsSystem.GameClose",Value::Null)?;
                host.invoke("PlanetSurfaceSystem.Reset", Value::Null)?;
                state.tool_reach = Value::Null;
                state.view_offset = [0.; 3];
            }
            if input["reset"] != true {
                if input["cheats"].is_object() {
                    change_cheats(
                        host,
                        &mut state,
                        input["cheats"]["enabled"] == true,
                        input["cheats"]["flight"] == true,
                        input["cheats"]["noclip"] == true,
                    )?;
                }
                for (key, pressed) in cheat_pressed.into_iter().enumerate() {
                    if pressed {
                        let enabled = if key == 0 {
                            !state.player.cheats
                        } else {
                            state.player.cheats
                        };
                        let noclip = if key == 2 && enabled {
                            !state.player.noclip
                        } else {
                            state.player.noclip && enabled
                        };
                        let flight = if key == 1 && enabled {
                            !state.player.flying
                        } else {
                            state.player.flying && enabled
                        };
                        change_cheats(
                            host,
                            &mut state,
                            enabled,
                            flight,
                            if key == 1 && !flight { false } else { noclip },
                        )?;
                    }
                }
            }
            let mut controls = input.clone();

            if state.pending_jump {
                controls["move"][4] = json!(true);
                state.pending_jump = false;
            }
            let mut previous_position = state.player.position;
            let surf_velocity=state.player.velocity;
            let old_eye=state.player.settings.eye_height;
            state
                .player
                .advance(&controls, surface["height"].as_f64().unwrap());
            // Opt-in bounded benchmark camera flight. Normal input cannot select it.
            // Noclip lets the survey cross geological walls between authored areas;
            // all regular world, water, audio, wildlife and rendering work stays active.
            if host.verify_session && host.close_after_frames().is_some()
                && std::env::var("WORMWOOD_GAME_REPLAY").as_deref()==Ok("area-soak") {
                if let Some(frame)=input["diagnostic_route_frame"].as_u64() {
                    let park=host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()).map(|p|p.offset).unwrap_or([0.,14.,-235.]);
                    let mut points:[V;12]=[
                        [0.,12.,65.],[-145.,40.,120.],[-310.,55.,65.],[-240.,85.,-100.],
                        [-150.,45.,-145.],[park[0],park[1]+58.,park[2]+30.],
                        [park[0]+45.,park[1]+15.,park[2]-15.],
                        [park[0]+22.,park[1]-81.,park[2]-45.],
                        [park[0]-22.,park[1]-82.,park[2]-60.],
                        [park[0],park[1]-78.,park[2]-28.],
                        [park[0]-45.,park[1]+8.,park[2]+25.],[-80.,35.,-90.]];
                    if std::env::var("WORMWOOD_TOWN_REVIEW").as_deref()==Ok("1") {
                        points=[[0.,70.,-190.],[125.,60.,-160.],[260.,75.,-280.],[440.,85.,-400.],
                        [710.,100.,-300.],[760.,85.,-150.],[655.,70.,70.],[580.,90.,355.],
                        [300.,75.,270.],[55.,65.,240.],[-245.,70.,230.],[-100.,70.,80.]];
                    }
                    let tick=frame.saturating_sub(1)%1200;let segment=(tick/100)as usize;
                    let t=(tick%100)as f64/100.;let a=points[segment];let b=points[(segment+1)%12];
                    let local=std::array::from_fn(|i|a[i]+(b[i]-a[i])*t);
                    let position=crate::audio::add(host.theatre.origin,local);
                    let forward=normalized(crate::audio::sub(b,a));
                    let (position,forward)=if std::env::var("WORMWOOD_GAME_REVIEW_VIEWS").as_deref()==Ok("1") {
                        // Fixed noon inspection views in real open chambers,
                        // separate from the noclip traversal between regions.
                        let venue=|p|crate::audio::add(host.theatre.origin,p);
                        let cave=crate::terrain::underground::world;
                        let park_point=|p|venue(crate::audio::add(park,p));
                        let views=[
                            (venue([-285.,75.,-130.]),venue([-240.,23.,87.])),
                            (venue([-110.,110.,-110.]),venue([0.,40.,-247.])),
                            (cave([-80.,-9.,-14.]),cave([-48.,-15.,-5.])),
                            (cave([-51.,-32.,16.]),cave([-60.,-50.,30.])),
                            (cave([-75.,-45.,35.]),cave([-51.,-44.,16.])),
                            (park_point([-25.,-83.,-52.]),park_point([20.,-87.,-55.])),
                        ];
                        let (eye,target)=views[((frame.saturating_sub(1)/240)as usize)%views.len()];
                        (eye,normalized(crate::audio::sub(target,eye)))
                    }else{(position,forward)};
                    state.player.place_spawn(position,forward);
                    state.player.cheats=true;state.player.flying=true;state.player.noclip=true;
                    previous_position=position;
                }
            }
            // Posture changes keep the feet planted; collision starts at the
            // adjusted eye instead of sweeping a standing capsule downward.
            previous_position=std::array::from_fn(|i|previous_position[i]+state.player.up[i]*(state.player.settings.eye_height-old_eye));
            if input["reset"]==true && host.system_enabled("TheatreSystem") {
                if let Some(position)=host.theatre.spawn(state.player.settings.eye_height) {
                    state.player.place_spawn(position,[0.,0.,1.]);
                    // Reset is a teleport; collision must not sweep the old
                    // location through the venue on the way to the console.
                    previous_position = state.player.position;
                }
            }
            if !state.player.cheats || input["reset"]==true {state.night_vision=false;}
            else if let Some(enabled)=input["cheats"]["night_vision"].as_bool(){state.night_vision=enabled;}
            host.audio_visualizer.show_speakers=state.speaker_visibility(input["cheats"]["sound_speakers"].as_bool(),input["reset"]==true);
            if !state.player.cheats || input["reset"]==true {
                state.time_override_minutes=None;
            } else if let Some(minutes)=input["time_override_minutes"].as_i64() {
                state.time_override_minutes=if (0..1440).contains(&minutes){Some(minutes as u16)}else{None};
            }
            if host.verify_session && host.close_after_frames().is_some()
                && std::env::var("WORMWOOD_GAME_REPLAY").as_deref()==Ok("area-soak")
                && (std::env::var("WORMWOOD_GAME_REVIEW_VIEWS").as_deref()==Ok("1")||std::env::var("WORMWOOD_TOWN_REVIEW").as_deref()==Ok("1")) {
                state.time_override_minutes=Some(720);
            }
            let floor = host.invoke(
                "PlanetSurfaceSystem.Height",
                json!({"position":state.player.position,"eye_height":state.player.settings.eye_height}),
            )?["height"]
                .as_f64()
                .unwrap();
            if !state.player.noclip {
                state.player.resolve_surface(floor);
            }
            if !state.player.noclip && host.system_enabled("CaveSystem") && input["reset"] != true {
                let cave_frame = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?;
                let body=host.invoke("CaveSystem.ResolveBody",json!({"previous":previous_position,"position":state.player.position,"eye_height":state.player.settings.eye_height,"edits":cave_frame["edits"]}))?;
                state.player.position = serde_json::from_value(body["position"].clone())
                    .map_err(|e| format!("cave body position: {e}"))?;
                for row in body["normals"]
                    .as_array()
                    .ok_or("cave contact normals required")?
                {
                    let normal: V = serde_json::from_value(row.clone())
                        .map_err(|e| format!("cave contact normal: {e}"))?;
                    state.player.resolve_volume_contact(normal);
                }
            }
            if !state.player.noclip {
                let material=host.invoke("PhysicsSystem.GameResolveBody",json!({"previous":previous_position,"position":state.player.position,"eye_height":state.player.settings.eye_height}))?;
                state.player.position=serde_json::from_value(material["position"].clone()).map_err(|e|format!("material body position: {e}"))?;
                for normal in material["normals"].as_array().unwrap(){state.player.resolve_volume_contact(serde_json::from_value(normal.clone()).unwrap());}
            }
            if !state.player.noclip {
                state.safe_position = Some(state.player.position);
            }

            if !state.player.noclip && host.system_enabled("TheatreSystem") {
                let body=host.invoke("TheatreSystem.ResolveBody",json!({"previous":previous_position,"position":state.player.position,"eye_height":state.player.settings.eye_height}))?;
                state.player.position=serde_json::from_value(body["position"].clone()).map_err(|_|"theatre body position required")?;
                for normal in body["normals"].as_array().ok_or("theatre contact normals required")? {
                    state.player.resolve_volume_contact(serde_json::from_value(normal.clone()).map_err(|_|"theatre contact normal required")?);
                }
                state.safe_position=Some(state.player.position);
            }

            if state.player.sitting && !state.player.noclip && !state.player.flying && input["reset"]!=true {
                if let Some(park)=host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()) {
                    let local=crate::audio::sub(previous_position,host.theatre.origin);
                    let feet=crate::audio::add(local,[0.,-0.45,0.]);
                    let (film,current)=park.film_sample(&host.theatre.flow,feet);
                    let (parcels,parcel_current,_)=host.water.soft_body.sample(crate::audio::add(feet,host.theatre.origin),1.2);
                    let current=if parcels {parcel_current}else{current};
                    let forward=(input["move"][0]==true) as i32-(input["move"][1]==true) as i32;
                    let side=(input["move"][3]==true) as i32-(input["move"][2]==true) as i32;
                    let f=input["axis"][1].as_f64().unwrap_or(forward as f64);let side=input["axis"][0].as_f64().unwrap_or(side as f64);
                    let forward=crate::audio::unit([state.player.forward[0],0.,state.player.forward[2]]);
                    let direction:V=std::array::from_fn(|i|forward[i]*f+state.player.right[i]*side);
                    let steering=crate::audio::mul(direction,3./crate::audio::length(direction).max(1.));
                    let mut velocity=surf_velocity;
                    if input["move"][4]==true && state.player.velocity[1]>velocity[1]+2. {velocity[1]=state.player.velocity[1];}
                    if let Some((position,velocity,grounded))=park.surf(local,velocity,steering,input["dt"].as_f64().unwrap_or(0.).min(0.05),current,film||parcels) {
                        surf_applied=true;
                        state.player.accept_surf(crate::audio::add(position,host.theatre.origin),velocity,grounded);
                        state.safe_position=Some(state.player.position);
                    }
                }
            }
            if crab["locked"]==true {state.player.position=previous_position;state.player.velocity=[0.;3];}
            let terrain=host.invoke("PlanetSurfaceSystem.Frame",Value::Null)?;
            let down=(-dot(state.player.forward,state.player.up)-0.20).max(0.)/0.65;
            let wanted=if state.player.grounded && !state.player.flying {down.clamp(0.,1.)}else{0.};
            state.work_bend+=(wanted-state.work_bend)*(1.-(-10.*input["dt"].as_f64().unwrap_or(0.)).exp());
            let posture=json!({"bend_radians":if state.player.sitting{0.25}else{0.68*state.work_bend},"lowering_metres":if state.player.sitting{0.1}else{0.16*state.work_bend},"forward":state.player.forward});
            if !state.player.noclip {
                if let Some(site)=&host.theatre.nightworld {
                    let (position,normals)=site.resolve(previous_position,state.player.position,state.player.settings.eye_height);
                    state.player.position=position;
                    for normal in normals {state.player.resolve_volume_contact(normal);}
                    state.safe_position=Some(position);
                }
            }
            let rig_input=json!({"position":state.player.position,"velocity":state.player.velocity,"forward":state.player.forward,"move":input["move"],"eye_height":state.player.settings.eye_height,"sitting":state.player.sitting,"grounded":state.player.grounded,"flying":state.player.flying,"dt":input["dt"],"reset":input["reset"],"tool_posture":posture});
            let plan=host.invoke("PlayerRigSystem.Plan",rig_input.clone())?;
            let planned_eye:V=std::array::from_fn(|i|state.player.position[i]+plan["view_offset"][i].as_f64().unwrap());
            let mut phone_grip=if host.system_enabled("PhoneSystem") {
                host.invoke("PhoneSystem.Grip",json!({"position":planned_eye,"forward":state.player.forward,"up":state.player.up}))?
            }else{Value::Null};
            if phone_grip["active"]!=true && host.system_enabled("GlassesSystem") {phone_grip=host.invoke("GlassesSystem.Grip",json!({"position":planned_eye,"forward":state.player.forward,"up":state.player.up}))?;}
            if phone_grip["active"]!=true && host.system_enabled("CraftingSystem"){phone_grip=host.invoke("CraftingSystem.Grip",json!({"position":planned_eye,"forward":state.player.forward,"up":state.player.up}))?;}
            let mut tool=host.invoke("PhysicsSystem.GameStep",json!({"position":planned_eye,"aim":state.player.forward,"up":state.player.up,"dt":input["dt"],"terrain":terrain,"tools":input["tools"],"tool_reach":plan["tool_reach"],"tools_suspended":phone_grip["active"]==true}))?;
            if phone_grip["active"] == true {
                tool["tool_grip"]=phone_grip;
            }
            tool["equipped"]=json!(true);tool["instances"]=json!([]);tool["color_mode"]=json!("dig");tool["trigger"]=json!(false);tool["interaction_handled"]=json!(true);
            if input["defer_presentation"]!=true && (tool["caption_changed"]==true || previous_cheats != state.player.cheats) {host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":input["entity"],"text":tool["caption"].as_str().unwrap_or("Mirror"),"hud":state.player.cheats}))?;}
            tool["work_feedback"]=json!(if tool["active"]==true{3}else{1});
            state.tool_equipped = tool["equipped"] == true;
            #[cfg(target_os = "ios")]
            {
                crate::ios_touch::set_tool_equipped(state.tool_equipped);

                crate::ios_touch::ack_cheats(
                    input["cheats"]["sequence"].as_u64(),
                    state.player.cheats,
                    state.player.flying,
                    state.player.noclip,
                );
            }
            if input["touch"]["tap"] == true && tool["interaction_handled"] != true {
                state.pending_jump = true;
            }
            let emotion=host.invoke("ColorEmotionSystem.Advance",json!({"dt":input["dt"],"charge":tool["charge"],"mode":tool["color_mode"],"trigger":tool["trigger"]}))?;
            let mut rig_input=rig_input;
            rig_input["tool_grip"]=tool["tool_grip"].clone();
            let body = host.invoke("PlayerRigSystem.Advance",rig_input)?;
            state.tool_reach = body["tool_reach"].clone();
            state.view_offset = std::array::from_fn(|i| body["view_offset"][i].as_f64().unwrap());
            let eye: V = std::array::from_fn(|i| {
                state.player.position[i] + body["view_offset"][i].as_f64().unwrap()
            });
            let mut material = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?;
            let mut physical=host.material_vertices(eye,state.player.forward,state.player.up,aspect)?;
            for rows in [&material["debris"],&tool["vertices"]] {
                for row in rows.as_array().into_iter().flatten() {
                    physical.push(std::array::from_fn(|k|row[k].as_f64().unwrap()));
                }
            }
            // Move the already-owned envelope; json! serializes Value inputs
            // again, recursively copying the unchanged cave profile each tick.
            let mut procedural=serde_json::Map::new();
            procedural.insert("edits".into(),std::mem::take(&mut material["edits"]));
            procedural.insert("debris".into(),Value::Array(Vec::new()));
            procedural.insert("underground".into(),std::mem::take(&mut material["underground"]));
            let procedural=Value::Object(procedural);
            host.update_material_entity(input["entity"].as_u64().unwrap(),procedural,physical)?;
            state.aspect = Some(aspect);
            let paused_scene=if input["defer_presentation"]==true{Some(state.view.clone())}else{None};
            state.view = json!({"body":body["entity"],"entity":input["entity"],"position":eye,"right":state.player.right,"up":state.player.up,"forward":state.player.forward,"yaw":0.,"pitch":0.,"aspect":aspect,"capture":input["capture"]==true,"instances":tool["instances"],"emotion":emotion["entity"]});
            if let Some(previous)=paused_scene {
                for key in ["time","time_override_minutes","water_instances","diagnostic_cave_fullbright","instances"] {
                    if let Some(value)=previous.get(key){state.view[key]=value.clone();}
                }
            }

            host.emit(
                json!({"event":"environment_step","state":state.snapshot(),"reset":input["reset"]}),
            )?;
        } else if let Some(event) = state.event.take() {
            host.invoke("AudicleSystem.Finish", event)?;
        }
        // Deferred movement belongs to the paused DAW observer. It must not
        // tick world animation or invalidate the retained world on idle frames.
        if state.view.is_object() && input["defer_presentation"]!=true {
            // Screenshot readback is an explicit diagnostic, never an implicit
            // consequence of editing terrain or resetting the player.
            state.view["capture"]=json!(input["capture"]==true);
            if state.scene_event.is_none() {
                let event = host.invoke("AudicleSystem.Create", Value::Null)?;
                state.scene_event = Some(event.clone());
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
            }
            host.invoke("AudicleSystem.Resume", state.scene_event.clone().unwrap())?;
            state.scene_time += input["dt"].as_f64().unwrap_or(0.);
            state.view["time"] = json!(state.scene_time);
            state.view["time_override_minutes"]=json!(state.time_override_minutes);
            // Explicit cheat inspection; the existing automated screenshot selection is retained.
            state.view["diagnostic_cave_fullbright"]=json!((state.player.cheats && state.night_vision) || host.verify_session && host.close_after_frames().is_some()
                && state.player.cheats && crate::terrain::underground::local(state.player.position)[1]<0.
                && std::env::var("WORMWOOD_GAME_REPLAY").as_deref()==Ok("area-soak")
                && std::env::var("WORMWOOD_GAME_REVIEW_VIEWS").as_deref()==Ok("1")
                && std::env::var("WORMWOOD_GAME_REVIEW_FULLBRIGHT").as_deref()==Ok("1"));
            if host.system_enabled("WaterSystem") {
                let status = host.invoke("WaterSystem.Frame", Value::Null)?;
                if status["configured"] != true {
                    host.invoke("WaterSystem.Configure", Value::Null)?;
                } else if status["paused"] == true {
                    host.invoke("WaterSystem.Pause", json!({"paused":false}))?;
                }
                if input["water_edit"].is_object() && input["water_edit"] != state.last_water_edit {
                    let mut edit = input["water_edit"].clone();
                    edit["position"] = state.view["position"].clone();
                    edit["direction"] = json!(state.player.forward);
                    edit["forward"] = json!(state.player.forward);
                    host.invoke("WaterSystem.Edit", edit)?;
                }
                state.last_water_edit = input["water_edit"].clone();
                let water_view=json!({"position":state.view["position"],"right":state.player.right,"up":state.player.up,"forward":state.player.forward,"aspect":state.aspect});
                let water=host.invoke("WaterSystem.Step",json!({"position":state.player.position,"forward":state.player.forward,"view":water_view,"dt":input["dt"],"park_gate":input["park_gate"]}))?;
                state.view["water_instances"] = water["instances"].clone();
                let feet: V = std::array::from_fn(|i| {
                    state.player.position[i]
                        - state.player.up[i] * (state.player.settings.eye_height - 0.15)
                });
                let wet = host.invoke("WaterSystem.Sample", json!({"position":feet}))?;
                state.submerged = wet["occupied"] == true;
                if state.submerged && !state.player.flying {
                    if crab["locked"]!=true && !surf_applied && let Ok(current) = serde_json::from_value::<V>(wet["current"].clone()) {
                        state.player.advect(
                            current.map(|v| v * wet["strength"].as_f64().unwrap_or(1.)),
                            input["dt"].as_f64().unwrap_or(0.),
                        );
                    }
                    if state.scene_time >= state.next_water_disturbance
                        && dot(state.player.velocity, state.player.velocity) > 0.01
                    {
                        host.invoke("WaterSystem.Disturb",json!({"position":feet,"impulse":state.player.velocity.map(|v|v*0.1),"strength":dot(state.player.velocity,state.player.velocity).sqrt().min(4.)*0.1}))?;
                        state.next_water_disturbance = state.scene_time + 0.25;
                    }
                }
            } else {
                state.view["water_instances"] = json!([]);
                state.submerged = false;
            }

            // Wildlife advances with the game scene even when audio is disabled.
            // Replace our prior snapshot on stationary frames rather than appending it.
            let mut wildlife_instances=state.view["instances"].as_array().cloned().unwrap_or_default();
            wildlife_instances.retain(|i|i["crab_pose"].is_null() && i["deer_pose"].is_null() && i["npc"]!=true && i["laboratory"]!=true);
            if !crate::terrain::nightworld::active(state.player.position) && host.system_enabled("DeerSystem") {
                let deer=host.invoke("DeerSystem.Step",json!({"position":state.player.position,"dt":input["dt"]}))?;
                wildlife_instances.extend(deer["instances"].as_array().ok_or("deer instances required")?.iter().cloned());
            }
            if let Some(instances)=crab["instances"].as_array(){wildlife_instances.extend(instances.iter().cloned());}
            let mut npc_caption=None;
            if host.system_enabled("NpcSystem") {
                let npc=host.invoke("NpcSystem.Step",json!({"position":state.player.position,"forward":state.player.forward,"dt":input["dt"],"talk":input["npc_talk"]==true}))?;
                if !crate::terrain::nightworld::active(state.player.position) {
                    wildlife_instances.extend(npc["instances"].as_array().ok_or("NPC instances required")?.iter().cloned());
                    npc_caption=npc["caption"].as_str().map(str::to_owned);
                }
            }
            if !laboratory.is_null() {
                if let Some(site)=host.theatre.venue.as_ref().and_then(|v|v.laboratory.as_ref()) {
                    if let Some(id)=laboratory["researcher"].as_u64() {
                        wildlife_instances.push(json!({"entity":id,"origin":crate::audio::add(host.theatre.origin,site.researcher()),"laboratory":true}));
                    }
                    if let Some(id)=laboratory["prismasqueek"].as_u64() {
                        wildlife_instances.push(json!({"entity":id,"origin":crate::audio::add(host.theatre.origin,crate::audio::vector(&laboratory["mouse"])?),"laboratory":true,"prismasqueek":true,"yaw":laboratory["yaw"],"gait":laboratory["gait"]}));
                    }
                }
                if let Some(caption)=laboratory["caption"].as_str() {npc_caption=Some(format!("NPC | {caption}"));}
            }
            state.view["laboratory_portals"]=laboratory["portal_open"].clone();
            if let Some(caption)=crab["caption"].as_str(){npc_caption=Some(caption.to_owned());}
            if npc_caption.is_some() || state.npc_caption.is_some() {
                let caption=npc_caption.as_deref().unwrap_or("Mirror | Esc pause");
                host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":input["entity"],"text":caption,"hud":npc_caption.is_some()}))?;
            }
            state.npc_caption=npc_caption;
            state.view["instances"]=json!(wildlife_instances);

            if host.system_enabled("CraftingSystem"){host.invoke("CraftingSystem.Frame",json!({"view":state.view,"dt":input["dt"]}))?;}
            if host.system_enabled("PhoneSystem") {
                state.view["phone"] = host.invoke("PhoneSystem.Frame",json!({"view":state.view,"dt":input["dt"]}))?;
            } else {
                state.view["phone"] = Value::Null;
            }

            if !host.system_enabled("EnvironmentAudioSystem") && input["defer_presentation"]!=true {
                host.invoke("AppearanceSystem.View", state.view.clone())?;
            }
        }
        if host.system_enabled("EnvironmentAudioSystem") && state.view.is_object() && input["defer_presentation"]!=true {
            if host.invoke("EnvironmentAudioSystem.Status", Value::Null)?["active"] != true {
                host.invoke("EnvironmentAudioSystem.Begin", Value::Null)?;
            }
            // Continuous sound/appearance belongs to its own event, including
            // stationary frames after the movement event has finished.
            host.invoke("EnvironmentAudioSystem.Frame",json!({"position":state.view["position"],"forward":state.player.forward,"up":state.player.up,"dt":input["dt"],"view":if input["defer_presentation"]==true{Value::Null}else{state.view.clone()}}))?;
            state.view["capture"] = json!(false);
        }
        Ok(state.snapshot())
    })();
    let result = match result {
        Err(error) => {
            if let Some(event) = state.scene_event.clone() {
                if host
                    .invoke(
                        "AudicleSystem.Fail",
                        json!({"audicle":event["audicle"],"reason":error}),
                    )
                    .is_ok()
                {
                    state.scene_event = None;
                }
            }
            let failure = if let Some(event) = state.event.clone() {
                host.invoke(
                    "AudicleSystem.Fail",
                    json!({"audicle":event["audicle"],"reason":error}),
                )
                .map(|_| {
                    state.event = None;
                })
            } else {
                Ok(())
            };
            Err(match failure {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; event failure recording: {cleanup}"),
            })
        }
        result => result,
    };
    host.environment = state;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Value {
        json!({"entity":1,"dt":0.05,"move":[false,false,false,false,false,false],"look":[0,0],"reset":false,"boost":false})
    }
    #[test]
    fn rotation_preserves_an_orthonormal_personal_frame() {
        let mut s = State::default();
        let mut i = input();
        i["look"] = json!([2., -3.]);
        for _ in 0..400 {
            s.player.advance(&i, 0.);
        }
        assert!(dot(s.player.right, s.player.up).abs() < 1e-10);
        assert!((dot(s.player.up, s.player.up) - 1.).abs() < 1e-10);
        assert!(s.player.forward[1].abs() <= 89f64.to_radians().sin() + 1e-10);
    }
    #[test]
    fn movement_obeys_the_environment_boundary() {
        let mut s = State::default();
        let mut i = input();
        i["move"][5] = json!(true);
        for _ in 0..100 {
            s.player.advance(&i, 0.);
        }
        // Flight descent respects the current surface.
        assert!(dot(s.player.position, s.player.position).sqrt() >= RADIUS + 1.999);
    }
}

#[cfg(test)] mod sound_speaker_cheat_tests {
    use super::*;
    #[test] fn speaker_option_requires_cheats_persists_until_disabled_and_clears_on_reset() {
        let mut state=State::default();
        assert!(!state.speaker_visibility(Some(true),false));
        state.player.set_cheats(true,false,false);
        assert!(state.speaker_visibility(Some(true),false));
        assert!(state.speaker_visibility(None,false));
        assert!(!state.speaker_visibility(Some(false),false));
        assert!(state.speaker_visibility(Some(true),false));
        state.player.set_cheats(false,false,false);
        assert!(!state.speaker_visibility(None,false));
        state.player.set_cheats(true,false,false);
        assert!(!state.speaker_visibility(None,false));
        assert!(state.speaker_visibility(Some(true),false));
        assert!(!state.speaker_visibility(None,true));
    }
}
