//! Native finite review of regional terrain and cities through production composition.
use super::Session;
use crate::audio::{V, cross, mul, sub, unit};
use crate::terrain::{RADIUS, regions as r};
use serde_json::json;
use std::path::Path;
fn point(east: f64, north: f64, above: f64) -> V {
    let n = unit([east + r::TOWN_X, RADIUS, north + r::TOWN_Z]);
    mul(n, RADIUS + crate::terrain::height(n) + above)
}
pub(super) fn run(root: &Path, output: &Path, backend: &str) -> Result<(), String> {
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let entity = crate::base_entity::Entity::from_spec(
        1,
        &json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}),
    )?;
    let mut session = Session::prepare(root, output, backend, &entity)?;
    session.environment("planet_surface");
    session.texture(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
    let southern_point=|x:f64,s:f64,h:f64| {let n=r::southern_normal(x,s);mul(n,RADIUS+crate::terrain::height(n)+h)};
    let cases = [
        ("jungle-city",southern_point(-500.,r::JUNGLE_CITY_SOUTH-650.,200.),southern_point(0.,r::JUNGLE_CITY_SOUTH,50.)),
        ("jungle-walkway",southern_point(6.,r::JUNGLE_CITY_SOUTH-35.,7.),southern_point(0.,r::JUNGLE_CITY_SOUTH+30.,0.)),
        ("southern-snow",southern_point(0.,106_000.,60.),southern_point(20.,106_010.,0.)),
        ("northwest-forest", point(-1_500., -4_000., 180.), point(-2_000., -4_700., 20.)),
        ("northern-sea", point(0., -6_150., 100.), point(0., -8_000., 15.)),
        ("northern-snow", point(1_000., -42_000., 80.), point(700., -43_000., 10.)),
        ("northern-ice-city", point(-700., -r::ICE_CITY_NORTH + 750., 240.), point(0., -r::ICE_CITY_NORTH, 55.)),
        ("plains", point(0., 0., 14.), point(-1_000., 0., 12.)),
        ("forest-edge", point(800., 0., 30.), point(1_500., 0., 40.)),
        (
            "western-ridge",
            point(-5_400., -900., 350.),
            point(-8_000., 0., 200.),
        ),
        (
            "stained-glass-city",
            point(r::EAST_CITY - 700., -750., 240.),
            point(r::EAST_CITY, 0., 55.),
        ),
        (
            "utopian-city",
            point(r::WEST_CITY - 700., -750., 300.),
            point(r::WEST_CITY, 0., 70.),
        ),
        (
            "eastern-coast",
            point(r::COAST - 250., -400., 100.),
            point(r::COAST + 150., 0., 15.),
        ),
        (
            "coastal-cliffs",
            point(r::shoreline(-1_000.) + 120., -1_000., 35.),
            point(r::shoreline(-1_000.) - 60., -1_000., 5.),
        ),
    ];
    let mut reports = Vec::new();
    let selected = std::env::var("WORMWOOD_REGION_VIEW").ok();
    for (name, eye, target) in cases {
        if selected
            .as_deref()
            .is_some_and(|list| !list.split(',').any(|item| item == name))
        {
            continue;
        }
        eprintln!("Regional review: {name}");
        let forward = unit(sub(target, eye));
        let right = unit(cross(forward, unit(eye)));
        let up = unit(cross(right, forward));
        let view = json!({"entity":1,"position":eye,"forward":forward,"right":right,"up":up,"yaw":0.,"pitch":0.,"aspect":1.5,"calendar":{"day_of_year":190,"year":2026},"day_night":{"fraction":0.5}});
        let scene = crate::base_system::layer_four::appearance_system::regional_review_scene(
            root,
            view.clone(),
        )?;
        session.table_eye(&scene.view)?;
        session.set_batch(&scene.triangles)?;
        session.set_vegetation(root, scene.vegetation)?;
        if matches!(name, "eastern-coast" | "coastal-cliffs" | "northern-sea") {
            // Use sampled production ground as the water pass's finite collision bed.
            let (water_x, water_z) = if name == "northern-sea" { (0., -6_500.) } else { (r::COAST, 0.) };
            let anchor = point(water_x, water_z, 0.);
            let mut vertices = Vec::new();
            let mut triangles = Vec::new();
            let size = 81usize;
            for j in 0..size {
                for i in 0..size {
                    let p = point(
                        water_x - 800. + i as f64 * 25.,
                        water_z - 1_000. + j as f64 * 25.,
                        0.,
                    );
                    vertices.push(sub(p, anchor).map(|v| v as f32));
                }
            }
            for j in 0..size - 1 {
                for i in 0..size - 1 {
                    let a = (j * size + i) as u32;
                    triangles.extend([
                        [a, a + size as u32, a + 1],
                        [a + 1, a + size as u32, a + size as u32 + 1],
                    ]);
                }
            }
            let snapshot = crate::water::types::Snapshot {
                origin: anchor,
                vertices,
                triangles,
                revision: 1,
                min: [-2_000.; 3],
                max: [2_000.; 3],
            };
            let mut terrain = crate::water::hydrology::State::default();
            terrain.update(snapshot, &Default::default(), 0);
            let bodies = crate::water::bodies::State::parse(
                &json!({"ocean":true,"generated":true,"rivers":[],"sources":[]}),
            )?;
            let mut wave = crate::water::ocean::State::default();
            wave.settings.amplitude = 2.2;
            wave.settings.shore_gain = 2.;
            wave.time = 2.;
            let (frame, mesh) = crate::water::surface_mesh::free_surfaces(
                eye, forward, &bodies, -3.5, &wave, &terrain,
            );
            let mut packet = crate::water::runtime::mesh_packet(frame, &mesh);
            let o = crate::water::optics::Settings::default();
            packet["optics"] = json!({"time":2.,"murkiness":0.,"sky_color_intensity":o.sky_color_intensity,"neutral_reflection_intensity":o.neutral_reflection_intensity,"sky_top":o.sky_top,"sky_bottom":o.sky_bottom,"light":o.light,"planar_reflection":false,"micro_phase":[0.,0.,0.],"mean_height":-3.5});
            packet["ripples"] = json!([]);
            session.set_water(root, &json!([packet]), &scene.view, &scene.triangles)?;
        } else {
            session.set_water(root, &json!([]), &scene.view, &scene.triangles)?;
        }
        session.capture_next();
        let frame = session.pump(true, true)?;
        std::fs::copy(
            output.join(format!("triangle-{backend}.ppm")),
            output.join(format!("{name}.ppm")),
        )
        .map_err(|e| e.to_string())?;
        reports.push(json!({"name":name,"triangles":scene.triangles.len(),"frame":frame}));
    }
    session.finish()?;
    std::fs::write(
        output.join("regional-review.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
