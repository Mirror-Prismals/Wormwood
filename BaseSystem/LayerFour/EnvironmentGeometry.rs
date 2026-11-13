//! Appearance of the material world. Landforms describe occupied volume; the
//! shared matter field supplies turf, soil, sediment and rock. No atlas terrain.
use crate::{
    physics::matter::world_field::{self, Field, Substance},
    terrain,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, sync::{Arc,mpsc}};
type V = [f64; 3];
type Vertex = [f32; 6];
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
fn unit(p: V) -> V {
    let l = dot(p, p).sqrt().max(1e-12);
    p.map(|v| v / l)
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|k| a[k] - b[k])
}
fn vector(v: &Value, k: &str) -> V {
    std::array::from_fn(|i| v[k][i].as_f64().unwrap())
}
fn basis(n: V) -> (V, V) {
    let e = unit(cross(
        if n[1].abs() > 0.95 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        },
        n,
    ));
    (e, cross(n, e))
}
#[derive(Clone)]
struct Block {
    start: usize,
    end: usize,
    center: V,
    radius: f64,
}
#[derive(Clone)]
struct Geometry {
    origin: V,
    anchor: [u64; 3],
    lod: i32,
    edits: Value,
    underground: Value,
    cave_window: [i64; 3],
    vertices: Vec<Vertex>,
    normals: Vec<V>,
    rocks: Vec<i8>,
    blocks: Vec<Block>,
}
#[derive(Clone)]
struct Distant {
    geometry: Arc<Geometry>,
    boundary: Vec<[f64; 6]>,
}
#[derive(Clone, Copy)]
struct MaterialSample {
    kind: Substance,
    color: [f32; 3],
    original_surface: bool,
    normal: V,
}
fn material_sample(point: V) -> MaterialSample {
    let kind = world_field::substance(point);
    let up = unit(point);
    let depth = terrain::RADIUS + terrain::height(up) - dot(point, point).sqrt();
    let normal = terrain::surface_normal(up);
    let color = if kind == Substance::Turf
        && dot(normal, up).abs() < 0.65
    {
        world_field::ROOT_COLOR
    } else {
        kind.color_at(point)
    };
    MaterialSample { kind, color, original_surface: depth.abs() < 0.001, normal }
}
#[derive(Default,Clone)]
struct Cache {
    near: Option<Arc<Geometry>>,
    unmined: Option<Arc<Geometry>>,
    distant: Option<Distant>,
    heading: Option<V>,
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache::default());}
// These are separate accepted geometry domains. Entering a cave must not
// discard the exterior mesh that can be reused on return (and vice versa).
// Each slot remains bounded to the existing near/unmined/distant cache.
thread_local! {
    static CAVE_CACHE:RefCell<Cache>=RefCell::new(Cache::default());
    static NIGHT_CACHE:RefCell<Cache>=RefCell::new(Cache::default());
    static NIGHT_CAVE_CACHE:RefCell<Cache>=RefCell::new(Cache::default());
}
fn geometry_cache(camera:V,deep:bool)->&'static std::thread::LocalKey<RefCell<Cache>> {
    match (terrain::nightworld::active(camera),deep) {
        (false,false)=>&CACHE,(false,true)=>&CAVE_CACHE,
        (true,false)=>&NIGHT_CACHE,(true,true)=>&NIGHT_CAVE_CACHE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn landform_point(x:f64,z:f64)->V {
        let n=unit([x,terrain::RADIUS,z]);
        n.map(|v|v*(terrain::RADIUS+terrain::height(n)))
    }
    #[test]
    fn lod_terrain_material_keeps_shared_strata_colors_at_all_resolutions() {
        let surface=json!({"edits":[],"debris":[],"underground":null});
        let p=landform_point(0.,0.);
        for edge in [0.5,4.,8.,32.] {
            let mut b=landform_point(edge,0.);b[1]-=0.25;
            let mut c=landform_point(0.,edge);c[1]-=0.25;
            assert_eq!(world_field::substance(p),Substance::Turf);
            assert_eq!(world_field::substance(b),Substance::Dirt);
            assert_eq!(world_field::substance(c),Substance::Dirt);
            let tri=[p,b,c].map(|p|[p[0],p[1],p[2],1.,1.,1.]);
            let g=finish(&surface,p,vec![tri],None,false);
            assert_eq!(&g.vertices[0][3..],&world_field::TURF_COLOR,
                "a shared turf point cannot inherit its neighbors' dirt as face size changes");
            assert_eq!(&g.vertices[1][3..],&Substance::Dirt.color());
            assert_eq!(&g.vertices[2][3..],&Substance::Dirt.color());
        }
    }
    #[test]
    fn original_surface_lighting_is_independent_of_face_size() {
        let surface=json!({"edits":[],"debris":[],"underground":null});
        let p=landform_point(0.,0.);
        let expected=terrain::surface_normal(unit(p));
        for edge in [0.5,4.,32.,128.] {
            let tri=[p,landform_point(edge,0.),landform_point(0.,edge)]
                .map(|p|[p[0],p[1],p[2],1.,1.,1.]);
            let g=finish(&surface,p,vec![tri],None,false);
            assert_eq!(g.normals[0],expected);
        }
    }

    #[test]
    fn coarse_landform_preserves_the_detailed_ground_cover() {
        let underground:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
        let surface=json!({"edits":[],"debris":[],"underground":underground});
        let field=Field::from_surface(&surface).unwrap();
        let cave=field.caves.as_ref().unwrap();
        let mesh=cave.mesh([0.,terrain::RADIUS+2.,0.],&[]).unwrap();
        let point=mesh.iter().flatten().map(|v|[v[0],v[1],v[2]])
            .find(|p|p[0].abs()<12. && p[2].abs()<12. &&
                world_field::substance(*p)==Substance::Dirt &&
                (terrain::height(unit(*p))-terrain::underground::local(*p)[1]).abs()<0.5).unwrap();
        let up=unit(point);
        let p=up.map(|v|v*(terrain::RADIUS+terrain::height(up)));
        let expected=terrain::surface_cover::Sampler::new(cave).unwrap().color(p).unwrap();
        assert!(expected[0]>expected[1],"exercise a dirt patch on the otherwise grassy landform");
        for edge in [0.5,4.,8.,32.] {
            let tri=[p,landform_point(p[0]+edge,p[2]),landform_point(p[0],p[2]+edge)]
                .map(|p|[p[0],p[1],p[2],1.,1.,1.]);
            let g=finish(&surface,p,vec![tri],None,false);
            assert_eq!(g.vertices[0][3..],expected,"dirt must survive coarse geometry");
        }
    }

    #[test]
    fn distant_rings_include_dirt_without_loading_nearby_geometry() {
        let underground:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
        let surface=json!({"edits":[],"debris":[],"underground":underground});
        let camera=[0.,terrain::RADIUS+1.8,0.];
        let start=std::time::Instant::now();
        let distant=build_distant(&surface,camera,[0.,1.,0.],0,None).unwrap();
        let mut dirt=0;let mut grass=0;
        for v in &distant.geometry.vertices {
            if (v[0] as f64).hypot(v[2] as f64)>320. {continue;}
            if v[3]>v[4] {dirt+=1;} else {grass+=1;}
        }
        assert!(dirt>0 && grass>0,"distant rings must retain both ground materials: {dirt}/{grass}");
        eprintln!("cold distant material build: {:?}; dirt/grass vertices within 320 m: {dirt}/{grass}",start.elapsed());
    }

    #[test]
    fn lod_terrain_material_keeps_exposed_turf_wall_as_roots() {
        let surface=json!({"edits":[],"debris":[],"underground":null});
        let p=landform_point(0.,0.);
        let points=[[p[0],p[1]-0.02,p[2]],
            [p[0]+0.04,p[1]-0.02,p[2]],
            [p[0]+0.02,p[1]-0.06,p[2]]];
        assert!(points.iter().all(|p|world_field::substance(*p)==Substance::Turf));
        let tri=points.map(|p|[p[0],p[1],p[2],1.,1.,1.]);
        let g=finish(&surface,p,vec![tri],None,false);
        assert!(g.vertices.iter().all(|v|v[3..]==world_field::ROOT_COLOR));
    }
    #[test]
    fn forward_cone_preserves_support_and_horizon_with_ordered_rings() {
        let up=[0.,1.,0.];
        let forward=[0.,0.,-1.];
        for r in [0.,0.5,4.,8.,2048.,8192.] {
            assert_eq!(detail_ring(up,r,64,Some(forward)),ring(up,r,64));
        }
        assert_eq!(cone_radius(128.,1.),256.);
        assert_eq!(cone_radius(128.,-1.),128.);
        for alignment in [-1.,0.,0.2,0.5,0.9,1.] {
            let mut previous=0.;
            for r in 1..=4096 {
                let next=cone_radius(r as f64,alignment);
                assert!(next>previous,"rings must never cross");
                previous=next;
            }
        }
    }

    #[test]
    fn actual_camera_axes_drive_cone_and_turns_invalidate_cached_terrain() {
        let y=terrain::RADIUS+terrain::height([0.,1.,0.])+1.8;
        let mut view=json!({"position":[0.,y,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"forward":[1.,0.,0.]});
        let surface=json!({"edits":[],"debris":[],"underground":null});
        planet(&view,&surface,None).unwrap();
        let first=CACHE.with(|c| {
            let c=c.borrow();
            assert_eq!(c.heading,Some([1.,0.,0.]));
            c.distant.as_ref().unwrap().boundary.clone()
        });
        view["forward"]=json!([-1.,0.,0.]);
        planet(&view,&surface,None).unwrap();
        CACHE.with(|c| {
            let c=c.borrow();
            let far=c.distant.as_ref().unwrap();
            assert_ne!(first,far.boundary);
            // Near and distant meshes share exact world-space seam vertices.
            let near=c.unmined.as_ref().unwrap();
            for point in &far.boundary {
                assert!(near.vertices.iter().any(|v| (0..3).all(|k|
                    (v[k] as f64-(point[k]-near.origin[k])).abs()<0.001)));
            }
        });
        view["forward"]=json!([0.,1.,0.]);
        planet(&view,&surface,None).unwrap();
        CACHE.with(|c|assert_eq!(c.borrow().heading,Some([-1.,0.,0.])));
    }

    #[test]
    fn deep_cave_view_has_rock_and_dark_boundary_without_exterior_turf() {
        let eye=terrain::underground::world([-51.,-44.,16.]);
        let view=json!({"position":eye,"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}});
        let caves:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
        let surface=json!({"edits":[],"debris":[],"underground":caves});
        let triangles=planet(&view,&surface,None).unwrap();
        assert!(triangles.iter().any(|t|t[15]<=-4096.));
        assert_eq!(triangles.iter().filter(|t|t[2]==0.99999 && t[6]==0.99999 && t[10]==0.99999).count(),2);
        assert!(triangles.iter().all(|t|t[13]<=t[12]*1.25 || t[13]<=t[14]*1.25),"outdoor green turf must not appear through the bounded deep-cave mesh");
    }

    #[test]
    fn ground_detail_extends_beyond_the_old_eighteen_metre_cutoff() {
        let y = terrain::RADIUS + terrain::height([0., 1., 0.]) + 1.8;
        let view = json!({"position":[0.,y,0.],"yaw":0.,"pitch":-0.1,"aspect":1.6});
        let surface = json!({"edits":[],"debris":[],"underground":null});
        let triangles = planet(&view, &surface, None).unwrap();
        assert!(triangles.iter().any(|t|
            (0..3).all(|i| t[i*4+3] > 20. && t[i*4+3] < 40.) &&
            (0..3).all(|i| t[15+i*4] >= 4096.)
        ));
    }

    #[test]
    fn numeric_material_skin_projects_identically_to_json() {
        let y = terrain::RADIUS + terrain::height([0., 1., 0.]) + 1.8;
        let view = json!({"position":[0.,y,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}});
        let skin = vec![
            [-0.2, y - 0.2, -1., 0.4, 0.3, 0.2],
            [0.2, y - 0.2, -1., 0.4, 0.3, 0.2],
            [0., y + 0.2, -1., 0.4, 0.3, 0.2],
        ];
        let rest = json!({"edits":[],"debris":[],"underground":null});
        let mut legacy = rest.clone();
        legacy["debris"] = json!(skin);
        let a = planet(&view, &rest, Some(&skin)).unwrap();
        let b = planet(&view, &legacy, None).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), planet(&view, &rest, None).unwrap().len() + 1);
    }
}

// A loading transition may change the radial chart while accepted edits are
// identical. Neither an old chart nor a cave-only mesh can stand in for the
// destination surface while a background geometry job finishes.
fn accepted_geometry(g:&Geometry,surface:&Value,camera:V,deep:bool)->bool {
    g.edits==surface["edits"] && g.underground==surface["underground"]
        && terrain::nightworld::active(g.origin)==terrain::nightworld::active(camera)
        && (g.lod == -1)==deep
}
pub(super) fn planet(
    view: &Value,
    surface: &Value,
    material: Option<&[[f64; 6]]>,
) -> Result<Vec<[f32; 24]>, String> {
    let camera = vector(view, "position");
    let deep = terrain::underground::deep_view(camera) && !surface["underground"].is_null();
    let distance = dot(camera, camera).sqrt();
    let altitude = (distance - terrain::RADIUS - terrain::height(unit(camera))).max(0.);
    let lod = if altitude < 35. {
        (altitude / 8.) as i32
    } else if altitude < 100. {
        5 + ((altitude - 35.) / 32.) as i32
    } else {
        8 + (altitude / 100.).ln().div_euclid(1.35f64.ln()) as i32
    };
    let lod=if deep {-1} else {lod};
    let anchor = terrain::mesh_anchor(camera).map(f64::to_bits);
    geometry_cache(camera,deep).with(|c| {
        let mut cache = c.borrow_mut();
        let asynchronous=cfg!(target_os="macos")&&!cfg!(test)
            &&std::env::var("WORMWOOD_GAME_GEOMETRY_SYNC").as_deref()!=Ok("1");
        let accepted=cache.near.as_ref().is_some_and(|g|accepted_geometry(g,surface,camera,deep));
        if asynchronous&&accepted {
            // Keep projecting accepted geometry with the CURRENT camera while
            // its next complete near/distant mesh is built. Never hold a camera
            // frame hostage to a view-cone or two-metre support-ring rebuild.
            stream_geometry(&mut cache,view,surface,camera,altitude,lod,anchor,deep)?;
        }else{
            prepare_cache(&mut cache,view,surface,camera,altitude,lod,anchor,deep)?;
        }
        let g = cache.near.as_deref().unwrap();
        let lighting = crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
        let horizon = crate::base_system::layer_four::atmosphere::palette(lighting.fraction, 1.);
        let capacity=(g.vertices.len()+cache.distant.as_ref().map_or(0,|d|d.geometry.vertices.len())+material.map_or(0,|m|m.len()))/3;
        let mut local = view.clone();
        local["position"] = json!([0, 0, 0]);
        local["far"] = json!(distance + terrain::RADIUS * 2.);
        local["near"] = json!(0.05);
        local["two_sided"] = json!(true);
        let projector=super::projection::Projector::new(&local);
        let geometries:Vec<_>=std::iter::once(cache.near.as_ref().unwrap().clone())
            .chain(cache.distant.as_ref().map(|d|d.geometry.clone())).collect();
        let mut out=project_parallel(geometries,view,camera,deep)?;
        let append_material=|v:[f64;6]| {
            let mut p=[0.;6];
            for k in 0..3 {p[k]=(v[k]-camera[k]) as f32;p[3+k]=v[3+k] as f32;}
            p
        };
        let mut emit_material=|tri:[Vertex;3]| {
            let mut face=projector.triangle(&tri).unwrap();
            if !in_frustum(&face) {return;}
            for (i,p) in tri.iter().enumerate() {
                let distance=p[..3].iter().map(|v|(*v as f64).powi(2)).sum::<f64>().sqrt();
                let fog=(1.-(-distance/15_000.).exp()).min(0.82);
                for k in 0..3 {
                    face[12+i*4+k]=(p[3+k] as f64*lighting.tint[k]*lighting.brightness()*(1.-fog)+horizon[k]*fog) as f32;
                }
            }
            out.push(face);
        };
        if let Some(material)=material {
            for tri in material.chunks_exact(3) {emit_material(std::array::from_fn(|i|append_material(tri[i])));}
        } else {
            if let Some(rows)=surface["debris"].as_array() {
                for tri in rows.chunks_exact(3) {emit_material(std::array::from_fn(|i|append_material(std::array::from_fn(|k|tri[i][k].as_f64().unwrap()))));}
            }
        }
        if deep {
            for xy in [[[-1.,-1.],[1.,-1.],[1.,1.]],[[-1.,-1.],[1.,1.],[-1.,1.]]] {
                let mut face=[0.;24];
                for i in 0..3 {
                    face[i*4..i*4+4].copy_from_slice(&[xy[i][0],xy[i][1],0.99999,1.]);
                    face[12+i*4..16+i*4].copy_from_slice(&if view["diagnostic_cave_fullbright"]==true{[0.16,0.16,0.16,1.]}else{[0.004,0.008,0.016,1.]});
                }
                out.push(face);
            }
        }
        out.retain(in_frustum);
        Ok(out)
    })
}
fn pack_terrain_material(face:&mut [f32;24],tri:&[Vertex;3],rock:i8,camera:V) {
            if tri.iter().any(|v| v[..3].iter().any(|x|x.abs() > 512.)) {return;}
            let points: [V;3] = std::array::from_fn(|i|[tri[i][0] as f64,tri[i][1] as f64,tri[i][2] as f64]);
            let n = cross(sub(points[1],points[0]),sub(points[2],points[0]));
            let axis = (0..3).max_by(|a,b| n[*a].abs().total_cmp(&n[*b].abs())).unwrap();
            let axes = [(axis+1)%3,(axis+2)%3];
            // Rock uses a signed 24-bit word: 20 UV bits + 12 material/frame tags.
            // Wrap the whole face at a shared world-space 2 m material period.
            let scale=if rock>=0 {64.} else {128.};
            let limit=if rock>=0 {1024.} else {2048.};
            let base = axes.map(|a|((camera[a]+points[0][a])/4.).floor()*4.-4.);
            let mut packed = [0.;3];
            for (i,p) in points.iter().enumerate() {
                let uv = [0,1].map(|k|((camera[axes[k]]-base[k]+p[axes[k]])*scale).round());
                if uv.iter().any(|x| !(0.0..limit).contains(x)) {break;}
                packed[i] = if rock>=0 {
                    let tag=rock as usize*6+axis*2+usize::from(n[axis]<0.);
                    -(4096.+uv[0]+1024.*uv[1]+1048576.*tag as f64) as f32
                } else {(4096. + uv[0] + 2048.*uv[1]) as f32};
            }
            if packed[2] != 0. {for i in 0..3 {face[15+i*4] = packed[i];}}
}

#[cfg(test)]
fn reference_planet(
    view: &Value,
    surface: &Value,
    material: Option<&[[f64; 6]]>,
) -> Result<Vec<[f32; 24]>, String> {
    let camera = vector(view, "position");
    let deep = terrain::underground::deep_view(camera) && !surface["underground"].is_null();
    let distance = dot(camera, camera).sqrt();
    let altitude = (distance - terrain::RADIUS - terrain::height(unit(camera))).max(0.);
    let lod = if altitude < 35. {
        (altitude / 8.) as i32
    } else if altitude < 100. {
        5 + ((altitude - 35.) / 32.) as i32
    } else {
        8 + (altitude / 100.).ln().div_euclid(1.35f64.ln()) as i32
    };
    let lod=if deep {-1} else {lod};
    let anchor = terrain::mesh_anchor(camera).map(f64::to_bits);
    CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        // Retain the cone through small turns; rebuild after about 11 degrees.
        // Looking vertically retains the last useful tangent heading.
        let requested = if !deep && altitude < 100. { terrain_heading(view, camera) } else { None };
        let heading = match (cache.heading, requested) {
            (Some(old), Some(new)) if dot(old, new) > 0.98 => Some(old),
            (_, Some(new)) => Some(new),
            (old, None) if !deep && altitude < 100. => old,
            _ => None,
        };
        let heading_changed = heading != cache.heading;
        cache.heading = heading;
        // Hills remain fixed while the precise two-metre support patch follows
        // the player. Only a short connecting annulus changes between them.
        let mut distant_changed = false;
        if !deep && altitude < 100. {
            let mut up = distant_anchor(camera);
            let near_up = terrain::mesh_anchor(camera);
            // The legacy local tangent basis changes reference axis near the
            // poles. Keep both seam rings on the same side of that boundary.
            if dot(basis(up).0, basis(near_up).0) < 0.99 {
                up = near_up;
            }
            let moved = heading_changed || cache.distant.as_ref().is_none_or(|d|
                d.geometry.anchor != up.map(f64::to_bits) || d.geometry.lod != lod);
            if moved || cache.near.as_ref().is_none_or(|g| g.edits != surface["edits"]) {
                // Cuts entirely inside the inner ring cannot alter distant triangles.
                let mut distant_surface = surface.clone();
                let center = up.map(|x| x * (terrain::RADIUS + terrain::height(up)));
                if let Some(edits) = distant_surface["edits"].as_array_mut() {
                    edits.retain(|e| terrain::material_domain::Domain::parse(e)
                        .is_none_or(|d| !d.mined || d.distance(center) > 68.));
                }
                if moved || cache.distant.as_ref().is_none_or(|d|d.geometry.edits != distant_surface["edits"]) {
                    cache.distant = Some(build_distant(&distant_surface, camera, up, lod, heading)?);
                    distant_changed = true;
                }
            }
        } else if cache.distant.take().is_some() {
            distant_changed = true;
        }
        if distant_changed
            || cache.near.as_ref().is_none_or(|g| {
                g.anchor != anchor
                    || g.lod != lod
                    || g.edits != surface["edits"]
                    || g.underground != surface["underground"]
                    || g.cave_window != terrain::underground::mesh_window(camera)
            })
        {
            let mut rest = surface.clone();
            let cuts: Vec<_> = surface["edits"].as_array().into_iter().flatten()
                .filter(|e| e["mined"] == true).cloned().collect();
            if let Some(edits) = rest["edits"].as_array_mut() { edits.retain(|e| e["mined"] != true); }
            if rest["underground"].is_object() {
                rest["underground"]["material_domains"] = rest["edits"].clone();
            }
            if distant_changed || cache.unmined.as_ref().is_none_or(|g| g.anchor != anchor ||
                g.lod != lod || g.edits != rest["edits"] || g.underground != rest["underground"] ||
                g.cave_window != terrain::underground::mesh_window(camera)) {
                let mut base = build(&rest, camera, altitude, cache.distant.as_ref(), heading)?;
                base.anchor = anchor;
                base.lod = lod;
                cache.unmined = Some(Arc::new(base));
            }
            let base = cache.unmined.as_ref().unwrap();
            let mut g = if cuts.is_empty() {base.as_ref().clone()} else { cut_geometry(base, &cuts, &rest)? };
            g.edits = surface["edits"].clone();
            g.underground = surface["underground"].clone();
            cache.near = Some(Arc::new(g));
        }
        let g = cache.near.as_deref().unwrap();
        let lighting = crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
        let horizon = crate::base_system::layer_four::atmosphere::palette(lighting.fraction, 1.);
        let mut rocks = Vec::new();
        let mut vertices = Vec::with_capacity(
            g.vertices.len()
                + cache
                    .distant
                    .as_ref()
                    .map_or(0, |d| d.geometry.vertices.len())
                + material.map_or_else(
                    || surface["debris"].as_array().map_or(0, Vec::len),
                    |m| m.len(),
                ),
        );
        let fullbright=view["diagnostic_cave_fullbright"]==true;
        let lit = |mut v: Vertex, n: V| {
            // Review-only unlit albedo: preserve geometry, textures and culling.
            if fullbright {return v;}
            let light = 0.30 + 0.70 * dot(n, lighting.primary).abs();
            let distance = v[..3]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            // The finite underground mesh ends in darkness, not an outdoor sky.
            let fog = if deep {let t=((distance-24.)/10.).clamp(0.,1.);t*t*(3.-2.*t)}
                else {(1. - (-distance / 15_000.).exp()).min(0.82)};
            let exposure=if deep {0.}else{terrain::underground::sky_exposure(std::array::from_fn(|i|camera[i]+v[i] as f64))};
            for k in 0..3 {
                v[3 + k] = (v[3 + k] as f64
                    * light
                    * lighting.tint[k]
                    * lighting.brightness()
                    * exposure
                    * (1. - fog)
                    + if deep {[0.004,0.008,0.016][k]*fog} else {horizon[k]*fog*exposure}) as f32;
            }
            v
        };
        let culler = crate::vegetation::forest::View::from_input(view)?;
        for g in std::iter::once(g).chain(cache.distant.as_ref().map(|d|d.geometry.as_ref())) {
            let shift = sub(g.origin, camera);
            for block in &g.blocks {
                let center = std::array::from_fn(|k| g.origin[k] + block.center[k]);
                if !culler.visible(center, block.radius) {
                    continue;
                }
                for i in block.start..block.end {
                    let mut v = g.vertices[i];
                    for k in 0..3 {
                        v[k] = (v[k] as f64 + shift[k]) as f32;
                    }
                    vertices.push(lit(v, g.normals[i]));
                    rocks.push(g.rocks[i]);
                }
            }
        }
        let terrain_vertices = vertices.len();
        // Accepted material geometry and tools share this pass on all backends.
        // Their workshop skin already carries local shape shading; keep it and
        // apply the same world time/fog as the resting material around it.
        let mut append_material = |v: [f64; 6]| {
            let mut p = [0.; 6];
            for k in 0..3 {
                p[k] = (v[k] - camera[k]) as f32;
                p[3 + k] = v[3 + k] as f32;
            }
            let distance = p[..3]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            let fog = (1. - (-distance / 15_000.).exp()).min(0.82);
            for k in 0..3 {
                p[3 + k] = (p[3 + k] as f64 * lighting.tint[k] * lighting.brightness() * (1. - fog)
                    + horizon[k] * fog) as f32;
            }
            vertices.push(p);
        };
        if let Some(material) = material {
            for &v in material {
                append_material(v);
            }
        } else {
            for v in surface["debris"].as_array().into_iter().flatten() {
                append_material(std::array::from_fn(|k| v[k].as_f64().unwrap()));
            }
        }
        let mut local = view.clone();
        local["position"] = json!([0, 0, 0]);
        local["far"] = json!(distance + terrain::RADIUS * 2.);
        local["near"] = json!(0.05);
        local["two_sided"] = json!(true);
        let mut out = super::projection::project(&vertices, &local);
        // Two-sided projection emits one triangle per input triangle. Nearby
        // terrain uses the spare alpha word for two fixed-point UV coordinates.
        for (face_index, (face, tri)) in out.iter_mut().zip(vertices[..terrain_vertices].chunks_exact(3)).enumerate() {
            let rock = rocks[face_index*3];
            if tri.iter().any(|v| v[..3].iter().any(|x|x.abs() > 512.)) {continue;}
            let points: [V;3] = std::array::from_fn(|i|[tri[i][0] as f64,tri[i][1] as f64,tri[i][2] as f64]);
            let n = cross(sub(points[1],points[0]),sub(points[2],points[0]));
            let axis = (0..3).max_by(|a,b| n[*a].abs().total_cmp(&n[*b].abs())).unwrap();
            let axes = [(axis+1)%3,(axis+2)%3];
            // Rock uses a signed 24-bit word: 20 UV bits + 12 material/frame tags.
            // Wrap the whole face at a shared world-space 2 m material period.
            let scale=if rock>=0 {64.} else {128.};
            let limit=if rock>=0 {1024.} else {2048.};
            let base = axes.map(|a|((camera[a]+points[0][a])/4.).floor()*4.-4.);
            let mut packed = [0.;3];
            for (i,p) in points.iter().enumerate() {
                let uv = [0,1].map(|k|((camera[axes[k]]-base[k]+p[axes[k]])*scale).round());
                if uv.iter().any(|x| !(0.0..limit).contains(x)) {break;}
                packed[i] = if rock>=0 {
                    let tag=rock as usize*6+axis*2+usize::from(n[axis]<0.);
                    -(4096.+uv[0]+1024.*uv[1]+1048576.*tag as f64) as f32
                } else {(4096. + uv[0] + 2048.*uv[1]) as f32};
            }
            if packed[2] != 0. {for i in 0..3 {face[15+i*4] = packed[i];}}
        }
        if deep {
            for xy in [[[-1.,-1.],[1.,-1.],[1.,1.]],[[-1.,-1.],[1.,1.],[-1.,1.]]] {
                let mut face=[0.;24];
                for i in 0..3 {
                    face[i*4..i*4+4].copy_from_slice(&[xy[i][0],xy[i][1],0.99999,1.]);
                    face[12+i*4..16+i*4].copy_from_slice(&if view["diagnostic_cave_fullbright"]==true{[0.16,0.16,0.16,1.]}else{[0.004,0.008,0.016,1.]});
                }
                out.push(face);
            }
        }
        out.retain(in_frustum);
        Ok(out)
    })
}
fn in_frustum(t: &[f32; 24]) -> bool {
    if (0..3).all(|i| t[i * 4 + 3] <= 0.) {
        return false;
    }
    for axis in 0..3 {
        if cfg!(target_os = "ios") && axis == 0 {
            continue;
        }
        if (0..3).all(|i| t[i * 4 + axis] > t[i * 4 + 3])
            || (0..3).all(|i| t[i * 4 + axis] < if axis == 2 { 0. } else { -t[i * 4 + 3] })
        {
            return false;
        }
    }
    true
}
fn push(g: &mut Geometry, points: [V; 3], color: [f32; 3], normal: Option<V>) {
    let n =
        normal.unwrap_or_else(|| unit(cross(sub(points[1], points[0]), sub(points[2], points[0]))));
    for p in points {
        let p = sub(p, g.origin);
        g.vertices.push([
            p[0] as f32,
            p[1] as f32,
            p[2] as f32,
            color[0],
            color[1],
            color[2],
        ]);
        g.normals.push(n);
        g.rocks.push(-1);
    }
}
fn distant_anchor(camera: V) -> V {
    let n = unit(camera);
    let axis = (0..3)
        .max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs()))
        .unwrap();
    let mut p = [0.; 3];
    p[axis] = n[axis].signum();
    for k in 0..3 {
        if k != axis {
            p[k] = (n[k] / n[axis].abs() * terrain::RADIUS / 32.).round() * 32. / terrain::RADIUS;
        }
    }
    unit(p)
}
/// Project the actual camera basis onto the planet tangent plane, including
/// security/workstation cameras that supply explicit axes instead of Euler angles.
fn terrain_heading(view: &Value, camera: V) -> Option<V> {
    let yaw = view["yaw"].as_f64().unwrap_or(0.);
    let pitch = view["pitch"].as_f64().unwrap_or(0.);
    let f: V = serde_json::from_value(view["forward"].clone()).unwrap_or([
        yaw.sin()*pitch.cos(), pitch.sin(), -yaw.cos()*pitch.cos(),
    ]);
    let up = unit(camera);
    let tangent = sub(f, up.map(|x| x*dot(f,up)));
    (dot(tangent,tangent) > 0.01).then(|| unit(tangent))
}
/// Smooth broad cone (full strength through 30 degrees, tapering to 90).
/// The radial mapping is monotone, fixes the support patch and rejoins the
/// original horizon mesh at 2048 m. Both sides of the near/far seam use it.
fn cone_radius(r: f64, alignment: f64) -> f64 {
    let t = (alignment / 30f64.to_radians().cos()).clamp(0.,1.);
    let weight = t*t*(3.-2.*t);
    let extension = if r <= 8. {0.}
        else if r <= 128. {(r-8.)*128./120.}
        else {128.*((2048.-r)/1920.).clamp(0.,1.)};
    r + extension*weight
}
fn detail_ring(up: V, r: f64, count: usize, heading: Option<V>) -> Vec<[f64; 6]> {
    let Some(heading) = heading else {return ring(up,r,count)};
    let (east,north) = basis(up);
    (0..count).map(|j| {
        let (sa,ca)=(j as f64/count as f64*std::f64::consts::TAU).sin_cos();
        let direction: V=std::array::from_fn(|k| east[k]*ca+north[k]*sa);
        let theta=cone_radius(r,dot(direction,heading))/terrain::RADIUS;
        let (st,ct)=theta.sin_cos();
        let n: V=std::array::from_fn(|k|up[k]*ct+direction[k]*st);
        let p=n.map(|x|x*(terrain::RADIUS+terrain::height(n)));
        [p[0],p[1],p[2],1.,1.,1.]
    }).collect()
}
fn ring(up: V, r: f64, count: usize) -> Vec<[f64; 6]> {
    let (east, north) = basis(up);
    let theta = r / terrain::RADIUS;
    let (st, ct) = theta.sin_cos();
    (0..count)
        .map(|j| {
            let (sa, ca) = (j as f64 / count as f64 * std::f64::consts::TAU).sin_cos();
            let n: V = std::array::from_fn(|k| up[k] * ct + (east[k] * ca + north[k] * sa) * st);
            let h = terrain::RADIUS + terrain::height(n);
            let p = n.map(|v| v * h);
            [p[0], p[1], p[2], 1., 1., 1.]
        })
        .collect()
}
fn connect(rings: &[Vec<[f64; 6]>], center: bool) -> Vec<[[f64; 6]; 3]> {
    let mut triangles = Vec::new();
    for (i, pair) in rings.windows(2).enumerate() {
        let a = &pair[0];
        let b = &pair[1];
        let ratio = b.len() / a.len();
        for j in 0..a.len() {
            let next = (j + 1) % a.len();
            let k = j * ratio;
            // Fan works for every integral refinement ratio, including the
            // first wide ring seen when flying far above the terrain.
            for s in 0..ratio {
                triangles.push([a[j], b[(k + s) % b.len()], b[(k + s + 1) % b.len()]]);
            }
            if !(center && i == 0) {
                triangles.push([a[j], b[(k + ratio) % b.len()], a[next]]);
            }
        }
    }
    triangles
}
fn extent(camera: V) -> f64 {
    let radius = terrain::RADIUS;
    let distance = dot(camera, camera).sqrt();
    ((radius / distance.max(radius + 0.1)).acos() * 1.6 + 0.001).clamp(0.002, 1.45) * radius
}
fn build_distant(surface: &Value, camera: V, up: V, lod: i32, heading: Option<V>) -> Result<Distant, String> {
    let field = Field::from_surface(surface)?;
    let mut rings = vec![detail_ring(up, 128., 512, heading)];
    let horizon = extent(camera);
    let mut r = 132.;
    while r < horizon && rings.len() < 144 {
        rings.push(detail_ring(up, r, 512, heading));
        r = if r < 384. {
            r + 4.
        } else if r < 8192. {
            r + (r * 0.16).min(160.)
        } else {
            r * 1.2
        };
    }
    rings.push(detail_ring(up, horizon, 512, heading));
    let boundary = rings[0].clone();
    let triangles = terrain::material_domain::clip(connect(&rings, false), &field.domains);
    let mut geometry = finish(surface, camera, triangles, None, true);
    geometry.anchor = up.map(f64::to_bits);
    geometry.lod = lod;
    Ok(Distant { geometry:Arc::new(geometry), boundary })
}
fn build(
    surface: &Value,
    camera: V,
    altitude: f64,
    distant: Option<&Distant>,
    heading: Option<V>,
) -> Result<Geometry, String> {
    let field = Field::from_surface(surface)?;
    if terrain::underground::deep_view(camera) {
        if let Some(cave)=&field.caves {
            let mut triangles=terrain::material_domain::clip(cave.mesh(camera,&[])?,&field.domains);
            triangles.extend(terrain::material_domain::shell(surface["edits"].as_array().map(Vec::as_slice).unwrap_or(&[]),field.caves.as_ref()));
            return Ok(finish(surface,camera,triangles,None,true));
        }
    }
    let up = terrain::mesh_anchor(camera);
    let extent = if distant.is_some() {
        64.
    } else {
        extent(camera)
    };
    // These rings are also sampled by the player support query. They are a
    // coarse mesh of the same material volume, not a second textured ground.
    let mut radii = vec![0.];
    let mut r = if altitude < 100. {
        0.5
    } else {
        (altitude * 0.02).max(1.)
    };
    while r < extent && radii.len() < 144 {
        radii.push(r);
        r = if r < 8. && altitude < 100. {
            r + 0.5
        } else if r < 96. && altitude < 100. {
            r + 2.
        } else if r < 128. {
            r * 1.20
        } else if r < 8192. {
            r + (r * 0.16).min(160.)
        } else {
            r * 1.20
        };
    }
    radii.push(extent);
    // Keep the exact 64-sector, half-metre support patch near the feet.
    // Wider rings gain angular samples instead of kilometre-wide triangles.
    let segments = |r: f64| {
        if r <= 16. {
            64
        } else if r <= 32. {
            256
        } else {
            512
        }
    };
    let mut rings: Vec<Vec<_>> = radii.iter().map(|r| detail_ring(up, *r, segments(*r), heading)).collect();
    if let Some(distant) = distant {
        // Match both boundary polygons exactly. Intermediate radial samples
        // avoid one enormous, untexturable strip between independently anchored meshes.
        let inner = rings.last().unwrap().clone();
        let outer = &distant.boundary;
        for step in 1..32 {
            let t = step as f64 / 32.;
            rings.push(inner.iter().zip(outer).map(|(a,b)| {
                let up = unit(std::array::from_fn(|k| a[k]*(1.-t)+b[k]*t));
                let p = up.map(|x| x*(terrain::RADIUS+terrain::height(up)));
                [p[0],p[1],p[2],1.,1.,1.]
            }).collect());
        }
        rings.push(outer.clone());
    }
    let mut triangles = connect(&rings, true);
    if let Some(cave) = &field.caves {
        if cave.affected(camera, 32.) {
            triangles = cave.replace_patch(triangles, camera, &[])?;
        }
    }
    triangles = terrain::material_domain::clip(triangles, &field.domains);
    triangles.extend(terrain::material_domain::shell(
        surface["edits"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        field.caves.as_ref(),
    ));
    Ok(finish(
        surface,
        camera,
        triangles,
        (altitude < 24.).then_some((&field, up)), altitude < 24.,
    ))
}
// Reuse the resting surface and grass. Only batches touching a removed cell
// need polygon clipping, and the newly exposed walls use the same lighting.
fn cut_geometry(base: &Geometry, cuts: &[Value], rest: &Value) -> Result<Geometry,String> {
    let domains = terrain::material_domain::from_edits(cuts);
    let mut g = base.clone();
    g.vertices.clear();
    g.normals.clear();
    g.rocks.clear();
    g.blocks.clear();
    for block in &base.blocks {
        let center = std::array::from_fn(|k| base.origin[k] + block.center[k]);
        let near: Vec<_> = domains.iter().filter(|d| d.distance(center) <= block.radius).cloned().collect();
        let start = g.vertices.len();
        if near.is_empty() {
            g.vertices.extend_from_slice(&base.vertices[block.start..block.end]);
            g.normals.extend_from_slice(&base.normals[block.start..block.end]);
            g.rocks.extend_from_slice(&base.rocks[block.start..block.end]);
        } else {
            for i in (block.start..block.end).step_by(3) {
                let tri = std::array::from_fn(|j| {
                    let v = base.vertices[i+j];
                    [base.origin[0]+v[0] as f64,base.origin[1]+v[1] as f64,base.origin[2]+v[2] as f64,
                        v[3] as f64,v[4] as f64,v[5] as f64]
                });
                for t in terrain::material_domain::clip(vec![tri], &near) {
                    for v in t {
                        g.vertices.push([(v[0]-base.origin[0]) as f32,(v[1]-base.origin[1]) as f32,
                            (v[2]-base.origin[2]) as f32,v[3] as f32,v[4] as f32,v[5] as f32]);
                        g.normals.push(base.normals[i]);
                        g.rocks.push(base.rocks[i]);
                    }
                }
            }
        }
        if g.vertices.len() > start {
            g.blocks.push(Block {start,end:g.vertices.len(),..block.clone()});
        }
    }
    let field = Field::from_surface(rest)?;
    let shell = terrain::material_domain::shell(cuts,field.caves.as_ref());
    let walls = finish(rest,base.origin,shell,None,true);
    let offset = g.vertices.len();
    g.vertices.extend(walls.vertices);
    g.normals.extend(walls.normals);
    g.rocks.extend(walls.rocks);
    g.blocks.extend(walls.blocks.into_iter().map(|b|Block {start:b.start+offset,end:b.end+offset,..b}));
    Ok(g)
}
fn finish(
    surface: &Value,
    camera: V,
    triangles: Vec<[[f64; 6]; 3]>,
    grass: Option<(&Field, V)>,
    texture: bool,
) -> Geometry {
    let mut g = Geometry {
        origin: camera,
        anchor: [0; 3],
        lod: 0,
        edits: surface["edits"].clone(),
        underground: surface["underground"].clone(),
        cave_window: terrain::underground::mesh_window(camera),
        vertices: Vec::new(),
        normals: Vec::new(),
        rocks: Vec::new(),
        blocks: Vec::new(),
    };
    let mut substances = HashMap::new();
    let cover_field = Field::from_surface(surface).ok().and_then(|field|field.caves);
    let cover=cover_field.as_ref().and_then(terrain::surface_cover::Sampler::new);
    for tri in triangles {
        let p = tri.map(|p| [p[0], p[1], p[2]]);
        let center: V = std::array::from_fn(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.);
        let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        let samples = p.map(|p| {
            *substances
                .entry(p.map(f64::to_bits))
                .or_insert_with(|| {
                    let mut sample=material_sample(p);
                    if sample.original_surface {
                        if let Some(color)=cover.as_ref().and_then(|cover|cover.color(p)) {
                            sample.color=color;
                        }
                    }
                    sample
                })
        });
        let material = if samples[0].kind == samples[1].kind || samples[0].kind == samples[2].kind {
            samples[0].kind
        } else if samples[1].kind == samples[2].kind {
            samples[1].kind
        } else {
            world_field::substance(center)
        };
        let steep = dot(n, unit(center)).abs() < 0.65;
        let original_surface = samples.iter().all(|sample| sample.original_surface);
        let start = g.vertices.len();
        push(&mut g, p, [0.; 3], Some(n));
        if material == Substance::Stone && terrain::cave_formations::mineral_color(center).is_none() {
            g.rocks[start..].fill(terrain::rock::kind(center));
        }
        // Base material is a world-space field at every resolution. The rendered
        // triangle's slope and size must not turn a bank from grass into soil.
        // Newly exposed cuts keep the face-based root-mat side appearance.
        for (vertex, sample) in g.vertices[start..].iter_mut().zip(samples) {
            let tint = if !original_surface && sample.kind == Substance::Turf && steep {
                world_field::ROOT_COLOR
            } else {
                sample.color
            };
            vertex[3..].copy_from_slice(&tint);
        }
        if original_surface {
            // Shared world-space normals keep lighting independent of face size.
            for (normal, sample) in g.normals[start..].iter_mut().zip(samples) {
                *normal = sample.normal;
            }
        }
        // Only unresolved grain detail fades as triangles become larger.
        let edge_squared = (0..3).map(|i| {
            let edge = sub(p[i], p[(i + 1) % 3]);
            dot(edge, edge)
        }).fold(0.0_f64, f64::max);
        let coarse = ((edge_squared.sqrt() - 1.) / 7.).clamp(0., 1.) as f32;
        // Bake the diffuse pattern in world space, so adjoining faces meet
        // cleanly and the grain stays put when the camera moves.
        if texture && coarse < 1. && material != Substance::Stone {
            for (v, point) in g.vertices[start..].iter_mut().zip(p) {
                let [x,y,z] = point;
                let mottling = (x*1.7 + y*0.9).sin() * (z*2.3-y*0.6).cos();
                let grain = match material {
                    Substance::Stone | Substance::Bedrock => (y*15.0 + z*0.8 + mottling).sin(),
                    _ => (x*8.0-z*5.0+y*3.0).sin(),
                };
                let shade = 1.0 + (1.0 - coarse) * (0.12*mottling + 0.055*grain) as f32;
                for channel in &mut v[3..] { *channel *= shade; }
            }
        }
    }
    if let Some((field, up)) = grass {
        turf(&mut g, &field, up);
    }
    // Spatially coherent batches are rejected before per-vertex lighting and
    // projection. Rebuilt only with geometry, not for every camera rotation.
    for start in (0..g.vertices.len()).step_by(96) {
        let end = (start + 96).min(g.vertices.len());
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for v in &g.vertices[start..end] {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k] as f64);
                hi[k] = hi[k].max(v[k] as f64);
            }
        }
        let center = std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5);
        let radius = dot(sub(hi, center), sub(hi, center)).sqrt();
        g.blocks.push(Block {
            start,
            end,
            center,
            radius,
        });
    }
    g
}

/// Resting crowns are world anchored and generated by the exact crown builder
/// used by the deforming turf. Geometry LOD replaces microscopic leaf simulation
/// at distance; it never replaces the physical root-mat material with dirt.
fn turf(g: &mut Geometry, field: &Field, up: V) {
    // Thousands of crowns rest on the same support-ring vertices. Reuse their
    // height samples instead of evaluating those identical landforms per blade.
    let mut support_heights = HashMap::new();
    let axis = (0..3)
        .max_by(|a, b| up[*a].abs().total_cmp(&up[*b].abs()))
        .unwrap();
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let center = [
        up[u] / up[axis].abs() * terrain::RADIUS,
        up[v] / up[axis].abs() * terrain::RADIUS,
    ];
    for (spacing, inner, reach, leaves) in
        [(0.22, 0., 2.4, 2), (0.42, 2.4, 6., 2), (0.85, 6., 12., 1)]
    {
        let start = [
            ((center[0] - reach) / spacing).floor() as i64,
            ((center[1] - reach) / spacing).floor() as i64,
        ];
        let end = [
            ((center[0] + reach) / spacing).ceil() as i64,
            ((center[1] + reach) / spacing).ceil() as i64,
        ];
        for x in start[0]..=end[0] {
            for z in start[1]..=end[1] {
                let seed = (x as i32).wrapping_mul(73856093) ^ (z as i32).wrapping_mul(19349663);
                let jitter = |salt: i32| {
                    let h = (seed ^ salt) as u32;
                    let h = h.wrapping_mul(0x45d9f3b);
                    (h ^ (h >> 16)) as f64 / u32::MAX as f64
                };
                let q = [
                    (x as f64 + 0.2 + 0.6 * jitter(13)) * spacing,
                    (z as f64 + 0.2 + 0.6 * jitter(79)) * spacing,
                ];
                let d = (q[0] - center[0]).hypot(q[1] - center[1]);
                if d < inner || d > reach {
                    continue;
                }
                let mut n = [0.; 3];
                n[axis] = up[axis].signum();
                n[u] = q[0] / terrain::RADIUS;
                n[v] = q[1] / terrain::RADIUS;
                n = unit(n);
                let h = terrain::height(n);
                let p = n.map(|v| v * (terrain::RADIUS + h));
                if world_field::substance(p) != Substance::Turf
                    || field.refined(p)
                    || field.rest_distance(n.map(|v| v * (terrain::RADIUS + h - 0.03))) > 0.
                {
                    continue;
                }
                let h = terrain::rendered_height(g.origin, n, |p| {
                    *support_heights
                        .entry(p.map(f64::to_bits))
                        .or_insert_with(|| terrain::height(p))
                })
                .unwrap_or(h);
                let p = n.map(|v| v * (terrain::RADIUS + h));
                let normal = terrain::surface_normal(n);
                let (e, _) = basis(normal);
                let base = sub(p, g.origin).map(|v| v as f32);
                let origin = g.origin;
                world_field::grass_crown(
                    base,
                    normal.map(|v| v as f32),
                    e.map(|v| v as f32),
                    seed,
                    leaves,
                    |tri, color| {
                        let points = tri.map(|p| std::array::from_fn(|k| p[k] as f64 + origin[k]));
                        push(g, points, color, Some(normal));
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod scene_cpu_terrain_tests {
    use super::*;
    #[test]
    fn scene_cpu_fused_terrain_matches_original_with_motion_light_edits_and_caves() {
        let caves:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
        for (camera,yaw,pitch,underground,edits) in [
            ([0.,terrain::RADIUS+1.8,0.],0.,0.,Value::Null,json!([])),
            ([2.25,terrain::RADIUS+1.8,-1.5],1.1,0.15,Value::Null,json!([])),
            ([30.,terrain::RADIUS+85.,-50.],3.14,-0.25,Value::Null,json!([])),
            ([0.,terrain::RADIUS+1.8,0.],0.,0.,caves.clone(),json!([])),
            ([0.,terrain::RADIUS-40.,0.],0.2,0.1,caves.clone(),json!([])),
            ([0.,terrain::RADIUS+1.8,0.],0.,0.,Value::Null,json!([{"normal":[0.,1.,0.],"radius":2.,"depth":0.8,"mined":true}])),
        ] {
            let skin=[[camera[0]-0.2,camera[1]-0.2,camera[2]-1.,0.4,0.3,0.2],
                [camera[0]+0.2,camera[1]-0.2,camera[2]-1.,0.4,0.3,0.2],
                [camera[0],camera[1]+0.2,camera[2]-1.,0.4,0.3,0.2]];
            let surface=json!({"edits":edits,"debris":[],"underground":underground});
            for fraction in [0.,0.5] {
                let view=json!({"position":camera,"yaw":yaw,"pitch":pitch,"aspect":1.6,"day_night":{"fraction":fraction}});
                let old=reference_planet(&view,&surface,Some(&skin)).unwrap();
                let new=planet(&view,&surface,Some(&skin)).unwrap();
                assert_eq!(old.len(),new.len(),"{camera:?}");
                for (i,(a,b)) in old.iter().zip(&new).enumerate() {assert_eq!(a.map(f32::to_bits),b.map(f32::to_bits),"face {i} camera {camera:?}");}
            }
        }
    }
}

// Share immutable accepted geometry; only per-camera output belongs to a job.
// Contiguous block spans preserve the exact original face and draw order.
struct ProjectionInput {geometries:Vec<Arc<Geometry>>,view:Value,camera:V,deep:bool}
fn project_span(input:&ProjectionInput,range:std::ops::Range<usize>)->Result<Vec<[f32;24]>,String>{
    let view=&input.view;let camera=input.camera;let deep=input.deep;
    let distance=dot(camera,camera).sqrt();
    let lighting=crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
    let horizon=crate::base_system::layer_four::atmosphere::palette(lighting.fraction,1.);
        let fullbright=view["diagnostic_cave_fullbright"]==true;
        let lit = |mut v: Vertex, n: V| {
            // Review-only unlit albedo: preserve geometry, textures and culling.
            if fullbright {return v;}
            let light = 0.30 + 0.70 * dot(n, lighting.primary).abs();
            let distance = v[..3]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            // The finite underground mesh ends in darkness, not an outdoor sky.
            let fog = if deep {let t=((distance-24.)/10.).clamp(0.,1.);t*t*(3.-2.*t)}
                else {(1. - (-distance / 15_000.).exp()).min(0.82)};
            let exposure=if deep {0.}else{terrain::underground::sky_exposure(std::array::from_fn(|i|camera[i]+v[i] as f64))};
            for k in 0..3 {
                v[3 + k] = (v[3 + k] as f64
                    * light
                    * lighting.tint[k]
                    * lighting.brightness()
                    * exposure
                    * (1. - fog)
                    + if deep {[0.004,0.008,0.016][k]*fog} else {horizon[k]*fog*exposure}) as f32;
            }
            v
        };

    let mut local=view.clone();local["position"]=json!([0,0,0]);local["far"]=json!(distance+terrain::RADIUS*2.);
    local["near"]=json!(0.05);local["two_sided"]=json!(true);
    let projector=super::projection::Projector::new(&local);
    let culler=crate::vegetation::forest::View::from_input(view)?;
    let mut out=PROJECT_OUTPUT.with(|slot|std::mem::take(&mut *slot.borrow_mut()));
    out.clear();out.reserve(range.len()*32);
    if range.start==0 && view["scene_sky_first"]==true {out.extend(lighting.sky(view));}
    let mut block_index=0;
        for g in &input.geometries {
            let shift=sub(g.origin,camera);
            for block in &g.blocks {
                let index=block_index;block_index+=1;
                if !range.contains(&index){continue;}
                let center=std::array::from_fn(|k|g.origin[k]+block.center[k]);
                if !culler.visible(center,block.radius) {continue;}
                for start in (block.start..block.end).step_by(3) {
                    let tri:[Vertex;3]=std::array::from_fn(|i| {
                        let mut v=g.vertices[start+i];
                        for k in 0..3 {v[k]=(v[k] as f64+shift[k]) as f32;}
                        v
                    });
                    let mut face=projector.triangle(&tri).unwrap();
                    // Same final homogeneous clipping predicate as before.
                    // Reject hidden faces before expensive fog/color work.
                    if !in_frustum(&face) {continue;}
                    for i in 0..3 {
                        let v=lit(tri[i],g.normals[start+i]);
                        face[12+i*4..15+i*4].copy_from_slice(&v[3..]);
                    }
                    pack_terrain_material(&mut face,&tri,g.rocks[start],camera);
                    out.push(face);
                }
            }
        }

    Ok(out)
}
struct ProjectionWorker{
 input:Option<mpsc::SyncSender<(Arc<ProjectionInput>,std::ops::Range<usize>)>>,
 output:mpsc::Receiver<Result<Vec<[f32;24]>,String>>,
 thread:Option<std::thread::JoinHandle<()>>,
}
impl ProjectionWorker{
 fn new()->Result<Self,String>{
  let(tx,rx)=mpsc::sync_channel::<(Arc<ProjectionInput>,std::ops::Range<usize>)>(1);
  let(done_tx,done_rx)=mpsc::sync_channel(1);
  let thread=std::thread::Builder::new().name("mirror-project-terrain".into()).spawn(move||{
   while let Ok((input,range))=rx.recv(){
    let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::GameGraphics);
    if done_tx.send(project_span(&input,range)).is_err(){break;}
   }
  }).map_err(|e|format!("terrain projection pool: {e}"))?;
  Ok(Self{input:Some(tx),output:done_rx,thread:Some(thread)})
 }
}
impl Drop for ProjectionWorker{fn drop(&mut self){self.input.take();if let Some(thread)=self.thread.take(){let _=thread.join();}}}
thread_local!{static PROJECT_OUTPUT:RefCell<Vec<[f32;24]>>=const{RefCell::new(Vec::new())};}
pub(super) fn recycle_projection_output(buffer:Vec<[f32;24]>){PROJECT_OUTPUT.with(|slot|*slot.borrow_mut()=buffer);}
thread_local!{static PROJECTION_POOL:RefCell<Vec<ProjectionWorker>>=const{RefCell::new(Vec::new())};}
fn project_parallel(geometries:Vec<Arc<Geometry>>,view:&Value,camera:V,deep:bool)->Result<Vec<[f32;24]>,String>{
 let blocks=geometries.iter().map(|g|g.blocks.len()).sum::<usize>();
 let input=Arc::new(ProjectionInput{geometries,view:view.clone(),camera,deep});
 if cfg!(test)||blocks<96||std::env::var("WORMWOOD_GAME_PROJECTION_PARALLEL").as_deref()!=Ok("1"){
  return project_span(&input,0..blocks);
 }
 PROJECTION_POOL.with(|pool|{
  let mut pool=pool.borrow_mut();while pool.len()<3{pool.push(ProjectionWorker::new()?);}
  for (i,worker) in pool.iter().enumerate(){
   worker.input.as_ref().unwrap().send((input.clone(),blocks*i/3..blocks*(i+1)/3)).map_err(|_|"terrain projector terminated".to_owned())?;
  }
  let mut output=Vec::new();let mut failure=None;
  for worker in pool.iter(){
   match worker.output.recv().map_err(|_|"terrain projector terminated".to_owned()).and_then(|r|r){
    Ok(mut faces)=>output.append(&mut faces),Err(error)=>{failure=Some(error);},
   }
  }
  if let Some(error)=failure{Err(error)}else{Ok(output)}
 })
}

#[cfg(test)]mod parallel_projection_tests {
 use super::*;
 #[test]fn exterior_accepted_mesh_stays_bounded_during_fast_turn_back(){
  let camera=[1.9316450851616325,6371002.039256963,-97.20798567522479];
  let surface=json!({"edits":[],"debris":[]});
  let distance=dot(camera,camera).sqrt();let altitude=(distance-terrain::RADIUS-terrain::height(unit(camera))).max(0.);
  let mut highest=terrain::height([0.,1.,0.]);
  for x in -8..=8 {for z in -8..=8 {if x*x+z*z<=64 {highest=highest.max(terrain::height(unit([x as f64*10.,terrain::RADIUS,z as f64*10.])));}}}
  let origin=[0.,terrain::RADIUS+highest+0.5,0.];
  let venue=crate::theatre::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  for heading in [0.,1.,2.,3.,4.,5.] {
   let mut cache=Cache::default();
   let warm=json!({"position":camera,"yaw":heading,"pitch":0.,"aspect":1.6});
   prepare_cache(&mut cache,&warm,&surface,camera,altitude,(altitude/8.)as i32,terrain::mesh_anchor(camera).map(f64::to_bits),false).unwrap();
   let view=json!({"position":camera,"yaw":0.,"pitch":0.,"aspect":1.6,
    "right":[-0.9896787608671664,0.000002486574636477075,0.14330369947160168],
    "up":[-0.011330833300355305,0.9968678134895165,-0.07826988338658865],
    "forward":[0.14285504018112113,0.07908579153329359,0.9865788742277035]});
   let geometries:Vec<_>=std::iter::once(cache.near.unwrap()).chain(cache.distant.map(|d|d.geometry)).collect();
   let blocks=geometries.iter().map(|g|g.blocks.len()).sum();
   let projected=project_span(&ProjectionInput{geometries,view:view.clone(),camera,deep:false},0..blocks).unwrap();
   let venue=super::super::theatre_mesh(origin,&venue.vertices,&view).unwrap();
   let count=projected.len()+venue.len()+2;
   eprintln!("accepted heading {heading}: terrain {}, venue {}, total {count}",projected.len(),venue.len());
   assert!(projected.iter().flatten().all(|v|v.is_finite()));
   assert!(count<=crate::scene_limits::MAX_SCENE_TRIANGLES,"accepted mesh exceeded scene storage on fast turn: {count}");
  }
 }
 #[test]fn parallel_spans_preserve_exact_serial_vertices_and_face_order(){
  let geometry=Geometry{origin:[0.,terrain::RADIUS,0.],anchor:[0;3],lod:0,edits:json!([]),underground:Value::Null,cave_window:[0;3],
   vertices:vec![[-1.,0.,-4.,0.3,0.4,0.2],[1.,0.,-4.,0.3,0.4,0.2],[0.,1.,-4.,0.3,0.4,0.2],[-1.,0.,-8.,0.6,0.2,0.1],[1.,0.,-8.,0.6,0.2,0.1],[0.,1.,-8.,0.6,0.2,0.1]],
   normals:vec![[0.,1.,0.];6],rocks:vec![-1;6],blocks:vec![Block{start:0,end:3,center:[0.,0.,-4.],radius:2.},Block{start:3,end:6,center:[0.,0.,-8.],radius:2.}]};
  let input=Arc::new(ProjectionInput{geometries:vec![Arc::new(geometry)],view:json!({"position":[0.,terrain::RADIUS,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}}),camera:[0.,terrain::RADIUS,0.],deep:false});
  let expected=project_span(&input,0..2).unwrap();assert!(!expected.is_empty());
  let workers:Vec<_>=(0..3).map(|_|ProjectionWorker::new().unwrap()).collect();
  for (i,w) in workers.iter().enumerate(){w.input.as_ref().unwrap().send((input.clone(),2*i/3..2*(i+1)/3)).unwrap();}
  let mut got=Vec::new();for w in workers{got.extend(w.output.recv_timeout(std::time::Duration::from_secs(5)).unwrap().unwrap());}
  assert_eq!(got,expected);
 }
}

fn prepare_cache(cache:&mut Cache,view:&Value,surface:&Value,camera:V,altitude:f64,lod:i32,anchor:[u64;3],deep:bool)->Result<(),String>{
        // Retain the cone through small turns; rebuild after about 11 degrees.
        // Looking vertically retains the last useful tangent heading.
        let requested = if !deep && altitude < 100. { terrain_heading(view, camera) } else { None };
        let heading = match (cache.heading, requested) {
            (Some(old), Some(new)) if dot(old, new) > 0.98 => Some(old),
            (_, Some(new)) => Some(new),
            (old, None) if !deep && altitude < 100. => old,
            _ => None,
        };
        let heading_changed = heading != cache.heading;
        cache.heading = heading;
        // Hills remain fixed while the precise two-metre support patch follows
        // the player. Only a short connecting annulus changes between them.
        let mut distant_changed = false;
        if !deep && altitude < 100. {
            let mut up = distant_anchor(camera);
            let near_up = terrain::mesh_anchor(camera);
            // The legacy local tangent basis changes reference axis near the
            // poles. Keep both seam rings on the same side of that boundary.
            if dot(basis(up).0, basis(near_up).0) < 0.99 {
                up = near_up;
            }
            let moved = heading_changed || cache.distant.as_ref().is_none_or(|d|
                d.geometry.anchor != up.map(f64::to_bits) || d.geometry.lod != lod);
            if moved || cache.near.as_ref().is_none_or(|g| g.edits != surface["edits"]) {
                // Cuts entirely inside the inner ring cannot alter distant triangles.
                let mut distant_surface = surface.clone();
                let center = up.map(|x| x * (terrain::RADIUS + terrain::height(up)));
                if let Some(edits) = distant_surface["edits"].as_array_mut() {
                    edits.retain(|e| terrain::material_domain::Domain::parse(e)
                        .is_none_or(|d| !d.mined || d.distance(center) > 68.));
                }
                if moved || cache.distant.as_ref().is_none_or(|d|d.geometry.edits != distant_surface["edits"]) {
                    cache.distant = Some(build_distant(&distant_surface, camera, up, lod, heading)?);
                    distant_changed = true;
                }
            }
        } else if cache.distant.take().is_some() {
            distant_changed = true;
        }
        if distant_changed
            || cache.near.as_ref().is_none_or(|g| {
                g.anchor != anchor
                    || g.lod != lod
                    || g.edits != surface["edits"]
                    || g.underground != surface["underground"]
                    || g.cave_window != terrain::underground::mesh_window(camera)
            })
        {
            let mut rest = surface.clone();
            let cuts: Vec<_> = surface["edits"].as_array().into_iter().flatten()
                .filter(|e| e["mined"] == true).cloned().collect();
            if let Some(edits) = rest["edits"].as_array_mut() { edits.retain(|e| e["mined"] != true); }
            if rest["underground"].is_object() {
                rest["underground"]["material_domains"] = rest["edits"].clone();
            }
            if distant_changed || cache.unmined.as_ref().is_none_or(|g| g.anchor != anchor ||
                g.lod != lod || g.edits != rest["edits"] || g.underground != rest["underground"] ||
                g.cave_window != terrain::underground::mesh_window(camera)) {
                let mut base = build(&rest, camera, altitude, cache.distant.as_ref(), heading)?;
                base.anchor = anchor;
                base.lod = lod;
                cache.unmined = Some(Arc::new(base));
            }
            let base = cache.unmined.as_ref().unwrap();
            let mut g = if cuts.is_empty() {base.as_ref().clone()} else { cut_geometry(base, &cuts, &rest)? };
            g.edits = surface["edits"].clone();
            g.underground = surface["underground"].clone();
            cache.near = Some(Arc::new(g));
        }

    Ok(())
}
struct GeometryBuild {cache:Cache,view:Value,surface:Value,camera:V,altitude:f64,lod:i32,anchor:[u64;3],deep:bool}
struct GeometryWorker{
 input:Option<mpsc::SyncSender<GeometryBuild>>,output:mpsc::Receiver<Result<Cache,String>>,
 thread:Option<std::thread::JoinHandle<()>>,busy:bool,
}
static STREAM_REQUESTS:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static STREAM_APPLIED:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static STREAM_REJECTED:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static STREAM_MAX_US:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
impl GeometryWorker{
 fn new()->Result<Self,String>{
  let(tx,rx)=mpsc::sync_channel::<GeometryBuild>(1);let(done_tx,done_rx)=mpsc::sync_channel(1);
  let thread=std::thread::Builder::new().name("mirror-build-terrain".into()).stack_size(16*1024*1024).spawn(move||{
   while let Ok(mut job)=rx.recv(){
    let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::GameGraphics);
    let start=std::time::Instant::now();
    let result=prepare_cache(&mut job.cache,&job.view,&job.surface,job.camera,job.altitude,job.lod,job.anchor,job.deep).map(|_|job.cache);
    STREAM_MAX_US.fetch_max(start.elapsed().as_micros().min(u64::MAX as u128)as u64,std::sync::atomic::Ordering::Relaxed);
    if done_tx.send(result).is_err(){break;}
   }
  }).map_err(|e|format!("terrain geometry worker: {e}"))?;
  Ok(Self{input:Some(tx),output:done_rx,thread:Some(thread),busy:false})
 }
}
impl Drop for GeometryWorker{fn drop(&mut self){self.input.take();if let Some(thread)=self.thread.take(){let _=thread.join();}}}
thread_local!{static GEOMETRY_WORKER:RefCell<Option<GeometryWorker>>=const{RefCell::new(None)};}
fn stream_geometry(cache:&mut Cache,view:&Value,surface:&Value,camera:V,altitude:f64,lod:i32,anchor:[u64;3],deep:bool)->Result<(),String>{
 GEOMETRY_WORKER.with(|slot|{
  let mut slot=slot.borrow_mut();if slot.is_none(){*slot=Some(GeometryWorker::new()?);}
  let worker=slot.as_mut().unwrap();
  if worker.busy{
   match worker.output.try_recv(){
    Ok(result)=>{worker.busy=false;let ready=result?;
     if ready.near.as_ref().is_some_and(|g|accepted_geometry(g,surface,camera,deep)){
      *cache=ready;STREAM_APPLIED.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
     }else{STREAM_REJECTED.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
    },
    Err(mpsc::TryRecvError::Empty)=>{},
    Err(mpsc::TryRecvError::Disconnected)=>return Err("terrain geometry worker terminated".into()),
   }
  }
  let requested=if !deep&&altitude<100.{terrain_heading(view,camera)}else{None};
  let turned=match (cache.heading,requested){(Some(a),Some(b))=>dot(a,b)<=0.98,(None,Some(_))=>true,_=>false};
  let near_changed=cache.near.as_ref().is_none_or(|g|g.anchor!=anchor||g.lod!=lod||g.cave_window!=terrain::underground::mesh_window(camera));
  let far_changed=if !deep&&altitude<100.{
   let mut up=distant_anchor(camera);let near=terrain::mesh_anchor(camera);if dot(basis(up).0,basis(near).0)<0.99{up=near;}
   cache.distant.as_ref().is_none_or(|d|d.geometry.anchor!=up.map(f64::to_bits)||d.geometry.lod!=lod)
  }else{cache.distant.is_some()};
  if !worker.busy&&(near_changed||far_changed||turned){
   worker.input.as_ref().unwrap().try_send(GeometryBuild{cache:cache.clone(),view:view.clone(),surface:surface.clone(),camera,altitude,lod,anchor,deep})
    .map_err(|e|format!("terrain geometry queue: {e}"))?;
   worker.busy=true;STREAM_REQUESTS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
  }
  Ok(())
 })
}
pub(super) fn streaming_report()->Value{
 use std::sync::atomic::Ordering::Relaxed;
 json!({"requested":STREAM_REQUESTS.load(Relaxed),"applied":STREAM_APPLIED.load(Relaxed),"stale_world_results_discarded":STREAM_REJECTED.load(Relaxed),
  "max_build_ms":STREAM_MAX_US.load(Relaxed)as f64/1000.,"scope":"One background near/distant build. Completed meshes publish together; the current camera and lighting project the last accepted geometry meanwhile. World edits invalidate immediately; stale world results never overwrite them. No mesh simplification or ray-budget reduction."})
}

#[cfg(test)] mod nightworld_cache_tests {
 use super::*;
 #[test] fn nightworld_cache_rejects_foreign_chart_and_surface_mode(){
  let surface=json!({"edits":[],"underground":null});let camera=[0.,-terrain::RADIUS+90.,0.];
  let mut g=finish(&surface,camera,Vec::new(),None,false);g.lod= -1;
  assert!(accepted_geometry(&g,&surface,camera,true));
  assert!(!accepted_geometry(&g,&surface,[0.,terrain::RADIUS-90.,0.],true));
  assert!(!accepted_geometry(&g,&surface,[0.,-terrain::RADIUS-60.,0.],false));
 }
}

#[cfg(test)] mod cave_lighting_tests {
 use super::*;
 #[test] fn roofed_cave_is_independent_of_daylight_and_night_vision_reveals_geometry() {
  let eye=terrain::underground::world([-51.,-44.,16.]);
  let mut view=json!({"position":eye,"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}});
  let surface=json!({"edits":[],"debris":[],"underground":serde_json::from_str::<Value>(include_str!("../../Procedures/caves.json")).unwrap()});
  let day=planet(&view,&surface,None).unwrap();
  view["day_night"]["fraction"]=json!(0.);
  let night=planet(&view,&surface,None).unwrap();
  let rock_colors=|mesh: &Vec<[f32;24]>|mesh.iter().filter(|t|t[15]<=-4096.).map(|t|[t[12],t[13],t[14],t[16],t[17],t[18],t[20],t[21],t[22]]).collect::<Vec<_>>();
  assert!(!rock_colors(&day).is_empty());assert_eq!(rock_colors(&day),rock_colors(&night));
  view["diagnostic_cave_fullbright"]=json!(true);
  let bright=planet(&view,&surface,None).unwrap();
  assert_eq!(bright.len(),day.len());
  assert!(rock_colors(&bright).iter().flatten().sum::<f32>()>rock_colors(&day).iter().flatten().sum::<f32>()+1.);
 }
}
