//! Bounded native review of the production sky and unchanged cloud compositor.
use super::Session;
use serde_json::json;
use std::path::Path;

pub(super) fn run(root: &Path, output: &Path, backend: &str) -> Result<(), String> {
    use crate::base_system::layer_four::{atmosphere::Lighting, cloud_system};
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let entity = crate::base_entity::Entity::from_spec(1, &json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
    let mut session = Session::prepare(root, output, backend, &entity)?;
    session.environment("planet_surface");
    session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
    for (name, hour, yaw, pitch, clouds) in [
        ("midnight", 0., 0., 0.65, false),
        ("midnight-east", 0., 1.57, 0.65, false),
        ("midnight-zenith", 0., 0., 1.50, false),
        ("midnight-clouds", 0., 0., 0.65, true),
        ("dawn", 5.5, 0., 0.65, false),
        ("noon", 12., 0., 0.65, false),
        ("before-midnight", 23.9999, 0., 0.65, false),
    ] {
        let mut view = json!({"entity":1,"position":[0.,6371002.,0.],"yaw":yaw,"pitch":pitch,"aspect":1.5,"day_night":{"fraction":hour/24.},"calendar":{"unix_seconds":1794441600.,"day_of_year":316.}});
        if clouds { view["clouds"] = cloud_system::dispatch(&view)?; }
        let triangles = Lighting::from_view(&view).sky(&view);
        session.table_eye(&view)?;
        session.set_batch(&triangles)?;
        session.capture_next();
        session.pump(true, true)?;
        std::fs::copy(output.join(format!("triangle-{backend}.ppm")), output.join(format!("{name}.ppm"))).map_err(|e| e.to_string())?;
    }
    session.finish()?;
    Ok(())
}
