#[path = "RigRetarget.rs"]
mod retarget;
// Presentation skinning and first-person head masking of the original Prismals mesh.
use crate::{
    base_system::layer_two::player_model_system::{Model, math::*},
    host::Host,
};
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct State {
    model: Option<Model>,
    entity: Option<u64>,
    feet: Option<[retarget::FootShape; 2]>,
    planned: bool,
    tube_mesh:Vec<[f32;6]>,
    tube_position:Option<V>,
}
fn spec(model: &Model, positions: &[V]) -> Value {
    let vertices: Vec<[f64; 6]> = positions
        .iter()
        .map(|p| [p[0], p[1], p[2], 1., 1., 1.])
        .collect();
    json!({"kind":"visual","shader":"player_body","vertices":vertices,"procedural":{"uvs":model.vertices.iter().map(|v|v.uv).collect::<Vec<_>>(),"head_triangles":model.head_triangles,"texture":"Procedures/Assets/Player/prismal_player.rgba"}})
}
// Fit the displayed sole to the visible terrain using world-space samples.
// The player/camera and physical tool are not moved by this presentation solve.
fn place_feet(
    host: &mut Host,
    model: &Model,
    shapes: &[retarget::FootShape; 2],
    pose: &mut Value,
    input: &Value,
) -> Result<(), String> {
    if input["sitting"]==true{return Ok(());}
    let eye = vector(&input["position"])?;
    let view = vector(&pose["view_offset"])?;
    let camera = add(eye, view);
    let origin = add(camera, vector(&pose["origin"])?);
    let left = vector(&pose["left"])?;
    let up = vector(&pose["up"])?;
    let forward = vector(&pose["forward"])?;
    let scale = pose["scale"].as_f64().ok_or("foot rig scale missing")?;
    let world_axis = |p: V| add(add(mul(left, p[0]), mul(up, p[1])), mul(forward, p[2]));
    let bind_axis = |p: V| [dot(p, left), dot(p, up), dot(p, forward)];
    let ankles = retarget::ankle_targets(model, pose)?;
    let planted = input["grounded"] == true && input["flying"] != true;
    let mut placements = Vec::with_capacity(2);
    let ankles:[V;2]=std::array::from_fn(|i|add(origin,mul(world_axis(ankles[i]),scale)));
    let probes:Vec<_>=ankles.iter().flat_map(|&ankle|[ankle,add(ankle,mul(left,0.12)),add(ankle,mul(forward,0.15))]).collect();
    let heights=host.surface_heights_numeric(&probes,camera)?;
    let grounds:Vec<_>=probes.iter().zip(heights).map(|(&p,h)|mul(unit(p),crate::terrain::RADIUS+h)).collect();
    let normals:[V;2]=std::array::from_fn(|i|{let centre=grounds[i*3];let across=grounds[i*3+1];let ahead=grounds[i*3+2];let mut normal=unit(cross(sub(across,centre),sub(ahead,centre)));if dot(normal,up)<0.{normal=mul(normal,-1.);}if len(normal)<0.5||dot(normal,up)<0.5{normal=up;}normal});
    let soles:Vec<_>=(0..2).flat_map(|i|shapes[i].samples.map(|sample|add(ankles[i],between(mul(world_axis(sample),scale),up,normals[i])))).collect();
    let heights=host.surface_heights_numeric(&soles,camera)?;
    for i in 0..2 {
        let ankle=ankles[i];let normal=normals[i];let mut correction=f64::NEG_INFINITY;
        for j in 0..5 {let sole=soles[i*5+j];let ground=mul(unit(sole),crate::terrain::RADIUS+heights[i*5+j]);correction=correction.max(dot(sub(ground,sole),up));}
        let lift=if planted{pose["foot_lift"][i].as_f64().unwrap_or(0.)}else{0.};correction+=0.006+lift;if !planted{correction=correction.max(0.);}
        let contacting=planted||correction>0.;let target=add(ankle,mul(up,correction));
        placements.push(json!({"ankle":mul(bind_axis(sub(target,origin)),1./scale),"up":bind_axis(if contacting{normal}else{up})}));
    }
    pose["foot_placement"] = json!(placements);
    Ok(())
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.player_rig);
    let result = (|| match hook {
        "Prepare" => {
            if s.entity.is_some() {
                return Err("player rig already prepared".into());
            }
            let data = host.invoke("PlayerModelSystem.Load", Value::Null)?;
            let model = Model::from_data(&data)?;
            let feet = retarget::foot_shapes(&model)?;
            host.invoke("InverseKinematicsSystem.Configure", data)?;
            let entity = host.invoke(
                "EntitySystem.Create",
                spec(
                    &model,
                    &model.vertices.iter().map(|v| v.p).collect::<Vec<_>>(),
                ),
            )?["entity"]
                .as_u64()
                .ok_or("player Entity not created")?;
            if host.mods.provides("waterpark") {
            let bytes=std::fs::read(host.mods.content_root("prismals").ok_or("Prismals assets missing")?.join("Procedures/Assets/Waterpark/tube.json")).map_err(|e|e.to_string())?;
            if bytes.len()>256*1024{return Err("tube mesh byte budget".into());}
            s.tube_mesh=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
            if s.tube_mesh.len()>4096||s.tube_mesh.len()%3!=0||s.tube_mesh.iter().flatten().any(|v|!v.is_finite()){return Err("invalid tube mesh".into());}
            }
            s.entity = Some(entity);
            s.model = Some(model);
            s.feet = Some(feet);
            host.invoke("GraphicsConnectionSystem.Texture",json!({"entity":input["entity"],"owner":entity,"path":"Procedures/Assets/Player/prismal_player.rgba"}))?;
            Ok(Value::Null)
        }
        "Plan" => {
            // Advance locomotion once, before contact, to expose THIS frame's
            // shoulders and anatomical limits to the held-tool constraint.
            let pose = host.invoke("InverseKinematicsSystem.Solve", input.clone())?;
            s.planned = true;
            Ok(pose)
        }
        "Advance" => {
            let eye=vector(&input["position"])?;
            s.tube_position=if input["sitting"]==true&&input["flying"]!=true&&host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()).is_some_and(|p|p.tube_region(crate::audio::sub(eye,host.theatre.origin))) {Some(crate::audio::add(eye,[0.,-0.38,0.]))}else{None};
            let mut request=input.clone();
            if std::mem::take(&mut s.planned) {
                request["dt"]=json!(0.);
                request["reset"]=json!(false);
            }
            let mut pose = host.invoke("InverseKinematicsSystem.Solve", request)?;
            let model = s.model.as_ref().ok_or("player rig not prepared")?;
            let feet = s.feet.as_ref().ok_or("player sole shapes not prepared")?;
            place_feet(host, model, feet, &mut pose, input)?;
            let positions = retarget::skin(model, &pose)?;
            // UVs, head mask and texture were validated at Prepare. Publish
            // only this frame's numeric skin through the same Update route.
            let vertices=positions.iter().map(|p|[p[0]as f32,p[1]as f32,p[2]as f32,1.,1.,1.]).collect();
            host.update_player_skin(s.entity.unwrap(),vertices)?;
            Ok(
                json!({"entity":s.entity.unwrap(),"view_offset":pose["view_offset"],"tool_grip":pose["tool_grip"],"tool_reach":pose["tool_reach"],"posture_offset":pose["posture_offset"]}),
            )
        }
        "Release" => {
            if let Some(entity) = s.entity {
                host.invoke(
                    "GraphicsConnectionSystem.ReleaseTexture",
                    json!({"entity":input["entity"],"owner":entity}),
                )?;
                host.invoke("EntitySystem.Remove", json!({"entity":entity}))?;
                s.entity = None;
                s.model = None;
                s.feet = None;
                s.planned = false;
                s.tube_mesh.clear();s.tube_position=None;
            }
            host.invoke("InverseKinematicsSystem.Release", Value::Null)?;
            Ok(Value::Null)
        }
        _ => Err("unknown player rig hook".into()),
    })();
    host.player_rig = s;
    result
}

/// Uses the Blender mesh retained by the player rig; no Entity or asset work per frame.
pub(crate) fn tube_frame(host:&Host,view:&Value)->Result<Vec<[f32;24]>,String> {
 let Some(position)=host.player_rig.tube_position else{return Ok(Vec::new());};
 let eye=crate::audio::vector(&view["position"])?;let offset=crate::audio::sub(position,eye);
 let vertices:Vec<_>=host.player_rig.tube_mesh.iter().map(|v|[v[0]+offset[0]as f32,v[1]+offset[1]as f32,v[2]+offset[2]as f32,v[3],v[4],v[5]]).collect();
 let mut local=view.clone();local["position"]=json!([0.,0.,0.]);local["two_sided"]=json!(true);
 Ok(super::appearance_system::projection::project(&vertices,&local))
}
