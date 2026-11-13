//! Registered leaf implementation. Every cross-System operation uses Host.invoke.
use super::{config, math::*, slides, soft_body, surface_mesh, types::*};
use crate::host::Host;
use serde_json::{Value, json};
fn settings(input: &Value) -> Result<config::Settings, String> {
    config::Settings::parse(input)
}
fn dt(input: &Value) -> Result<f64, String> {
    number(input, "dt", 0., 0., 0.1)
}
pub(crate) fn tide(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let s = &mut host.water.tide;
    match hook {
        "Configure" => {
            s.settings = settings(input)?.tide;
            Ok(Value::Null)
        }
        "Step" => {
            let sample = s.advance(dt(input)?, input["world_age"].as_f64())?;
            Ok(
                json!({"height":sample.height,"phase":sample.phase,"normalized":sample.normalized,"rising":sample.rising,"continuous_animation":s.settings.enabled&&s.settings.high!=s.settings.low}),
            )
        }
        "Sample" => {
            let p = s.sample();
            Ok(
                json!({"height":p.height,"phase":p.phase,"normalized":p.normalized,"rising":p.rising}),
            )
        }
        "Close" => {
            *s = Default::default();
            s.settings.enabled = false;
            Ok(Value::Null)
        }
        _ => Err("unknown tide hook".into()),
    }
}
fn ocean_settings(s: &super::ocean::Settings) -> Value {
    json!({"amplitude":s.amplitude,"period":s.period,"wavelength":s.wavelength,"direction":s.direction,"shore_depth":s.shore_depth,"shore_gain":s.shore_gain})
}
fn ocean_snapshot(v: &Value) -> Result<super::ocean::State, String> {
    let mut s = super::ocean::State::default();
    s.settings = settings(&json!({"ocean":v["settings"]}))?.ocean;
    s.time = number(v, "time", 0., 0., 1e12)?;
    Ok(s)
}
pub(crate) fn ocean(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let s = &mut host.water.ocean;
    match hook {
        "Configure" => {
            s.settings = settings(input)?.ocean;
            Ok(Value::Null)
        }
        "Step" => {
            s.time += dt(input)?.min(0.1);
            Ok(json!({"time":s.time,"continuous_animation":s.settings.amplitude>0.}))
        }
        "Snapshot" => Ok(json!({"time":s.time,"settings":ocean_settings(&s.settings)})),
        "Sample" => {
            let p: [f64; 2] =
                serde_json::from_value(input["uv"].clone()).map_err(|_| "ocean uv required")?;
            if p.iter().any(|n| !n.is_finite()) {
                return Err("invalid ocean uv".into());
            }
            let mean = number(input, "mean", 0., -10000., 10000.)?;
            let bed = number(input, "bed", 0., -10000., 10000.)?;
            let shore = number(input, "shore_distance", 0., -1e7, 1e7)?;
            Ok(
                json!({"height":s.surface(p,mean,bed,shore),"offset":s.offset(p,(mean-bed).max(0.),shore)}),
            )
        }
        "Close" => {
            *s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown ocean hook".into()),
    }
}
pub(crate) fn flow(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    if hook == "Sample" {
        let wet = host.invoke("TerrainSurfaceHydrologySystem.Sample", input.clone())?;
        let mut sample = if wet["occupied"] != true {
            super::flow::Sample::default()
        } else {
            let frame = frame(&wet["frame"])?;
            let w = super::flow::WetPoint {
                frame,
                point: vector(&wet["local_point"])?,
                uv: serde_json::from_value(wet["uv"].clone()).map_err(|_| "water uv required")?,
                surface_height: wet["surface_height"]
                    .as_f64()
                    .ok_or("water surface required")?,
                mean_height: wet["mean_height"].as_f64().ok_or("water mean required")?,
                bed_height: wet["bed_height"].as_f64().ok_or("water bed required")?,
                shore_distance: wet["shore_distance"].as_f64().unwrap_or(0.),
                shore_gradient: serde_json::from_value(wet["shore_gradient"].clone())
                    .map_err(|_| "shore gradient required")?,
                wave_amplitude: wet["wave_amplitude"].as_f64().unwrap_or(0.),
                shore_direction: serde_json::from_value(wet["shore_direction"].clone())
                    .map_err(|_| "shore direction required")?,
                downstream_direction: serde_json::from_value(wet["downstream_direction"].clone())
                    .map_err(|_| "river direction required")?,
                kind: if wet["kind"] == "lake" {
                    super::flow::Kind::Lake
                } else if wet["kind"] == "river" {
                    super::flow::Kind::River
                } else {
                    super::flow::Kind::Ocean
                },
            };
            host.water.flow.sample(Some(&w))
        };
        if !input["consumer"].is_null() {
            let id = input["consumer"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("water consumer requires positive identity")?;
            let s = &mut host.water.flow;
            if s.filters.len() >= 128 && !s.filters.contains_key(&id) {
                return Err("water current consumer capacity reached".into());
            }
            let delta = number(input, "dt", s.step_dt, 0., 0.1)?;
            sample = s
                .filters
                .entry(id)
                .or_default()
                .apply(sample, delta, s.settings.response);
        }
        return Ok(
            json!({"occupied":sample.occupied,"velocity":sample.velocity,"strength":sample.strength,"depth":sample.depth,"surface_height":sample.surface_height,"kind":if sample.occupied{wet["kind"].clone()}else{json!("dry")}}),
        );
    }
    let s = &mut host.water.flow;
    match hook {
        "Configure" => {
            s.settings = settings(input)?.flow;
            Ok(Value::Null)
        }
        "Step" => {
            s.advance(dt(input)?);
            Ok(Value::Null)
        }
        "Close" => {
            *s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown water flow hook".into()),
    }
}
fn pose_value(p: slides::Pose) -> Value {
    json!({"position":p.position,"right":p.right,"up":p.up,"forward":p.forward,"radius":p.radius})
}
pub(crate) fn slide(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let s = &mut host.water.slides;
    match hook {
        "Configure" => {
            s.settings = settings(&input["settings"])?.slides;
            s.configure(&input["slides"])?;
            Ok(Value::Null)
        }
        "Edit" => {
            match input["operation"].as_str() {
                Some("place") => {
                    s.place(
                        vector(&input["anchor"])?,
                        vector(&input["normal"])?,
                        vector(&input["forward"])?,
                        input["new_chain"] == true,
                    )?;
                }
                Some("connect") => {
                    s.connect(
                        input["start"].as_u64().ok_or("start required")?,
                        input["end"].as_u64().ok_or("end required")?,
                    )?;
                }
                Some("select") => {
                    let selected = if let Some(id) = input.get("tie_id") {
                        let id = id.as_u64().ok_or("positive slide tie id required")?;
                        s.ties.iter().find(|tie| tie.id == id).map(|tie| tie.id)
                    } else {
                        let anchor = vector(&input["anchor"])?;
                        let distance = number(input, "max_distance", 2., 0.01, 8.)?;
                        s.ties
                            .iter()
                            .filter_map(|tie| {
                                let d = len(sub(tie.anchor, anchor));
                                (d <= distance).then_some((d, tie.id))
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                            .map(|(_, id)| id)
                    };
                    s.selected = Some(selected.ok_or("no existing slide tie at selection")?);
                }
                Some("radius") => s.adjust_radius(number(input, "steps", 0., -100., 100.)?),
                Some("clear") => s.clear(input["all"] == true),
                Some("material") => {
                    s.material = if s.material == 528 { 529 } else { 528 };
                    s.revision += 1;
                }
                _ => return Err("unknown waterslide edit".into()),
            }
            Ok(
                json!({"revision":s.revision,"selected":s.selected,"radius":s.radius,"material":s.material}),
            )
        }
        "Course" => match s.course_from(if input["start"].is_null() {
            None
        } else {
            Some(
                input["start"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or("course start must be a positive tie id")?,
            )
        }) {
            Ok(poses) => Ok(
                json!({"source_course_available":!poses.is_empty(),"poses":poses.into_iter().map(pose_value).collect::<Vec<_>>(),"revision":s.revision}),
            ),
            Err(reason) => Ok(
                json!({"poses":[],"revision":s.revision,"source_course_available":false,"reason":reason}),
            ),
        },
        "CollisionMesh" => {
            let m = s.mesh()?;
            Ok(
                json!({"vertices":m.vertices.iter().map(|p|p.position).collect::<Vec<_>>(),"normals":m.vertices.iter().map(|p|p.normal).collect::<Vec<_>>(),"materials":m.vertices.iter().map(|p|p.material).collect::<Vec<_>>(),"triangles":m.triangles,"revision":s.revision}),
            )
        }
        "Snapshot" => Ok(
            json!({"ties":s.ties.iter().map(|t|json!({"id":t.id,"anchor":t.anchor,"normal":t.normal,"forward":t.forward,"radius":t.radius})).collect::<Vec<_>>(),"segments":s.segments.iter().map(|p|json!({"start":p.start,"end":p.end,"material":p.material})).collect::<Vec<_>>(),"revision":s.revision}),
        ),
        "Close" => {
            *s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown water slide hook".into()),
    }
}
fn parse_slide_mesh(v: &Value) -> Result<slides::Mesh, String> {
    let positions: Vec<V> =
        serde_json::from_value(v["vertices"].clone()).map_err(|_| "invalid trough positions")?;
    let normals: Vec<V> =
        serde_json::from_value(v["normals"].clone()).map_err(|_| "invalid trough normals")?;
    let materials: Vec<u32> =
        serde_json::from_value(v["materials"].clone()).map_err(|_| "invalid trough materials")?;
    let triangles: Vec<[u32; 3]> =
        serde_json::from_value(v["triangles"].clone()).map_err(|_| "invalid trough triangles")?;
    if positions.len() > 262144
        || normals.len() != positions.len()
        || materials.len() != positions.len()
        || positions.iter().any(|p| !finite(*p))
        || triangles
            .iter()
            .flatten()
            .any(|i| *i as usize >= positions.len())
    {
        return Err("invalid bounded trough mesh".into());
    }
    Ok(slides::Mesh {
        vertices: positions
            .into_iter()
            .enumerate()
            .map(|(i, position)| slides::Vertex {
                position,
                normal: normals[i],
                uv: [0.; 2],
                material: materials[i],
            })
            .collect(),
        triangles,
    })
}
pub(crate) fn mesh_packet(frame: Frame, mesh: &surface_mesh::Mesh) -> Value {
    let rows: Vec<_> = mesh
        .vertices
        .iter()
        .map(|p| {
            let position = sub(p.position, frame.origin);
            json!([
                [position[0], position[1], position[2], 1.],
                [p.normal[0], p.normal[1], p.normal[2], 0.],
                [p.geometric[0], p.geometric[1], p.geometric[2], 0.],
                [p.axis[0], p.axis[1], p.axis[2], 0.],
                [p.uv[0], p.uv[1], p.flow[0], p.flow[1]],
                [p.depth, p.thickness, p.class, p.flags]
            ])
        })
        .collect();
    json!({"water_vertices":rows,"water_indices":mesh.indices,"truncated":mesh.truncated,"frame":{"origin":frame.origin,"right":frame.right,"up":frame.up,"forward":frame.forward}})
}
fn accept_snapshot(host: &mut Host, s:&mut super::hydrology::State, snapshot:Snapshot)->Result<Value,String> {
    if s.snapshot.is_some() && snapshot.revision < s.revision {
        return Err("stale accepted water collision snapshot".into());
    }
    let slide=host.invoke("WaterSlideSystem.CollisionMesh",Value::Null)?;
    let revision=slide["revision"].as_u64().ok_or("slide revision required")?;
    if s.snapshot.is_none() || s.revision != snapshot.revision || s.slide_revision != revision {
        s.update(snapshot,&parse_slide_mesh(&slide)?,revision);
    }
    Ok(json!({"revision":s.revision,"slide_revision":s.slide_revision,"coverage":s.snapshot.as_ref().map(|p|json!({"origin":p.origin,"min":p.min,"max":p.max})),"triangles":s.triangles.len()}))
}
pub(crate) fn update_numeric(host:&mut Host,snapshot:Snapshot)->Result<Value,String> {
    let mut s=std::mem::take(&mut host.water.hydrology);
    let result=accept_snapshot(host,&mut s,snapshot);
    host.water.hydrology=s;
    result
}
pub(crate) fn hydrology(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.water.hydrology);
    let result = (|| match hook {
        "AudioSources" => {
            let point = vector(&input["position"])?;
            let mean = host.invoke("TideSystem.Sample", Value::Null)?["height"].as_f64().ok_or("tide height required")?;
            let wave = ocean_snapshot(&host.invoke("OceanWaveSystem.Snapshot", Value::Null)?)?;
            let parcels = host.invoke("SoftBodyWaterSystem.Snapshot", Value::Null)?;
            let material = host.invoke("PhysicsSystem.GameWater", Value::Null)?;
            Ok(super::ambient::discover(&s, point, mean, &wave, host.water.flow.settings.river_push, &parcels, &material))
        }
        "MaterialDomain" => {
            if input["clear"]==true{s.material_domains.clear();return Ok(json!({"cleared":true}));}
            let domain=crate::terrain::material_domain::Domain::parse(input).ok_or("water material domain required")?;
            if s.material_domains.len()>=4096{return Err("material water storage limit reached".into());}
            s.material_domains.push(domain);Ok(json!({"accepted":true}))
        }
        "Configure" => {
            s.bodies = super::bodies::State::parse(input)?;
            s.surface_cache=None;
            Ok(Value::Null)
        }
        "Update" => {
            let snapshot = if input["snapshot"].is_null() {
                s.snapshot
                    .clone()
                    .ok_or("water collision snapshot not supplied")?
            } else {
                Snapshot::parse(&input["snapshot"])?
            };
            accept_snapshot(host,&mut s,snapshot)
        }
        "Raycast" => {
            let hit = s.ray(
                vector(&input["position"])?,
                vector(&input["direction"])?,
                number(input, "distance", 8., 0.01, 1024.)?,
            );
            Ok(hit.map_or(
                Value::Null,
                |h| json!({"position":h.point,"normal":h.normal,"fraction":h.fraction}),
            ))
        }
        "Sample" | "SurfaceMesh" | "MaterialPreview" => {
            let mean = host.invoke("TideSystem.Sample", Value::Null)?["height"]
                .as_f64()
                .ok_or("tide height required")?;
            let wave = ocean_snapshot(&host.invoke("OceanWaveSystem.Snapshot", Value::Null)?)?;
            if hook=="MaterialPreview" {
                let positions=input["positions"].as_array().filter(|a|a.len()<=1000).ok_or("bounded water transfer samples required")?;
                let mut occupied=Vec::new();for p in positions{let p=vector(p)?;occupied.push(!s.material_domains.iter().any(|d|d.distance(p)<=0.)&&s.bodies.query(p,mean,&wave,&s).is_some());}
                return Ok(json!({"occupied":occupied}));
            }
            let point = vector(&input["position"])?;
            if hook == "Sample" {
                let material=host.invoke("PhysicsSystem.GameWaterSample",input.clone())?;
                if material["occupied"]==true{return Ok(material);}
                let wet = if s.material_domains.iter().any(|d|d.distance(point)<=0.){None}else{s.bodies.query(point, mean, &wave, &s)};
                Ok(wet.map_or(json!({"occupied":false,"coverage_known":s.covered(point,0.)}),|w|json!({"occupied":true,"coverage_known":true,"frame":{"origin":w.frame.origin,"right":w.frame.right,"up":w.frame.up,"forward":w.frame.forward},"local_point":w.point,"uv":w.uv,"surface_height":w.surface_height,"surface_position":mul(unit(point),crate::terrain::RADIUS+w.surface_height),"mean_height":w.mean_height,"bed_height":w.bed_height,"shore_distance":w.shore_distance,"shore_gradient":w.shore_gradient,"shore_direction":w.shore_direction,"downstream_direction":w.downstream_direction,"wave_amplitude":w.wave_amplitude,"kind":match w.kind{super::flow::Kind::River=>"river",super::flow::Kind::Lake=>"lake",_=>"ocean"}})))
            } else {
                let surface=extract_surface(host,&mut s,input,point,mean,&wave)?;
                let mut packet=mesh_packet(surface.frame,&surface.mesh);
                packet["reflection_allowed"]=json!(surface.reflection_allowed);packet["mean_height"]=json!(surface.mean_height);
                Ok(packet)
            }
        }
        "Drainage" => Ok(crate::terrain::drainage::report()),
        "Revision" => Ok(json!({"revision":s.revision,"slide_revision":s.slide_revision})),
        "Snapshot" => Ok(
            json!({"snapshot":s.snapshot.as_ref().map(|p|json!({"origin":p.origin,"vertices":p.vertices,"triangles":p.triangles,"revision":p.revision,"coverage":{"min":p.min,"max":p.max}})),"sources":s.bodies.sources.iter().map(|p|json!({"id":p.id,"frame":{"origin":p.frame.origin,"right":p.frame.right,"up":p.frame.up,"forward":p.frame.forward},"min":p.min,"max":p.max,"target":p.target,"velocity":p.velocity})).collect::<Vec<_>>()}),
        ),
        "Close" => {
            s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown surface hydrology hook".into()),
    })();
    host.water.hydrology = s;
    result
}
fn parcel_snapshot(s: &soft_body::State) -> Value {
    json!({"radius":s.settings.radius,"revision":s.revision,"time":s.time,"emitted":s.emitted,"returned":s.removed,"parcel_count":s.parcels.len(),"conserved":s.emitted.saturating_sub(s.removed)==s.parcels.len()as u64,"coverage_needed":s.coverage_needed,"coverage_points":s.coverage_points,"parcels":s.parcels.iter().map(|p|json!({"id":p.id,"position":p.position,"velocity":p.velocity,"resting":p.resting,"covered":p.covered,"impact":p.impact})).collect::<Vec<_>>(),"whitewater":super::whitewater::emitters(&s.parcels).into_iter().map(|p|json!({"position":p.position,"velocity":p.velocity,"strength":p.strength})).collect::<Vec<_>>()})
}
fn parse_parcels(v: &Value) -> Result<soft_body::State, String> {
    let mut s = soft_body::State::default();
    s.settings.radius = number(v, "radius", 0.48, 0.01, 10.)?;
    for row in v["parcels"]
        .as_array()
        .filter(|p| p.len() <= 1000)
        .ok_or("bounded parcel snapshot required")?
    {
        let id = s.spawn(
            vector(&row["position"])?,
            vector(&row["velocity"])?,
            None,
            None,
        )?;
        let p = s.parcels.iter_mut().find(|p| p.id == id).unwrap();
        p.covered = row["covered"] == true;
        p.resting = row["resting"] == true;
        p.impact = number(row, "impact", 0., 0., 1.)?;
    }
    Ok(s)
}
fn contact_cache(
    host: &mut Host,
    s: &mut soft_body::State,
) -> Result<Option<Box<super::hydrology::State>>, String> {
    let revision = host.invoke("TerrainSurfaceHydrologySystem.Revision", Value::Null)?;
    if let Some(cached) = s.contact_cache.take() {
        if revision["revision"].as_u64() == Some(cached.revision)
            && revision["slide_revision"].as_u64() == Some(cached.slide_revision)
        {
            return Ok(Some(cached));
        }
    }
    host.water_contact_snapshot()
}
pub(crate) fn soft_water(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.water.soft_body);
    let result = (|| match hook {
        "Configure" => {
            let next = settings(input)?.soft_body;
            if (!s.parcels.is_empty() && next.radius != s.settings.radius)
                || next.capacity < s.parcels.len()
            {
                return Err(
                    "live water material cannot change radius or discard parcels through capacity"
                        .into(),
                );
            }
            s.settings = next;
            Ok(Value::Null)
        }
        "Step" => {
            let delta=dt(input)?;
            s.coverage_needed = false;
            s.coverage_points.clear();
            // No discrete water and no emitters means no contact work exists.
            // Continuous ocean/river rendering and queries are independent.
            if s.parcels.is_empty()&&host.water.hydrology.bodies.sources.is_empty(){return Ok(parcel_snapshot(&s));}
            let Some(contact) = contact_cache(host, &mut s)? else {
                s.coverage_needed = true;
                return Ok(parcel_snapshot(&s));
            };
            let result = (|| {
                s.fill_sources(&contact.bodies.sources, &contact)?;
                s.step(delta, &contact)?;
                Ok(parcel_snapshot(&s))
            })();
            s.contact_cache = Some(contact);
            result
        }
        "Gates" => {
                let mut gate = super::floodgate::State::default();
                gate.settings = settings(&json!({"floodgate":input["settings"]}))?.floodgate;
                gate.next_slot = input["next_slot"].as_u64().unwrap_or(0) as usize;
                gate.spawned = input["spawned"].as_u64().unwrap_or(0);
                gate.recycled = input["recycled"].as_u64().unwrap_or(0);
                gate.returned = input["returned"].as_u64().unwrap_or(0);
                for p in input["course"]["poses"]
                    .as_array()
                    .filter(|p| p.len() <= 200000)
                    .ok_or("bounded gate course required")?
                {
                    gate.course.push(slides::Pose {
                        position: vector(&p["position"])?,
                        right: vector(&p["right"])?,
                        up: vector(&p["up"])?,
                        forward: vector(&p["forward"])?,
                        radius: number(p, "radius", 0.65, 0.05, 100.)?,
                    });
                }
                if !gate.active(){gate.return_owned(&mut s);}else{
                    let Some(contact)=contact_cache(host,&mut s)?else{s.coverage_needed=true;return Ok(json!({"active":false,"coverage_needed":true}));};
                    let result=gate.step(&mut s,&contact);s.contact_cache=Some(contact);result?;
                }
                Ok(
                    json!({"active":gate.active(),"next_slot":gate.next_slot,"spawned":gate.spawned,"recycled":gate.recycled,"returned":gate.returned,"coverage_needed":s.coverage_needed,"coverage_points":s.coverage_points}),
                )
        }
        "Sample" => {
            let (occupied, velocity, impact) = s.sample(
                vector(&input["position"])?,
                number(input, "radius", 1.15, 0.01, 10.)?,
            );
            Ok(json!({"occupied":occupied,"velocity":velocity,"whitewater":impact}))
        }
        "Disturb" => {
            let n = s.disturb(
                vector(&input["position"])?,
                number(input, "radius", 1.5, 0.01, 100.)?,
                vector(&input["impulse"])?,
            );
            Ok(json!({"woken":n}))
        }
        "Snapshot" => Ok(parcel_snapshot(&s)),
        "Close" => {
            s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown soft water hook".into()),
    })();
    host.water.soft_body = s;
    result
}
pub(crate) fn floodgate(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.water.floodgate);
    let result = (|| match hook {
        "Configure" => {
            if input["operation"] == "select_course" {
                let start = if input["start"].is_null() {
                    None
                } else {
                    Some(
                        input["start"]
                            .as_u64()
                            .filter(|id| *id > 0 && *id <= 1_000_000_000_000)
                            .ok_or("source start must be a positive tie id")?,
                    )
                };
                if let Some(start) = start {
                    let course = host.invoke("WaterSlideSystem.Course", json!({"start":start}))?;
                    if course["source_course_available"] != true
                        || course["poses"].as_array().is_none_or(|p| p.len() < 2)
                    {
                        return Err("source requires an existing open authored course".into());
                    }
                }
                s.settings.course_start = start;
                s.revision = s.revision.wrapping_add(1);
            } else {
                s.settings = settings(input)?.floodgate;
            }
            Ok(json!({"course_start":s.settings.course_start,"revision":s.revision}))
        }
        "Step" => {
            let course = if let Some(start) = s.settings.course_start {
                host.invoke("WaterSlideSystem.Course", json!({"start":start}))?
            } else {
                json!({"poses":[],"source_course_available":false,"reason":"no continuous source course configured"})
            };
            s.course.clear();
            for p in course["poses"].as_array().ok_or("slide course required")? {
                s.course.push(slides::Pose {
                    position: vector(&p["position"])?,
                    right: vector(&p["right"])?,
                    up: vector(&p["up"])?,
                    forward: vector(&p["forward"])?,
                    radius: number(p, "radius", 0.65, 0.05, 100.)?,
                });
            }
            let result=host.invoke("SoftBodyWaterSystem.Gates",json!({"course":course,"next_slot":s.next_slot,"spawned":s.spawned,"recycled":s.recycled,"returned":s.returned,"settings":{"enabled":s.settings.enabled,"target":s.settings.target,"spawn_per_step":s.settings.spawn_per_step,"initial_speed":s.settings.initial_speed,"course_start":s.settings.course_start}}))?;
            s.next_slot = result["next_slot"].as_u64().unwrap_or(s.next_slot as u64) as usize;
            s.spawned = result["spawned"].as_u64().unwrap_or(s.spawned);
            s.recycled = result["recycled"].as_u64().unwrap_or(s.recycled);
            s.returned = result["returned"].as_u64().unwrap_or(s.returned);
            Ok(result)
        }
        "Close" => {
            s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown floodgate hook".into()),
    })();
    host.water.floodgate = s;
    result
}

pub(crate) struct Surface {pub frame:Frame,pub mesh:surface_mesh::Mesh,pub reflection_allowed:bool,pub mean_height:f64}
fn extract_surface(host:&mut Host,s:&mut super::hydrology::State,input:&Value,point:V,mean:f64,wave:&super::ocean::State)->Result<Surface,String>{
                let visibility=if std::env::var("WORMWOOD_WATER_VIEW_CULLING").as_deref()==Ok("0"){None}else{super::visibility::View::parse(&input["view"])};
                let (frame, mut mesh) = s.free_surface_visible(point,vector(&input["forward"])?,mean,wave,visibility.as_ref());
                mesh.remove_material_domains(&s.material_domains);
                let live=host.invoke("PhysicsSystem.GameWater",Value::Null)?;
                let vertices:Vec<[f64;3]>=serde_json::from_value(live["vertices"].clone()).map_err(|_|"material water triangles required")?;
                mesh.material_water(&vertices,frame);
                let parcels = host.invoke("SoftBodyWaterSystem.Snapshot", Value::Null)?;
                surface_mesh::parcels(&mut mesh, frame, &parse_parcels(&parcels)?);
                let mut plane = None;
                let mut reflection_allowed = true;
                for vertex in &mesh.vertices {
                    let level = match vertex.class {
                        1 => mean,
                        2 => len(vertex.position) - crate::terrain::RADIUS,
                        _ => {
                            reflection_allowed = false;
                            continue;
                        }
                    };
                    if plane.is_some_and(|previous: f64| (previous - level).abs() > 0.02) {
                        reflection_allowed = false;
                    } else {
                        plane = Some(level);
                    }
                }
    Ok(Surface{frame,mesh,reflection_allowed,mean_height:plane.unwrap_or(mean)})
}
pub(crate) fn surface_numeric(host:&mut Host,input:&Value)->Result<Surface,String>{
    let mut s=std::mem::take(&mut host.water.hydrology);
    let result=(||{let point=vector(&input["position"])?;
        let mean=host.invoke("TideSystem.Sample",Value::Null)?["height"].as_f64().ok_or("tide height required")?;
        let wave=ocean_snapshot(&host.invoke("OceanWaveSystem.Snapshot",Value::Null)?)?;
        extract_surface(host,&mut s,input,point,mean,&wave)})();host.water.hydrology=s;result
}
