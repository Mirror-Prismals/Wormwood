//! Bounded numeric vegetation. Cache rest shapes, sample wind once per tree,
//! and spend separate budgets on reachable trees and whole hillsides.
use crate::vegetation::{
    self, State,
    geometry::{self, Detail},
    math::*,
    packet::Geometry,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};
struct CachedTree {
    tree: vegetation::Tree,
    mesh: geometry::Mesh,
}
#[derive(Default)]
struct Cache {
    anchor: Option<[i64; 3]>,
    sites: Vec<vegetation::Site>,
    revision: u64,
    season: u64,
    meshes: HashMap<(u64, u8), Arc<CachedTree>>,
    forest: vegetation::forest::Forest,
    understory: vegetation::understory::Understory,
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache::default());}
pub(crate) fn frame(state: &State, input: &Value) -> Result<Value, String> {
    frame_numeric(state, input).map(Geometry::into_json)
}
pub(crate) fn frame_numeric(state: &State, input: &Value) -> Result<Geometry, String> {
    let view = vegetation::forest::View::from_input(input)?;
    let camera = view.eye;
    let time = input["time"].as_f64().unwrap_or(0.);
    let range = input["max_distance"].as_f64().unwrap_or(6000.);
    let budget = input["max_triangles"].as_u64().unwrap_or(16384).min(32768) as usize;
    if !time.is_finite()
        || time.abs() > 1e9
        || !range.is_finite()
        || !(1. ..=6000.).contains(&range)
        || budget < 64
        || camera.iter().any(|v| v.abs() > 1e9)
    {
        return Err("invalid vegetation frame bounds".into());
    }
    CACHE.with(|c| draw(&mut c.borrow_mut(), state, input, view, time, range, budget))
}
fn draw(
    cache: &mut Cache,
    state: &State,
    input: &Value,
    view: vegetation::forest::View,
    time: f64,
    range: f64,
    budget: usize,
) -> Result<Geometry, String> {
    let camera = view.eye;
    let anchor = camera.map(|v| (v / 24.).floor() as i64);
    if cache.anchor != Some(anchor) {
        // Only branch/leaf geometry needs the individual-site search.
        // Distant habitat uses a separate bounded hierarchy.
        cache.sites = vegetation::sites_near(camera, 200., 2048);
        cache.anchor = Some(anchor);
    }
    let season = input["calendar"]["day_of_year"]
        .as_f64()
        .unwrap_or(180.)
        .to_bits()
        ^ input["calendar"]["year"].as_u64().unwrap_or(0);
    if cache.revision != state.revision || cache.season != season {
        cache.meshes.clear();
        cache.revision = state.revision;
        cache.season = season;
    }
    let mut sites = cache.sites.clone();
    // Keep explicit saved individuals discoverable after habitat changes.
    let mut known: HashSet<_> = sites.iter().map(|s| s.id).collect();
    for id in state.plants.keys() {
        if !known.contains(id) {
            if let Some(s) = vegetation::site_by_id(*id) {
                if length(sub(s.origin, camera)) < 180. {
                    sites.push(s);
                    known.insert(*id);
                }
            }
        }
    }
    sites.retain(|s| {
        length(sub(s.origin, camera)) < range.min(160.)
            && view.visible(add(s.origin, mul(s.normal, 12.)), 28.)
            && !state.materialized.contains(&s.id)
            && !state.reserved(*s)
    });
    sites.sort_by(|a, b| {
        length(sub(a.origin, camera))
            .total_cmp(&length(sub(b.origin, camera)))
            .then(a.id.cmp(&b.id))
    });
    if cache.meshes.len() > 384 {
        let keep: HashSet<_> = sites.iter().map(|s| s.id).collect();
        cache.meshes.retain(|(id, _), _| keep.contains(id));
    }
    let mut out = Geometry {
        origin: camera,
        revision: state.revision,
        vertices: Vec::new(),
        indices: Vec::new(),
        metadata: json!({}),
    };
    let mut replaced = HashSet::new();
    let mut instances = Vec::new();
    let mut built = 0;
    // Share the existing packet bound: ground flora cannot grow GPU buffers.
    let understory_budget = budget / 4;
    cache.understory.append(state, input, &view, range, understory_budget, &mut out);
    let understory_triangles = out.indices.len() / 3;
    let detailed_budget = budget / 2 + understory_triangles;
    for site in sites {
        let available = detailed_budget.saturating_sub(out.indices.len() / 3);
        // The distant envelope remains present if no detailed mesh fits.
        // Do this check BEFORE generating an entire branch graph.
        if available < 64 {
            break;
        }
        if !state.plant(site).alive {
            continue;
        }
        let distance = length(sub(site.origin, camera));
        let preferred = if distance < 24. {
            Detail::Near
        } else if distance < 64. {
            Detail::Middle
        } else {
            Detail::Far
        };
        let baked_fallback=!cfg!(target_os="ios") && (2..6).contains(&site.profile)
            && state.plant(site).broken_branches.is_empty()
            && state.mechanics.rooted_tree(site.id).is_none();
        let mut selected = None;
        for detail in [preferred, Detail::Middle, Detail::Far] {
            // Healthy authored species use their baked real silhouette when
            // detailed branches no longer fit. Do not insert the inflated
            // six-spray Far crown between the real tree and that silhouette.
            if detail==Detail::Far && baked_fallback { break; }
            if preferred == Detail::Far && detail == Detail::Middle {
                continue;
            }
            let key = (
                site.id,
                match detail {
                    Detail::Near => 0,
                    Detail::Middle => 1,
                    Detail::Far => 2,
                },
            );
            // Mechanically moving rooted trees cannot reuse a rest frame.
            let moving = state.mechanics.rooted_tree(site.id).is_some();
            if moving {
                cache.meshes.remove(&key);
            }
            if !cache.meshes.contains_key(&key) {
                let Some(tree) = state.tree(site) else {
                    break;
                };
                let season = vegetation::season::Foliage::for_profile(input, site.id, vegetation::profiles::profile(site.profile));
                let mut profile = vegetation::profiles::profile(site.profile);
                profile.leaf_color = season.color;
                let mut mesh = geometry::build_seasonal(&tree, profile, detail, 0., season.cover);
                // The analytic bend leaves y unchanged, so undoing its
                // time-zero displacement recovers reusable rest positions.
                for v in &mut mesh.vertices {
                    v.position = sub(v.position, sub(tree.wind(v.position, 0.), v.position));
                }
                cache
                    .meshes
                    .insert(key, Arc::new(CachedTree { tree, mesh }));
                built += 1;
            }
            let cached = cache.meshes[&key].clone();
            if cached.mesh.triangles() <= available {
                selected = Some((cached, detail));
                break;
            }
            if detail == Detail::Far {
                break;
            }
        }
        let Some((cached, detail)) = selected else {
            continue;
        };
        let tree = &cached.tree;
        let mesh = &cached.mesh;
        let profile = vegetation::profiles::profile(site.profile);
        let (e, n) = basis(tree.site.normal);
        let axes = tree.frame.unwrap_or([e, tree.site.normal, n]);
        let direction = |v: V| {
            add(
                add(mul(axes[0], v[0]), mul(axes[1], v[1])),
                mul(axes[2], v[2]),
            )
        };
        let shift = sub(tree.site.origin, camera);
        let phase = random(site.id, 77) * std::f64::consts::TAU;
        let gust = (time * 0.83 + phase).sin() * 0.65 + (time * 1.71 + phase * 0.7).sin() * 0.35;
        let wind = [0.009 * gust, 0., 0.004 * (time * 0.63 + phase).sin()];
        let start = out.indices.len();
        let offset = out.vertices.len() as u32;
        for v in &mesh.vertices {
            let mut p = v.position;
            let mut normal = v.normal;
            if tree.frame.is_none() {
                let t = (p[1] / tree.height.max(0.1)).clamp(0., 1.);
                p = add(p, mul(wind, tree.height * t * t));
                if v.position[1] > 0. && v.position[1] < tree.height.max(0.1) {
                    normal = unit([
                        normal[0],
                        normal[1] - 2. * t * (wind[0] * normal[0] + wind[2] * normal[2]),
                        normal[2],
                    ]);
                }
            }
            let p = add(shift, direction(p));
            let normal = direction(normal);
            out.vertices.push([
                p[0] as f32,
                p[1] as f32,
                p[2] as f32,
                normal[0] as f32,
                normal[1] as f32,
                normal[2] as f32,
                v.uv[0],
                v.uv[1],
                v.color[0],
                v.color[1],
                v.color[2],
                v.material as f32,
                v.transmission,
                v.roughness,
                v.occlusion,
                (profile.bark_ridge_scale + 1000. * profile.bark_tile() as f64) as f32,
            ]);
        }
        out.indices.extend(mesh.indices.iter().map(|i| i + offset));
        replaced.insert(site.id);
        instances.push(json!({"site_id":site.id,"profile":profile.name,"index_start":start,"index_count":mesh.indices.len(),"detail":format!("{detail:?}"),"height":tree.height,"crown_radius":tree.crown_radius}));
    }
    let detailed = out.indices.len() / 3;
    cache.forest.append(
        state,
        input,
        &view,
        range,
        budget - detailed,
        &replaced,
        &mut out,
    )?;
    out.metadata["instances"] = json!(instances);
    out.metadata["time"] = json!(time);
    out.metadata["triangles"] = json!(out.indices.len() / 3);
    out.metadata["triangle_budget"] = json!(budget);
    out.metadata["detailed_triangles"] = json!(detailed - understory_triangles);
    out.metadata["rest_meshes_built"] = json!(built);
    out.metadata["cached_rest_meshes"] = json!(cache.meshes.len());
    Ok(out)
}
