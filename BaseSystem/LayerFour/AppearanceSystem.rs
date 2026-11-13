//! Resolves Entity appearance data before crossing the graphics connection.
#[path = "EnvironmentGeometry.rs"]
mod environment_geometry;
#[path="TerrainProjectionWorker.rs"]
mod terrain_worker;
#[path = "Projection.rs"]
pub(crate) mod projection;
use crate::host::Host;
use serde_json::Value;
use std::{borrow::Cow, cell::RefCell};
thread_local! { static SCENE_BUFFER: RefCell<Vec<[f32;24]>> = const { RefCell::new(Vec::new()) }; }
pub(crate) const MAX_SCENE_INSTANCES: usize = 14;

#[path = "TheatreAppearance.rs"]
mod theatre_appearance;
pub(crate) fn theatre_shared_mesh(origin:[f64;3],venue:&std::sync::Arc<crate::theatre::Venue>,input:&Value)->Result<Vec<[f32;24]>,String> {
    theatre_appearance::project_shared(origin,venue,input)
}
pub(crate) fn theatre_mesh(origin:[f64;3],vertices:&[[f32;7]],input:&Value)->Result<Vec<[f32;24]>,String> {
    theatre_appearance::project(origin, vertices, input)
}

#[cfg(test)]
pub(crate) fn benchmark_planet(view:&Value,surface:&Value,material:&[[f64;6]])->Result<Vec<[f32;24]>,String> {
    environment_geometry::planet(view,surface,Some(material))
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "View" => {
            let portal_loading=input["nightworld_loading"].as_f64().is_some_and(|t|t>0.);
            if portal_loading {
                if let Some(mut worker)=host.scene_preparation.take(){let result=worker.poll(host,true);host.scene_preparation=Some(worker);result?;}
            }
            if !portal_loading && cfg!(target_os="macos") && !cfg!(test) && host.backend()=="WebGPU" && input["security_camera"]!=true
                && std::env::var("WORMWOOD_GAME_SCENE_SYNC").as_deref()!=Ok("1") {
                return submit_game_scene(host,input);
            }
            let scene = scene(host,input)?;
            let mut request=serde_json::json!({"entity":input["entity"],"capture":input["capture"]==true});
            request["view"]=scene.view;
            let result=host.set_scene_view_water(&request,&scene.triangles,scene.vegetation,&scene.water);
            SCENE_BUFFER.with(|buffer| {let mut triangles=scene.triangles;triangles.clear();*buffer.borrow_mut()=triangles;});
            result
        }

        "Prepare" => {
            let entity = host.invoke("EntitySystem.Get", input.clone())?;
            host.invoke("GraphicsConnectionSystem.Prepare", entity)
        }
        "Present" => {
            host.entity_shared_snapshot(input["entity"].as_u64().ok_or("Entity ID required")?)?;
            host.invoke("GraphicsConnectionSystem.Present", input.clone())
        }
        "Pump" => {
            poll_game_scene(host,input["entity"].as_u64())?;
            host.entity_shared_snapshot(input["entity"].as_u64().ok_or("Entity ID required")?)?;
            host.invoke("GraphicsConnectionSystem.Pump", input.clone())
        }
        "Release" => {
            if host.scene_preparation.as_ref().is_some_and(|w|Some(w.entity)==input["entity"].as_u64()){host.scene_preparation=None;}
            terrain_worker::close();host.invoke("GraphicsConnectionSystem.Release", input.clone())},
        _ => Err("unknown appearance hook".into()),
    }
}

/// The complete game scene, shared by player and workstation camera views.
pub(crate) struct Scene {
    pub view: Value,
    pub triangles: Vec<[f32;24]>,
    pub vegetation: Option<crate::vegetation::packet::Packet>,
    pub water: Vec<crate::water::presentation::Packet>,
}
pub(crate) fn scene(host:&mut Host,input:&Value)->Result<Scene,String>{
    // Lighting rehearsal uses the actual venue and shared native materials,
    // without rebuilding outdoor terrain, water, reflection or vegetation passes.
    if input["stage_rehearsal"]==true {
        let (origin,venue,chair)=host.theatre_geometry(input)?.ok_or("stage rehearsal requires the theatre")?;
        let mut view=input.clone();view["table_eye"]=serde_json::json!(crate::audio::sub(crate::audio::vector(&input["position"])?,origin));
        view["retain_offscreen_reflections"]=serde_json::json!(false);
        let mut triangles=theatre_appearance::project_shared(origin,&venue,&view)?;
        triangles.extend(theatre_appearance::project_chair(origin,&chair.vertices,&view)?);
        return Ok(Scene{view,triangles,vegetation:None,water:Vec::new()});
    }
    prepare(host,input)?.build()
}

struct Prepared {
    view:Value,
    entities:std::collections::HashMap<u64,std::sync::Arc<crate::base_entity::Entity>>,
    enabled:std::collections::HashSet<&'static str>,
    plants:Option<crate::vegetation::State>,
    venue:crate::base_system::layer_two::theatre_system::Mesh,
    nightworld:Option<std::sync::Arc<crate::theatre::nightworld::Site>>,
    monitor:bool,
    phone:Vec<[f32;24]>,rays:Vec<[f32;24]>,
    geometry:Option<std::sync::Arc<crate::terrain::material_geometry::Surface>>,
    security:Option<(Value,std::sync::Arc<Vec<[f64;6]>>)> ,
    reflections:bool,
    recycle:Option<Vec<[f32;24]>>,
}
impl Prepared {
 fn system_enabled(&self,name:&str)->bool{self.enabled.contains(name)}
 fn entity(&self,id:u64)->Result<std::sync::Arc<crate::base_entity::Entity>,String>{self.entities.get(&id).cloned().ok_or_else(||format!("scene Entity {id} missing"))}
}
fn prepare(host:&mut Host,input:&Value)->Result<Prepared,String>{
 if input["instances"].as_array().map_or(0,Vec::len)>MAX_SCENE_INSTANCES || input["water_instances"].as_array().map_or(0,Vec::len)>16 {
  return Err("scene appearance instance budget exceeded".into());
 }
 let mut view=if input["security_camera"]==true&&input["day_night"]["fraction"].as_f64().is_some_and(f64::is_finite){input.clone()}else{super::atmosphere::Lighting::enrich(input)};
 let mut enabled=std::collections::HashSet::new();
 for name in ["VegetationSystem","TheatreSystem","CloudSystem","PhoneRenderSystem","GlassesRenderSystem","CraftingRenderSystem","MonasteryRenderSystem","AudioRayVisualizerSystem"] {if host.system_enabled(name){enabled.insert(name);}}
 let mut entities=std::collections::HashMap::new();
 let mut ids=vec![input["entity"].as_u64().ok_or("scene Entity missing")?];
 for key in ["body","emotion"]{if let Some(id)=input[key].as_u64(){ids.push(id);}}
 for key in ["instances","water_instances"]{for item in input[key].as_array().into_iter().flatten(){ids.push(item["entity"].as_u64().ok_or("instance Entity missing")?);}}
 for id in ids{if !entities.contains_key(&id){entities.insert(id,host.entity_shared_snapshot(id)?);}}
 let shader=entities[&input["entity"].as_u64().unwrap()].shader().unwrap_or("");
 let planet=shader=="planet_surface";
 if planet&&enabled.contains("TheatreSystem")&&host.system_enabled("EnvironmentAudioSystem"){
 // The visual consumer samples the audible cursor, never queued render-ahead.
 let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
 let transport=&audio["transport"];
 let seconds=transport["position_frame"].as_u64().zip(transport["sample_rate"].as_u64().filter(|r|*r>0)).map_or(0.,|(f,r)|f as f64/r as f64);
 view["stage_lighting"]=host.invoke("WorkstationDataSystem.StageLighting",serde_json::json!({"seconds":seconds}))?["levels"].clone();
 }

 if planet&&enabled.contains("CloudSystem"){view["clouds"]=host.invoke("CloudSystem.Frame",view.clone())?;}else{view["clouds"]=Value::Null;}
 let deep=planet&&crate::terrain::underground::deep_view(crate::audio::vector(&view["position"])?);
 if crate::terrain::nightworld::active(crate::audio::vector(&view["position"])?) {enabled.remove("VegetationSystem");}
 let plants=if planet&&!deep&&enabled.contains("VegetationSystem"){Some(host.vegetation_scene_snapshot()?)}else{None};
 let venue=if planet&&enabled.contains("TheatreSystem"){host.theatre_geometry(&view)?}else{None};
 let mut phone=if planet&&enabled.contains("PhoneRenderSystem"){host.phone_geometry(&view)?}else{Vec::new()};
 if planet&&enabled.contains("GlassesRenderSystem"){phone.extend(host.glasses_geometry(&view)?);}
 if planet&&enabled.contains("PlayerRigSystem"){phone.extend(super::player_rig_system::tube_frame(host,&view)?);}
 if planet&&enabled.contains("CraftingRenderSystem"){phone.extend(host.crafting_geometry(&view)?);}
 if planet&&enabled.contains("MonasteryRenderSystem"){phone.extend(host.monastery_geometry(&view)?);}
 let rays=if planet&&enabled.contains("AudioRayVisualizerSystem"){host.audio_ray_geometry(&view)?}else{Vec::new()};
 let security=if planet&&input["security_camera"]==true{
  let eye=crate::audio::vector(&input["position"])?;let yaw=input["yaw"].as_f64().unwrap();let pitch=input["pitch"].as_f64().unwrap();
  let up=[-yaw.sin()*pitch.sin(),pitch.cos(),yaw.cos()*pitch.sin()];
  let frame=host.invoke("PlanetSurfaceSystem.Frame",Value::Null)?;
  let mut physical=host.material_vertices(eye,crate::audio::vector(&input["forward"])?,up,input["aspect"].as_f64().ok_or("aspect required")?)?;
  for row in frame["debris"].as_array().into_iter().flatten(){physical.push(std::array::from_fn(|k|row[k].as_f64().unwrap()));}
  Some((serde_json::json!({"edits":frame["edits"],"debris":[],"underground":frame["underground"]}),std::sync::Arc::new(physical)))
 }else{None};
 let reflections=host.water.presentation.planar_reflections()&&entities.values().any(|e|{
  if let Some(packet)=&e.water_packet{packet.optics["planar_reflection"]==true}
  else{e.shader()==Some("water_surface")&&e.visual_procedural().is_some_and(|p|p["optics"]["planar_reflection"]==true)}
 });
 Ok(Prepared{view,entities,enabled,plants,venue,nightworld:host.theatre.nightworld.clone(),monitor:host.theatre.monitor_image.is_some(),phone,rays,geometry:host.ray_audio.terrain_geometry.clone(),security,
  reflections,recycle:None})
}
impl Prepared {
 fn build(mut self)->Result<Scene,String>{
            if let Some(recycle)=self.recycle.take(){SCENE_BUFFER.with(|buffer|*buffer.borrow_mut()=recycle);}
            let prepared=&self;let input=&prepared.view;
            if input["nightworld_loading"].as_f64().is_some_and(|t|t>0.) {
                return Ok(Scene{view:input.clone(),triangles:nightworld_loading(input),vegetation:None,water:Vec::new()});
            }
            let lighting = super::atmosphere::Lighting::from_view(input);
            let data = prepared.entity(input["entity"].as_u64().ok_or("Entity ID required")?)?;
            let mut material = data.material_vertices.clone();
            let crate::base_entity::Appearance::Visual { vertices, shader, procedural } = &data.appearance else {
                return Err("view requires a visual".into());
            };
            let mut procedural=Cow::Borrowed(procedural);
            if let Some((surface,physical))=&prepared.security {procedural=Cow::Borrowed(surface);material=Some(physical.clone());}

            let deep_cave=shader=="planet_surface" && crate::terrain::underground::deep_view(crate::audio::vector(&input["position"])?);
            // Dispatch before vegetation, venue lighting, rigs and overlays.
            // Their Host reads stay on the owning thread; the worker sees only
            // immutable accepted data and is joined for this exact camera View.
            let mut terrain_view=input.clone();
            terrain_view["detailed_vegetation"]=serde_json::json!(shader=="planet_surface" && !deep_cave && prepared.system_enabled("VegetationSystem"));
            terrain_view["scene_sky_first"]=serde_json::json!(!(input["phone"]["equipped"]==true && input["phone"]["flashlight_on"]==true));
            let terrain_job=if shader=="planet_surface" && terrain_worker::enabled(){
                let recycle=SCENE_BUFFER.with(|buffer|std::mem::take(&mut *buffer.borrow_mut()));
                Some(terrain_worker::begin(terrain_view,procedural.as_ref().clone(),material.clone(),recycle)?)
            }else{None};
            let vegetation = if shader == "planet_surface"
                && !deep_cave && prepared.system_enabled("VegetationSystem")
            {
                let mut request = input.clone();
                // Leave room for water-plane clipping to double a reflected face.
                request["max_triangles"] = serde_json::json!(16384);
                request["max_distance"] = serde_json::json!(6000.);
                let geometry = crate::base_system::layer_two::vegetation_system::geometry_numeric(prepared.plants.as_ref().ok_or("vegetation snapshot missing")?,&request)?;
                Some(crate::vegetation::packet::Packet::from_numeric(geometry, crate::vegetation::packet::Scene::from_view(input)?)?)
            } else {
                None
            };
            let mut planet_view = input.clone();
            planet_view["detailed_vegetation"] = serde_json::json!(vegetation.is_some());
            let terrain_batch = match shader.as_str() {
                "planet_surface" if terrain_job.is_some()=>Vec::new(),
                "planet_surface" => {
                    environment_geometry::planet(&planet_view, &procedural, material.as_deref().map(Vec::as_slice))?
                }
                "material_surface" => {
                    let mut view = input.clone();
                    view["two_sided"] = serde_json::json!(true);
                    projection::project(&vertices, &view)
                }
                _ => projection::project(&vertices, input),
            };
            let sky_first=shader=="planet_surface" && !(input["phone"]["equipped"]==true && input["phone"]["flashlight_on"]==true);
            let mut triangles=SCENE_BUFFER.with(|buffer|std::mem::take(&mut *buffer.borrow_mut()));
            triangles.clear();
            triangles.reserve(if terrain_job.is_some(){4096}else if shader=="planet_surface" {crate::scene_limits::MAX_SCENE_TRIANGLES} else {terrain_batch.len()});
            if sky_first && terrain_job.is_none(){triangles.extend(lighting.sky(input));}
            triangles.extend(terrain_batch);
            let mut material_view=input.clone();
            if shader == "planet_surface" && prepared.system_enabled("TheatreSystem") {
                if let Some((origin,venue,chair))=prepared.venue.clone() {
                    material_view["table_eye"]=serde_json::json!(crate::audio::sub(crate::audio::vector(&input["position"])?,origin));
                    let mut theatre_view=input.clone();
                    theatre_view["stage_lighting"]=serde_json::json!(crate::theatre::lighting::levels(input));
                    theatre_view["retain_offscreen_reflections"]=serde_json::json!(prepared.reflections
                        && input["water_instances"].as_array().is_some_and(|a|!a.is_empty()));
                    if theatre_view["retain_offscreen_reflections"]==true {
                        let mut planes=Vec::<[[f64;3];2]>::new();
                        for item in input["water_instances"].as_array().into_iter().flatten() {
                            let e=prepared.entity(item["entity"].as_u64().ok_or("water Entity missing")?)?;
                            let packet=if let Some(p)=&e.water_packet{p.as_ref().clone()}else{
                                let crate::base_entity::Appearance::Visual{procedural,..}=&e.appearance else{continue;};
                                crate::water::presentation::Packet::parse(procedural)?
                            };
                            if packet.optics["planar_reflection"]!=true {continue;}
                            let mean=packet.optics["mean_height"].as_f64().ok_or("reflection height")?;
                            let plane=[packet.frame.up,std::array::from_fn(|i|packet.frame.origin[i]+packet.frame.up[i]*mean-origin[i])];
                            if !planes.contains(&plane){planes.push(plane);}
                        }
                        if !planes.is_empty(){theatre_view["reflection_planes"]=serde_json::json!(planes);}else{theatre_view["retain_offscreen_reflections"]=serde_json::json!(false);}
                    }
                    triangles.extend(theatre_appearance::project_shared(origin,&venue,&theatre_view)?);
                    triangles.extend(theatre_appearance::project_chair(origin,&chair.vertices,&theatre_view)?);
                    if prepared.monitor {
                        let [cx,cy,cz] = venue.computer_position;
                        let eye = crate::audio::vector(&input["position"])?;
                        let mut view = input.clone();
                        view["position"] = serde_json::json!(crate::audio::sub(eye,origin));
                        view["far"] = serde_json::json!(crate::audio::length(eye)+2.*crate::terrain::RADIUS);
                        view["two_sided"] = serde_json::json!(true);
                        // make_theatre.py: glass center y=.175, front z=.058.
                        // Place the image 1mm in front of it; yaw 180 faces -Z.
                        let corners = [[0.,0.],[1.,0.],[1.,1.],[0.,1.]];
                        let vertices = [0,1,2,0,2,3].map(|i| {
                            let [u,v] = corners[i];
                            [(cx+(0.5-u)*0.88) as f32,(cy+0.175+(0.5-v)*0.505) as f32,
                             (cz-0.059) as f32,u as f32,v as f32,1.]
                        });
                        for mut face in projection::project(&vertices,&view) {
                            for vertex in face[12..].chunks_exact_mut(4) { vertex[3]=105.; }
                            triangles.push(face);
                        }
                    }
                }
            }
            if let Some(id) = input["body"].as_u64() {
                let data = prepared.entity(id)?;
                let crate::base_entity::Appearance::Visual {
                    vertices,
                    shader,
                    procedural,
                } = &data.appearance
                else {
                    return Err("body must be visual".into());
                };
                if shader != "player_body" {
                    return Err("body requires player shader".into());
                }
                let procedural=data.visual_procedural().ok_or("player metadata missing")?;
                let mut local = input.clone();
                local["position"] = serde_json::json!([0, 0, 0]);
                local["two_sided"] = serde_json::json!(true);
                let camera = input["position"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap().powi(2))
                    .sum::<f64>()
                    .sqrt();
                local["far"] = serde_json::json!(camera + 2. * 6_371_000.);
                local["near"] = serde_json::json!(0.05);
                // No culling in project: the head mask and UVs retain exact triangle correspondence.
                let projected = projection::project(&vertices, &local);
                for (i, mut triangle) in projected.into_iter().enumerate() {
                    if procedural["head_triangles"][i] == true {
                        continue;
                    }
                    for v in 0..3 {
                        triangle[12 + v * 4] =
                            procedural["uvs"][i * 3 + v][0].as_f64().unwrap() as f32;
                        triangle[13 + v * 4] =
                            procedural["uvs"][i * 3 + v][1].as_f64().unwrap() as f32;
                        triangle[14 + v * 4] = lighting.brightness() as f32;
                        triangle[15 + v * 4] = -2.;
                    }
                    triangles.push(triangle);
                }
            }
            let mut all_instances = input["instances"].as_array().cloned().unwrap_or_default();
            if let Some(water) = input["water_instances"].as_array() {
                all_instances.extend(water.iter().cloned());
            }
            let mut water_packets = Vec::new();
            if !all_instances.is_empty() {
                let instances = &all_instances;
                if input["instances"].as_array().map_or(0, Vec::len) > MAX_SCENE_INSTANCES
                    || input["water_instances"].as_array().map_or(0, Vec::len) > 16
                {
                    return Err("scene appearance instance budget exceeded".into());
                }
                for instance in instances {
                    // All render instances use the declared typed Entity/Get boundary.
                    // Validation occurs at creation/update, not by serializing and
                    // reparsing unchanged mesh coordinates each frame.
                    let e = prepared.entity(instance["entity"].as_u64().ok_or("instance Entity missing")?)?;
                    let crate::base_entity::Appearance::Visual {
                        vertices,
                        shader,
                        procedural,
                    } = &e.appearance
                    else {
                        return Err("tool instance must be visual".into());
                    };
                    if shader == "water_surface" {
                        let mut packet = if let Some(packet)=&e.water_packet{packet.as_ref().clone()}else{crate::water::presentation::Packet::parse(procedural)?};
                        let mut metadata=packet.metadata();light_water_packet(&mut metadata,&lighting);packet.optics=metadata["optics"].take();packet.optics["fullbright"]=serde_json::json!(input["diagnostic_cave_fullbright"]==true);packet.optics["cave_fog"]=serde_json::json!(deep_cave);
                        if deep_cave {packet.retain_deep_surfaces();}
                        if !packet.indices.is_empty() {water_packets.push(packet);}
                        continue;
                    }
                    let unlit = shader == "unlit_surface" || instance["prismasqueek"]==true || instance["silver_spider_crab"]==true;
                    if shader != "material_surface" && !unlit {
                        return Err("tool instance shader invalid".into());
                    }
                    if instance["laboratory"]==true && crate::audio::length(crate::audio::sub(crate::audio::vector(&instance["origin"])?,crate::audio::vector(&input["position"])?))>250. {continue;}
                    let mut vertices=vertices.clone();
                    if instance["prismasqueek"]==true {super::laboratory_appearance::shade_mouse(&mut vertices,instance,input)?;}
                    if !instance["bird_pose"].is_null() {
                        super::cardinal_rig::animate(&mut vertices, instance)?;
                    }
                    if !instance["crab_pose"].is_null() {
                        super::silver_spider_crab_rig::animate(&mut vertices, instance)?;
                    }
                    if !instance["deer_pose"].is_null() {
                        super::deer_rig::animate(&mut vertices, instance)?;
                    }
                    let origin: [f64; 3] = serde_json::from_value(instance["origin"].clone())
                        .map_err(|e| format!("tool origin: {e}"))?;
                    let camera = crate::audio::vector(&input["position"])?;
                    for v in &mut vertices {
                        for i in 0..3 {
                            v[i] = (v[i] as f64 + origin[i]
                                - camera[i])
                                as f32;
                        }
                    }
                    let mut local = input.clone();
                    local["position"] = serde_json::json!([0, 0, 0]);
                    local["two_sided"] = serde_json::json!(true);
                    local["near"] = serde_json::json!(0.05);
                    local["far"] = serde_json::json!(
                        input["position"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_f64().unwrap().powi(2))
                            .sum::<f64>()
                            .sqrt()
                            + 2. * 6_371_000.
                    );
                    if deep_cave {
                        vertices=vertices.chunks_exact(3).filter(|tri|tri.iter().any(|v|{
                            let point=std::array::from_fn(|i|camera[i]+v[i] as f64);
                            crate::terrain::underground::local(point)[1]< -18.
                        })).flatten().copied().collect();
                    }
                    // Placeholder emission never evaluates per-vertex illumination.
                    if !unlit && input["diagnostic_cave_fullbright"]!=true {
                        for vertex in &mut vertices {
                            let point=std::array::from_fn(|i|camera[i]+vertex[i] as f64);
                            let exposure=crate::terrain::underground::sky_exposure(point) as f32;
                            let fade=if deep_cave {let t=((crate::audio::length([vertex[0] as f64,vertex[1] as f64,vertex[2] as f64])-24.)/10.).clamp(0.,1.);(1.-t*t*(3.-2.*t)) as f32}else{1.};
                            for c in 3..6 {vertex[c]*=exposure*fade;}
                        }
                    }
                    let mut projected = projection::project(&vertices, &local);
                    for triangle in &mut projected {
                        for v in 0..3 {
                            for c in 0..3 {
                                triangle[12 + v * 4 + c] *=
                                    if unlit || input["diagnostic_cave_fullbright"]==true {1.}else{(lighting.tint[c] * lighting.brightness()) as f32};
                            }
                        }
                    }
                    triangles.extend(projected);
                }
            }
            if let Some(id) = input["emotion"].as_u64() {
                let entity=prepared.entity(id)?;
                let crate::base_entity::Appearance::Visual {
                    vertices,
                    shader,
                    procedural,
                } = &entity.appearance
                else {
                    return Err("color emotion must be visual".into());
                };
                if shader != "color_emotion" {
                    return Err("invalid color emotion Entity".into());
                }
                let kind = if procedural["mode"] == "pickup" {
                    -5.
                } else {
                    -4.
                };
                for chunk in vertices.chunks_exact(3) {
                    let mut t = [0f32; 24];
                    for (i, v) in chunk.iter().enumerate() {
                        t[i * 4..i * 4 + 4].copy_from_slice(&[v[0], v[1], 0., 1.]);
                        t[12 + i * 4..16 + i * 4].copy_from_slice(&[
                            procedural["intensity"].as_f64().unwrap() as f32,
                            procedural["time"].as_f64().unwrap() as f32,
                            procedural["charge"].as_f64().unwrap() as f32,
                            kind,
                        ]);
                    }
                    triangles.push(t);
                }
            }
            if let Some(job)=terrain_job {
                let mut terrain=job.finish()?;
                terrain.extend(triangles);
                triangles=terrain;
            }
            if shader == "planet_surface" {
                if prepared.system_enabled("PhoneRenderSystem") {
                    crate::phone::flashlight::illuminate(&mut triangles,input,
                        prepared.geometry.as_deref().map(|s|s as &dyn crate::audio::geometry::Geometry))?;
                }
                triangles.extend(prepared.phone.clone());
                if prepared.system_enabled("AudioRayVisualizerSystem") {
                    triangles.extend(prepared.rays.clone());
                }
                // Sky-first remains production: opaque-first was pixel-exact
                // but slower on the measured Metal renderer (see benchmark evidence).
                if !sky_first {let mut sky=lighting.sky(input);sky.append(&mut triangles);triangles=sky;}
            }
            if shader=="planet_surface" {
                if let Some(site)=&prepared.nightworld {
                    let (mesh,view)=site.geometry(input)?;
                    triangles.extend(projection::project(&mesh,&view));
                }
                if input["nightworld_loading"].as_f64().is_some_and(|t|t>0.) {
                    triangles=nightworld_loading(input);water_packets.clear();
                }
            }
            if let Some((origin,venue,_))=&prepared.venue {
                if let Some(site)=&venue.laboratory {
                    triangles.extend(super::laboratory_appearance::portal_view(site,*origin,input,&prepared.entities)?);
                }
            }
            Ok(Scene { view: material_view, triangles, vegetation, water: water_packets })
}

}

/// Let opaque surfaces reject hidden sky fragments. Stop before the player
/// texture or any blended overlay so their original background stays intact.
/// The sky keeps its original far depth, equations, sampling and triangle count.
pub(crate) fn insert_sky_after_opaque(triangles: &mut Vec<[f32;24]>, sky: Vec<[f32;24]>) {
    let first_blended = {
        triangles.iter().position(|t| {
            let material = t[15];
            material == 121. || (material > -2.5 && material < -1.5) || (material <= -3.5 && material > -4096.)
        }).unwrap_or(triangles.len())
    };
    triangles.splice(first_blended..first_blended, sky);
}

// Sky palette RGB already contains the time-of-day brightness. Reducing its
// blend weight at night selects the bright neutral fallback instead.
fn light_water_packet(packet: &mut Value, lighting: &super::atmosphere::Lighting) {
    packet["optics"]["light"] = serde_json::json!(lighting.primary);
    packet["optics"]["sky_top"] =
        serde_json::json!(super::atmosphere::palette(lighting.fraction, 0.));
    packet["optics"]["sky_bottom"] =
        serde_json::json!(super::atmosphere::palette(lighting.fraction, 1.));
    packet["optics"]["neutral_reflection_intensity"] = serde_json::json!(
        packet["optics"]["neutral_reflection_intensity"]
            .as_f64()
            .unwrap_or(1.)
            * lighting.brightness()
    );
}

#[cfg(test)]
mod sky_order_tests {
    use super::*;
    #[test]
    fn night_water_uses_dark_sky_instead_of_white_fallback() {
        for sky_weight in [0., 0.4, 1.] {
            let sample = |fraction| {
                let mut packet = serde_json::json!({"optics":{
                    "sky_color_intensity":sky_weight,"neutral_reflection_intensity":1.
                }});
                light_water_packet(&mut packet, &super::super::atmosphere::Lighting::at(fraction, [0.,1.,0.]));
                let o = &packet["optics"];
                assert_eq!(o["sky_color_intensity"].as_f64().unwrap(), sky_weight);
                let neutral = (1.-sky_weight)*o["neutral_reflection_intensity"].as_f64().unwrap();
                // Grazing reflection contribution, before any planar-scene blend.
                (0..3).map(|i| sky_weight*o["sky_top"][i].as_f64().unwrap()+neutral).sum::<f64>()/3.
            };
            assert!(sample(0.) < sample(0.5)*0.3, "night reflection must dim for sky weight {sky_weight}");
        }
    }
    #[test]
    fn sky_is_after_opaque_and_before_original_blended_suffix() {
        let face=|tag| { let mut t=[0.;24]; t[15]=tag;t };
        for blended in [-2.,-4.,-5.] {
            let mut triangles=vec![face(-4096.),face(112.),face(blended),face(0.)];
            insert_sky_after_opaque(&mut triangles,vec![face(-10.)]);
            assert_eq!(triangles.iter().map(|t|t[15]).collect::<Vec<_>>(),vec![-4096.,112.,-10.,blended,0.]);
        }
    }
}

struct CompletedScene { scene:Scene,capture:bool,sequence:u64,work_ms:f64,audicle:Option<u64> }
/// One running scene and one queued snapshot, both immutable accepted inputs. Graphics and
/// Host routing stay on the owning thread; a completed coherent camera/scene is
/// published at Pump. Queue depth is bounded; incomplete work is never published.
pub(crate) struct Worker {
    entity:u64,
    input:Option<std::sync::mpsc::SyncSender<(Prepared,u64,Option<u64>)>>,
    output:std::sync::mpsc::Receiver<Result<CompletedScene,String>>,
    thread:Option<std::thread::JoinHandle<()>>,
    pending:usize,issued:u64,published:u64,completed:u64,skipped:u64,total_work_ms:f64,max_work_ms:f64,max_triangles:usize,
}
impl Worker {
 fn start(entity:u64)->Result<Self,String>{
  let(tx,rx)=std::sync::mpsc::sync_channel::<(Prepared,u64,Option<u64>)>(1);let(done_tx,done_rx)=std::sync::mpsc::sync_channel(2);
  let thread=std::thread::Builder::new().name("mirror-prepare-scene".into()).stack_size(16*1024*1024).spawn(move||{
   while let Ok((prepared,sequence,audicle))=rx.recv(){
    let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::GameGraphics);
    let start=std::time::Instant::now();let capture=prepared.view["capture"]==true;
    let result=prepared.build().map(|scene|CompletedScene{scene,capture,sequence,audicle,work_ms:start.elapsed().as_secs_f64()*1000.});
    if done_tx.send(result).is_err(){break;}
   }
  }).map_err(|e|format!("scene preparation worker: {e}"))?;
  Ok(Self{entity,input:Some(tx),output:done_rx,thread:Some(thread),pending:0,issued:0,published:0,completed:0,skipped:0,total_work_ms:0.,max_work_ms:0.,max_triangles:0})
 }
 fn publish(&mut self,host:&mut Host,result:Result<CompletedScene,String>)->Result<(),String>{
  let done=result?;
  // Pause/stop can finish the requesting event while its CPU scene is still
  // building. Retain the displayed image instead of publishing stale work.
  if !host.audicles.is_go(done.audicle){return Ok(());}
  let previous=host.active_audicle;
  host.active_audicle=done.audicle;
  let result=publish(host,self.entity,done.capture,done.scene);
  host.active_audicle=previous;
  result?;self.published=done.sequence;Ok(())
 }
 fn poll(&mut self,host:&mut Host,wait:bool)->Result<(),String>{
  let mut latest=None;
  while self.pending>0 {
   let result=if wait{Some(self.output.recv().map_err(|_|"scene preparation worker terminated".to_owned())?)}else{
    match self.output.try_recv(){Ok(result)=>Some(result),Err(std::sync::mpsc::TryRecvError::Empty) if latest.is_none()=>{
       match self.output.recv_timeout(std::time::Duration::from_micros(750)){
        Ok(result)=>Some(result),Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>None,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>return Err("scene preparation worker terminated".into())
       }
     },Err(std::sync::mpsc::TryRecvError::Empty)=>None,
     Err(std::sync::mpsc::TryRecvError::Disconnected)=>return Err("scene preparation worker terminated".into())}
   };
   let Some(result)=result else{break;};self.pending-=1;
   let done=result?;self.max_triangles=self.max_triangles.max(done.scene.triangles.len());self.completed+=1;self.total_work_ms+=done.work_ms;self.max_work_ms=self.max_work_ms.max(done.work_ms);latest=Some(done);
  }
  if let Some(done)=latest{self.publish(host,Ok(done))?;}Ok(())
 }
 pub(crate) fn sequence(&self)->u64{self.published}
 pub(crate) fn report(&self)->Value{serde_json::json!({"issued":self.issued,"published_sequence":self.published,"requests_skipped_while_busy":self.skipped,
  "mean_prepare_ms":if self.completed>0{Some(self.total_work_ms/self.completed as f64)}else{None},"max_prepare_ms":self.max_work_ms,
  "in_flight":self.pending,"completed":self.completed,"max_scene_triangles":self.max_triangles,"terrain_streaming":environment_geometry::streaming_report(),"scope":"At most one running and one queued immutable scene, latest completed coherent camera at draw. Skipped intermediate requests are counted, never represented as fresh scenes. No graphics API or Host access on worker. Initial scene waits before gameplay starts; subsequent Pumps allow at most0.75ms readiness grace, never wait for a rebuild."})}
}
impl Drop for Worker{fn drop(&mut self){self.input.take();if let Some(thread)=self.thread.take(){let _=thread.join();}}}
fn publish(host:&mut Host,entity:u64,capture:bool,scene:Scene)->Result<(),String>{
 let request=serde_json::json!({"entity":entity,"capture":capture,"view":scene.view});
 let result=host.set_scene_view_water(&request,&scene.triangles,scene.vegetation,&scene.water);
 SCENE_BUFFER.with(|buffer|{let mut triangles=scene.triangles;triangles.clear();*buffer.borrow_mut()=triangles;});
 result.map(|_|())
}
fn submit_game_scene(host:&mut Host,input:&Value)->Result<Value,String>{
 let entity=input["entity"].as_u64().ok_or("scene Entity required")?;
 let mut worker=host.scene_preparation.take().map_or_else(||Worker::start(entity),Ok)?;
 let result=(||{
  // Present only at Pump. Polling here as well could upload two scenes
  // in one game iteration and none in the next, creating avoidable spikes.
  if input["capture"]==true{worker.poll(host,true)?;}
  if worker.pending>=2{worker.skipped+=1;return Ok(Value::Null);}
  let mut prepared=prepare(host,input)?;
  // Return the previously presented allocation to the builder, rather than
  // allocate/free/copy a full terrain triangle batch every frame.
  let recycle=SCENE_BUFFER.with(|buffer|std::mem::take(&mut *buffer.borrow_mut()));
  prepared.recycle=Some(recycle);
  match worker.input.as_ref().unwrap().try_send((prepared,worker.issued+1,host.active_audicle)){
   Ok(())=>{worker.issued+=1;worker.pending+=1;},
   Err(std::sync::mpsc::TrySendError::Full(_))=>{worker.skipped+=1;return Ok(Value::Null);},
   Err(std::sync::mpsc::TrySendError::Disconnected(_))=>return Err("scene preparation worker terminated".into()),
  }
  if worker.issued==1||input["capture"]==true{worker.poll(host,true)?;}
  Ok(Value::Null)
 })();
 host.scene_preparation=Some(worker);result
}
fn poll_game_scene(host:&mut Host,entity:Option<u64>)->Result<(),String>{
 if let Some(mut worker)=host.scene_preparation.take(){
  let result=if Some(worker.entity)==entity{worker.poll(host,false)}else{Ok(())};
  host.scene_preparation=Some(worker);result
 }else{Ok(())}
}

#[cfg(test)]mod scene_pipeline_tests {
 use super::*;
 #[test]fn nightworld_scene_and_loading_card_use_complete_scene_pipeline(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  for elevation in [-92.,-40.,60.] {
   let mut p=snapshot(0.);let c=crate::terrain::nightworld::shaft();
   let entity=crate::base_entity::Entity::from_spec(1,&serde_json::json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[],"underground":serde_json::from_str::<Value>(include_str!("../../Procedures/caves.json")).unwrap()}})).unwrap();
   p.entities.insert(1,std::sync::Arc::new(entity));
   p.view=serde_json::json!({"entity":1,"position":crate::terrain::nightworld::world([c[0],elevation,c[2]]),"forward":[0.,0.,-1.],"up":[0.,-1.,0.],"right":[-1.,0.,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}});
   p.nightworld=Some(std::sync::Arc::new(crate::theatre::nightworld::Site::load(root).unwrap()));
   let scene=p.build().unwrap();assert!(scene.triangles.len()>100);crate::scene_limits::validate(&scene.triangles,false).unwrap();assert!(scene.vegetation.is_none());
  }
  let mut p=snapshot(0.);p.view["nightworld_loading"]=serde_json::json!(0.4);
  let card=p.build().unwrap();assert!(card.triangles.len()>=8);assert!(card.water.is_empty());assert!(card.vegetation.is_none());assert!(card.triangles[2][2]<card.triangles[0][2],"loading marks must be in front of the opaque background");crate::scene_limits::validate(&card.triangles,false).unwrap();
 }
 #[test]fn exterior_turn_back_scene_fits_native_storage(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let output=std::env::temp_dir().join(format!("mirror-exterior-turn-{}",std::process::id()));
  let mut host=Host::load_paths(root,&output).unwrap();
  crate::base_system::layer_two::theatre_system::dispatch(&mut host,"Configure",&Value::Null).unwrap();
  let mut prepared=snapshot(0.);
  let entity=crate::base_entity::Entity::from_spec(1,&serde_json::json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]})).unwrap();
  prepared.entities.insert(1,std::sync::Arc::new(entity));
  prepared.view=serde_json::from_str(include_str!("../../BuildSupport/Fixtures/exterior-turn-back-view.json")).unwrap();
  prepared.enabled.insert("TheatreSystem");
  prepared.venue=host.theatre.venue.clone().map(|v|(host.theatre.origin,v,host.theatre.chair.clone()));
  let scene=prepared.build().unwrap();
  eprintln!("exterior turn-back scene: {} triangles",scene.triangles.len());
  assert!(scene.triangles.iter().flatten().all(|v|v.is_finite()));
  assert!(scene.triangles.len()<=crate::scene_limits::MAX_SCENE_TRIANGLES,"{} triangles exceed native storage",scene.triangles.len());
 }
 fn snapshot(x:f64)->Prepared{
  let entity=crate::base_entity::Entity::from_spec(1,&serde_json::json!({"kind":"visual","shader":"vertex_color","vertices":[[-1.,-1.,-4.,1.,0.,0.],[1.,-1.,-4.,0.,1.,0.],[0.,1.,-4.,0.,0.,1.]]})).unwrap();
  Prepared{view:serde_json::json!({"entity":1,"position":[x,0.,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"capture":true,"day_night":{"fraction":0.5}}),entities:std::collections::HashMap::from([(1,std::sync::Arc::new(entity))]),enabled:Default::default(),plants:None,venue:None,nightworld:None,monitor:false,phone:vec![],rays:vec![],geometry:None,security:None,reflections:false,recycle:None}
 }
 #[test]fn queued_scenes_keep_their_camera_geometry_capture_and_sequence_coherent(){
  let worker=Worker::start(1).unwrap();
  let expected_a=snapshot(0.).build().unwrap().triangles;
  let expected_b=snapshot(1.).build().unwrap().triangles;
  assert_ne!(expected_a,expected_b);
  worker.input.as_ref().unwrap().send((snapshot(0.),1,Some(41))).unwrap();
  worker.input.as_ref().unwrap().send((snapshot(1.),2,Some(42))).unwrap();
  for (sequence,x,expected) in [(1,0.,expected_a),(2,1.,expected_b)]{
   let result=worker.output.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap();
   assert_eq!(result.sequence,sequence);assert_eq!(result.audicle,Some(sequence+40));assert!(result.capture);assert_eq!(result.scene.view["position"][0],x);assert_eq!(result.scene.triangles,expected);
  }
 }
 #[test]fn failed_scene_is_reported_and_closing_with_a_queued_reply_joins(){
  let worker=Worker::start(1).unwrap();let mut bad=snapshot(0.);bad.entities.clear();
  worker.input.as_ref().unwrap().send((bad,1,None)).unwrap();
  assert!(worker.output.recv_timeout(std::time::Duration::from_secs(5)).unwrap().is_err());
  worker.input.as_ref().unwrap().send((snapshot(0.),2,None)).unwrap();drop(worker);
 }
}

#[cfg(test)]
pub(crate) fn check_finished_scene_discard(host:&mut Host,audicle:Option<u64>){
 let mut worker=Worker::start(1).unwrap();
 let previous=host.active_audicle;
 let done=CompletedScene{scene:Scene{view:Value::Null,triangles:Vec::new(),vegetation:None,water:Vec::new()},capture:false,sequence:1,work_ms:0.,audicle};
 worker.publish(host,Ok(done)).unwrap();
 assert_eq!(worker.published,0);assert_eq!(host.active_audicle,previous);
}

fn nightworld_loading(view:&Value)->Vec<[f32;24]>{
    let remaining=view["nightworld_loading"].as_f64().unwrap_or(0.);
    let progress=(1.-remaining/0.8).clamp(0.,1.) as f32;
    let mut out=Vec::new();
    let mut rect=|x0:f32,y0:f32,x1:f32,y1:f32,color:[f32;3]|{
        let depth=0.9-out.len() as f32*0.0001;
        let p=[[x0,y0],[x1,y0],[x1,y1],[x0,y1]];
        for indices in [[0,1,2],[0,2,3]]{let mut t=[0.;24];for (i,j) in indices.into_iter().enumerate(){t[i*4..i*4+4].copy_from_slice(&[p[j][0],p[j][1],depth,1.]);t[12+i*4..16+i*4].copy_from_slice(&[color[0],color[1],color[2],1.]);}out.push(t);}
    };
    rect(-1.,-1.,1.,1.,[0.009,0.012,0.021]);
    rect(-0.42,-0.08,0.42,-0.065,[0.08,0.1,0.14]);
    rect(-0.42,-0.08,-0.42+0.84*progress,-0.065,[0.45,0.9,0.08]);
    // Compact five-column type keeps loading inside the graphics batch and
    // does not add a gameplay HUD or a platform-specific overlay dependency.
    let glyphs=[[17u8,25,21,19,17,17,17],[31,4,4,4,4,4,31],[14,17,16,23,17,17,14],[17,17,17,31,17,17,17],[31,4,4,4,4,4,4],[17,17,17,21,21,21,10],[14,17,17,17,17,17,14],[30,17,17,30,20,18,17],[16,16,16,16,16,16,31],[30,17,17,17,17,17,30]];
    for (g,rows) in glyphs.iter().enumerate(){for (row,bits) in rows.iter().enumerate(){for col in 0..5{if bits&(1<<(4-col))!=0 {
        let x= -0.44+(g*6+col) as f32*0.015;let y=0.065-row as f32*0.018;
        rect(x,y,x+0.012,y+0.014,[0.76,0.81,0.85]);
    }}}}
    rect(-0.035,0.12,0.0,0.3,[0.45,0.9,0.08]);
    rect(0.0,0.12,0.035,0.3,[0.85,0.02,0.4]);
    out
}

/// Finite native review through the same immutable scene builder as gameplay.
/// It deliberately excludes input, audio-device ownership and a traversal claim.
pub(crate) fn nightworld_review_scene(root:&std::path::Path,view:Value)->Result<Scene,String>{
 let caves:Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/caves.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 let entity=crate::base_entity::Entity::from_spec(1,&serde_json::json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[],"underground":caves}}))?;
 Prepared{view,entities:std::collections::HashMap::from([(1,std::sync::Arc::new(entity))]),enabled:Default::default(),plants:None,venue:None,nightworld:Some(std::sync::Arc::new(crate::theatre::nightworld::Site::load(root)?)),monitor:false,phone:Vec::new(),rays:Vec::new(),geometry:None,security:None,reflections:false,recycle:None}.build()
}

/// Finite review of the same terrain, forest and regional venue composition.
pub(crate) fn regional_review_scene(root:&std::path::Path,view:Value)->Result<Scene,String>{
 let caves:Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/caves.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 let entity=crate::base_entity::Entity::from_spec(1,&serde_json::json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[],"underground":caves}}))?;
 let state=crate::base_system::layer_two::theatre_system::State::default();
 let venue=crate::base_system::layer_two::theatre_system::mesh(&state,&view)?;
 Prepared{view,entities:std::collections::HashMap::from([(1,std::sync::Arc::new(entity))]),enabled:std::collections::HashSet::from(["VegetationSystem","TheatreSystem"]),plants:Some(Default::default()),venue,nightworld:None,monitor:false,phone:Vec::new(),rays:Vec::new(),geometry:None,security:None,reflections:false,recycle:None}.build()
}
