//! Layer-two bounded plant state/query operations; application Systems own clocks,
//! Audicles, persistence IO and physical authorization for damage.
use crate::vegetation::{self, State, math::*};
use serde_json::{Value, json};
#[path = "../LayerFour/VegetationGeometry.rs"]
mod appearance_geometry;
pub(crate) fn geometry_numeric(state: &State, input: &Value) -> Result<vegetation::packet::Geometry, String> {
    appearance_geometry::frame_numeric(state, input)
}
fn site(input: &Value) -> Result<vegetation::Site, String> {
    let position: V = serde_json::from_value(input["position"].clone())
        .map_err(|e| format!("site position: {e}"))?;
    let id = input["site_id"].as_u64().ok_or("site_id required")?;
    if !position.iter().all(|n| n.is_finite() && n.abs() <= 1e9) || length(position) < 1. {
        return Err("invalid site position".into());
    }
    vegetation::site_by_id(id)
        .filter(|s| length(sub(s.origin, position)) <= 48.)
        .ok_or("site not present near supplied position".into())
}
pub(crate) fn dispatch(state: &mut State, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "MaterialTree" => {
            if input["operation"]=="release" {state.materialized.clear();state.revision=state.revision.wrapping_add(1);return Ok(json!({"released":true}));}
            let site=site(input)?;
            if input["operation"]=="commit" {
                if input["expected_revision"].as_u64()!=Some(state.revision){return Err("tree changed before material transfer".into());}
                if !state.materialized.insert(site.id){return Err("tree already transferred".into());}
                state.revision=state.revision.wrapping_add(1);
                return Ok(json!({"revision":state.revision}));
            }
            let tree=state.tree(site).ok_or("tree is unavailable")?;
            let mut profile=vegetation::profiles::profile(site.profile);let season=vegetation::season::Foliage::for_profile(input,site.id,profile);profile.leaf_color=season.color;
            let mesh=vegetation::geometry::build_seasonal(&tree,profile,vegetation::geometry::Detail::Middle,0.,season.cover);
            let vertices:Vec<_>=mesh.indices.iter().map(|&i|{let v=mesh.vertices[i as usize];let p=tree.world(v.position);json!({"position":p,"color":v.color,"branch":v.branch_id,"material":v.material})}).collect();
            let branches:Vec<_>=tree.branches.iter().map(|b|json!({"start":tree.world(b.start),"end":tree.world(b.end),"radius":b.radius,"end_radius":b.radius*if b.order==0{0.77}else{0.36},"id":b.id,"order":b.order})).collect();
            Ok(json!({"site_id":site.id,"origin":site.origin,"height":tree.height,"revision":state.revision,"vertices":vertices,"branches":branches,"bark":vegetation::profiles::profile(site.profile).bark_color}))
        }
        "QueryWood" => query_wood(state,input),
        "QueryGeometry" => appearance_geometry::frame(state, input),
        "QueryPerches" => vegetation::query_perches(state, input),
        "Snapshot" => Ok(state.snapshot()),
        "Restore" => {
            state.restore(input)?;
            Ok(json!({"revision":state.revision}))
        }
        "QueryPlants" => {
            let position: V = serde_json::from_value(input["position"].clone())
                .map_err(|e| format!("position: {e}"))?;
            let radius = input["radius"].as_f64().ok_or("radius required")?;
            let limit = input["limit"]
                .as_u64()
                .filter(|n| *n <= 512)
                .ok_or("limit must be <=512")? as usize;
            if !position.iter().all(|v| v.is_finite())
                || length(position) < 1.
                || !radius.is_finite()
                || !(0.0..=1800.).contains(&radius)
            {
                return Err("invalid plant query".into());
            }
            let plants=vegetation::sites_near(position,radius,limit).into_iter().map(|s|{let p=state.plant(s);let tree=state.tree(s);json!({"site_id":s.id,"position":s.origin,"up":s.normal,"profile":vegetation::profiles::profile(s.profile).name,"age_years":p.age_years,"alive":p.alive,"height":tree.as_ref().map_or(0.,|t|t.height),"crown_radius":tree.as_ref().map_or(0.,|t|t.crown_radius),"broken_branches":p.broken_branches})}).collect::<Vec<_>>();
            Ok(json!({"revision":state.revision,"plants":plants}))
        }
        "AdvanceGrowth" => {
            let position: V = serde_json::from_value(input["position"].clone())
                .map_err(|e| format!("position: {e}"))?;
            let radius = input["radius"].as_f64().ok_or("radius required")?;
            if !position.iter().all(|v| v.is_finite())
                || length(position) < 1.
                || !radius.is_finite()
                || !(0.0..=1800.).contains(&radius)
            {
                return Err("invalid growth region".into());
            }
            let years = input["years"]
                .as_f64()
                .ok_or("explicit ecology years required")?;
            let sites = vegetation::sites_near(position, radius, 512);
            state.grow(&sites, years)?;
            Ok(json!({"revision":state.revision,"individuals":sites.len(),"years":years}))
        }
        "ApplyDamage" => {
            let prepare = if input["prepare"].is_null() {
                false
            } else {
                input["prepare"]
                    .as_bool()
                    .ok_or("damage prepare must be boolean")?
            };
            if let Some(expected) = input.get("expected_revision") {
                if expected.as_u64() != Some(state.revision) {
                    return Err("vegetation changed since accepted damage plan".into());
                }
            }
            let site = site(input)?;
            let branch = if input["branch_id"].is_null() {
                None
            } else {
                Some(
                    input["branch_id"]
                        .as_u64()
                        .filter(|n| *n < vegetation::branch_count(site.profile) as u64)
                        .ok_or("invalid branch_id")? as u32,
                )
            };
            let tree = state.tree(site).ok_or("tree is already removed")?;
            // Preview exact structural transfer before changing living state.
            // The application/material owner accepts that transfer, then commits
            // against expected_revision. Neither phase creates physical material.
            let mut candidate = state.clone();
            candidate.damage(site, branch)?;
            let retained = candidate
                .tree(site)
                .map(|t| {
                    t.branches
                        .into_iter()
                        .map(|b| b.id)
                        .collect::<std::collections::BTreeSet<_>>()
                })
                .unwrap_or_default();
            let detached:Vec<_>=tree.branches.iter().filter(|b|!retained.contains(&b.id)).map(|b|json!({"branch_id":b.id,"parent":b.parent,"start":tree.world(b.start),"end":tree.world(b.end),"radius":b.radius,"end_radius":b.radius*if b.order==0{0.77}else{0.36}})).collect();
            let planned_revision = candidate.revision;
            if !prepare {
                *state = candidate;
            }
            Ok(
                json!({"revision":state.revision,"planned_revision":planned_revision,"state_committed":!prepare,"site_id":site.id,"detached_structure":detached,"physical_material_created":false}),
            )
        }
        _ => Err("unknown vegetation hook".into()),
    }
}

pub(crate) fn query_wood(state:&vegetation::State,input:&Value)->Result<Value,String> {
            let position: V = serde_json::from_value(input["position"].clone())
                .map_err(|_| "wood position required")?;
            let radius = input["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && (0.0..=1800.).contains(r))
                .ok_or("bounded wood radius required")?;
            let time = input["time"]
                .as_f64()
                .filter(|t| t.is_finite())
                .unwrap_or(0.);
            if position.iter().any(|n| !n.is_finite()) || length(position) < 1. {
                return Err("invalid wood query".into());
            }
            let mut wood = Vec::new();
            for site in vegetation::sites_near(position, radius, vegetation::WOOD_SITE_LIMIT) {
                if let Some(tree) = state.tree(site) {
                    for branch in &tree.branches {
                        wood.push(json!({"site_id":site.id,"branch_id":branch.id,"start":sub(tree.world(tree.wind(branch.start,time)),position),"end":sub(tree.world(tree.wind(branch.end,time)),position),"radius":branch.radius,"end_radius":branch.radius*if branch.order==0{0.77}else{0.36}}));
                    }
                }
            }
            if wood.len()>vegetation::wood_segment_limit(){return Err("authored wood query exceeds its shared branch bound".into());}
            Ok(json!({"revision":state.revision,"origin":position,"wood":wood}))
}
