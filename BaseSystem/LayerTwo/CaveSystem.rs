//! Shared continuous cave queries. Geological voids are distinct from tool cuts.
use crate::{
    audio::{V, add, dot, length, mul, sub, unit, vector},
    terrain::{self, underground::Field},
};
use serde_json::{Value, json};
use std::path::Path;
#[derive(Default)]
pub(crate) struct State {
    field: Option<Field>,
    configured: bool,
}
fn height(normal: V, edits: &[Value]) -> f64 {
    terrain::tool_cut::height(normal, terrain::height(normal), edits, true)
}
pub(crate) fn dispatch(
    state: &mut State,
    root: &Path,
    hook: &str,
    input: &Value,
) -> Result<Value, String> {
    if hook == "Configure" {
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("Procedures/caves.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let data=crate::modding::patch_document(root,"Procedures/caves.json",data)?;
        state.field = Field::parse(&data)?;
        state.configured = true;
        return Ok(json!({"enabled":state.field.is_some()}));
    }
    if !state.configured {
        return Err("caves have not been configured".into());
    }
    if hook == "Snapshot" {
        return Ok(state.field.as_ref().map_or(Value::Null, Field::snapshot));
    }
    let Some(field) = &mut state.field else {
        return Ok(match hook {
            "Support" => json!({"height":null}),
            "ResolveBody" => json!({"position":input["position"],"normals":[]}),
            _ => Value::Null,
        });
    };
    let edits = input["edits"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !terrain::tool_cut::valid_edits(&json!(edits)) {
        return Err("accepted cave surface edits required".into());
    }
    // Query dispatch owns this state exclusively. Reuse parsed geometry and
    // install the same accepted edit domains instead of cloning the full cave
    // profile, tunnels and formations for every support sample.
    field.set_material_domains(edits);
    let sample_height = |n| height(n, edits);
    match hook {
        "Sample" => {
            let p = vector(&input["position"])?;
            let distance = field.distance(p, sample_height(unit(p)));
            Ok(
                json!({"signed_distance":distance,"solid":distance<=0.,"normal":field.normal(p,&sample_height),"depths":terrain::underground::local(p)[1]< -22.}),
            )
        }
        "Support" => {
            let p = vector(&input["position"])?;
            let eye = input["eye_height"].as_f64().unwrap_or(2.);
            if !eye.is_finite() || !(0.5..=3.0).contains(&eye) {
                return Err("bounded player eye height required".into());
            }
            let support = field.support(p, eye, &sample_height).or_else(|| {
                (field.affected(p, 4.) && length(p) - terrain::RADIUS < sample_height(unit(p)) - 1.)
                    .then(|| length(p) - terrain::RADIUS - eye - 128.)
            });
            Ok(json!({"height":support}))
        }
        "Mesh" => {
            let p = vector(&input["position"])?;
            Ok(
                json!({"vertices":field.mesh(p,edits)?.into_iter().flatten().collect::<Vec<_>>(),"origin":[0.,0.,0.],"sampling_metres":if field.snapshot()["meshing"]=="surface_net" {3.0} else {2.0}}),
            )
        }
        "ResolveBody" => {
            let previous = vector(&input["previous"])?;
            let desired = vector(&input["position"])?;
            let eye = input["eye_height"].as_f64().unwrap_or(2.);
            if !eye.is_finite() || !(0.5..=3.0).contains(&eye) {
                return Err("bounded player eye height required".into());
            }
            let delta = sub(desired, previous);
            let travel = length(delta);
            if travel > 8. {
                return Err("cave movement exceeds bounded sweep".into());
            }
            if !field.affected(previous, 4.) && !field.affected(desired, 4.) {
                return Ok(json!({"position":desired,"normals":[]}));
            }
            let steps = (travel / 0.12).ceil().max(1.) as usize;
            let mut position = previous;
            let mut normals = Vec::<V>::new();
            for step in 1..=steps {
                let mut candidate = add(previous, mul(delta, step as f64 / steps as f64));
                for _ in 0..8 {
                    let up = unit(candidate);
                    let mut changed = false;
                    for offset in [-eye + 0.26, -eye * 0.5, -0.26] {
                        let point = add(candidate, mul(up, offset));
                        let distance = field.distance(point, sample_height(unit(point)));
                        if distance < 0.26 - 1e-6 {
                            let normal = field.normal(point, &sample_height);
                            if length(normal) < 0.5 {
                                return Err("cave contact has no valid normal".into());
                            }
                            candidate = add(candidate, mul(normal, (0.26 - distance).min(0.5)));
                            if normals.len() < 16 && !normals.iter().any(|n| dot(*n, normal) > 0.98)
                            {
                                normals.push(normal);
                            }
                            changed = true;
                        }
                    }
                    if !changed {
                        break;
                    }
                }
                let up = unit(candidate);
                if [-eye + 0.26, -eye * 0.5, -0.26].iter().any(|offset| {
                    let p = add(candidate, mul(up, *offset));
                    field.distance(p, sample_height(unit(p))) < 0.25
                }) {
                    break;
                }
                position = candidate;
            }
            Ok(json!({"position":position,"normals":normals}))
        }
        _ => Err("unknown cave hook".into()),
    }
}

fn support_one(field:&Field,p:V,eye:f64,edits:&[Value])->Option<f64>{
    // A radial support search repeatedly normalizes points on the same column.
    // Reuse only bit-identical directions within this one immutable query; no
    // approximation, cross-frame cache, or changed terrain/cave sample spacing.
    let memo=std::cell::RefCell::new(([([0u64;3],0f64);32],0usize));
    let sample_height=|n:V|{
        let key=n.map(f64::to_bits);
        {let memo=memo.borrow();if let Some((_,h))=memo.0[..memo.1].iter().find(|(k,_)|*k==key){return *h;}}
        let h=height(n,edits);let mut memo=memo.borrow_mut();let i=memo.1;
        if i<memo.0.len(){memo.0[i]=(key,h);memo.1+=1;}h
    };
    field.support(p,eye,&sample_height).or_else(||(field.affected(p,4.)&&length(p)-terrain::RADIUS<sample_height(unit(p))-1.).then(||length(p)-terrain::RADIUS-eye-128.))
}
pub(crate) fn supports_numeric(state:&mut State,positions:&[V],eye:f64,edits:&[Value])->Result<Vec<Option<f64>>,String>{
    static PARALLEL:std::sync::OnceLock<bool>=std::sync::OnceLock::new();
    let parallel=*PARALLEL.get_or_init(||!cfg!(test)&&std::env::var("WORMWOOD_CAVE_SUPPORT_PARALLEL").as_deref()!=Ok("0")&&std::thread::available_parallelism().map_or(false,|n|n.get()>=4));
    supports_batch(state,positions,eye,edits,parallel)
}
fn supports_batch(state:&mut State,positions:&[V],eye:f64,edits:&[Value],parallel:bool)->Result<Vec<Option<f64>>,String>{
    if !state.configured{return Err("caves have not been configured".into());}
    if positions.len()>32||positions.iter().flatten().any(|n|!n.is_finite())||!eye.is_finite()||!(0.5..=3.).contains(&eye)||!terrain::tool_cut::valid_edits(&json!(edits)){return Err("invalid bounded cave support batch".into());}
    let Some(field)=&mut state.field else{return Ok(vec![None;positions.len()]);};field.set_material_domains(edits);let field=&*field;
    if !parallel||positions.len()<3{return Ok(positions.iter().map(|&p|support_one(field,p,eye,edits)).collect());}
    Ok(std::thread::scope(|scope|{
        let mut jobs=Vec::new();
        for i in 0..3 {let part=&positions[positions.len()*i/3..positions.len()*(i+1)/3];let build=move||{let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::GameLogic);part.iter().map(|&p|support_one(field,p,eye,edits)).collect::<Vec<_>>()};
            match std::thread::Builder::new().name("mirror-foot-support".into()).spawn_scoped(scope,build){Ok(job)=>jobs.push((Some(job),None)),Err(_)=>jobs.push((None,Some(build())))}
        }
        let mut out=Vec::with_capacity(positions.len());for (job,ready) in jobs{let mut values=if let Some(job)=job{job.join().unwrap_or_else(|e|std::panic::resume_unwind(e))}else{ready.unwrap()};out.append(&mut values);}out
    }))
}
#[cfg(test)]mod support_batch_tests {
 use super::*;
 #[test]fn memoized_columns_keep_scalar_support_bit_exact_across_the_world(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));let mut state=State::default();dispatch(&mut state,root,"Configure",&Value::Null).unwrap();
  let field=state.field.as_ref().unwrap();let mut points=Vec::new();
  for (x,z) in [(-240.,0.),(-80.,-14.),(-51.,16.),(0.,-280.),(20.,-13.),(400.,500.)]{for y in [-70.,-48.,-32.,-9.,2.,50.,150.]{for dx in [0.,0.12,0.24]{points.push(terrain::underground::world([x+dx,y,z]));}}}
  let start=std::time::Instant::now();let expected:Vec<_>=points.iter().map(|&p|{let h=|n|height(n,&[]);field.support(p,2.,&h).or_else(||(field.affected(p,4.)&&length(p)-terrain::RADIUS<h(unit(p))-1.).then(||length(p)-terrain::RADIUS-2.-128.)).map(f64::to_bits)}).collect();let reference=start.elapsed();
  let start=std::time::Instant::now();let actual:Vec<_>=points.iter().map(|&p|support_one(field,p,2.,&[]).map(f64::to_bits)).collect();let memoized=start.elapsed();assert_eq!(actual,expected);
  eprintln!("{} radial support columns: reference {:?}, exact memo {:?}",points.len(),reference,memoized);
 }
 #[test]fn batched_support_matches_scalar_for_air_ground_and_deep_caves(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));let mut state=State::default();dispatch(&mut state,root,"Configure",&Value::Null).unwrap();
  let points:Vec<_>=[[-240.,40.,0.],[0.,2.,0.],[12.,-8.,-8.],[25.,-64.,-272.]].into_iter().flat_map(|p|(0..4).map(move|k|terrain::underground::world([p[0]+k as f64*0.12,p[1],p[2]]))).collect();
  let expected:Vec<_>=points.iter().map(|p|dispatch(&mut state,root,"Support",&json!({"position":p,"eye_height":2.,"edits":[]})).unwrap()["height"].as_f64().map(f64::to_bits)).collect();
  for parallel in [false,true]{let got=supports_batch(&mut state,&points,2.,&[],parallel).unwrap();assert_eq!(got.into_iter().map(|x|x.map(f64::to_bits)).collect::<Vec<_>>(),expected);}
 }
}
