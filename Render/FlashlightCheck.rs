//! Native beam review on large untextured faces using production projection,
//! phone lighting, geometry visibility and renderer. No device audio is started.
use super::Session;
use serde_json::json;
use std::path::Path;
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String>{
    std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
    let entity=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
    let mut session=Session::prepare(root,output,backend,&entity)?;
    session.environment("planet_surface");
    session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
    // A single large wall and floor: corners alone cannot describe the beam.
    let mut vertices=Vec::new();
    for quad in [ [[-12.,-2.,-8.],[12.,-2.,-8.],[12.,12.,-8.],[-12.,12.,-8.]],
        [[-12.,-2.,-0.3],[12.,-2.,-0.3],[12.,-2.,-8.],[-12.,-2.,-8.]] ] {
        for i in [0,1,2,0,2,3] {let p=quad[i];vertices.push([p[0],p[1],p[2],0.02,0.025,0.03]);}
    }
    let blocker=crate::audio::geometry::Snapshot::parse(&json!({"revision":1,"origin":[0,0,0],"triangles":[],
        "solids":[{"min":[-12,-12,-2.3],"max":[-0.8,12,-2.18]}]}))?;
    let mut reports=vec![];
    for (name,on,pitch,shadow) in [("off",false,0.,false),("aim-level",true,0.,false),
        ("aim-down",true,-0.35,false),("partial-shadow",true,0.,true)] {
        let view=json!({"entity":1,"position":[0,0,0],"yaw":0.,"pitch":pitch,"aspect":1.5,"two_sided":true,
            "phone":{"equipped":true,"flashlight_on":on}});
        let mut triangles=crate::base_system::layer_four::appearance_system::projection::project(&vertices,&view);
        let original=triangles.len();
        let start=std::time::Instant::now();
        crate::phone::flashlight::illuminate(&mut triangles,&view,
            if shadow{Some(&blocker as &dyn crate::audio::geometry::Geometry)}else{None})?;
        let micros=start.elapsed().as_micros();
        session.set_batch(&triangles)?;session.capture_next();let frame=session.pump(true,true)?;
        std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
        reports.push(json!({"name":name,"original_triangles":original,"lit_triangles":triangles.len(),
            "cpu_microseconds":micros,"frame":frame,"view":view}));
    }
    session.finish()?;
    std::fs::write(output.join("report.json"),serde_json::to_vec_pretty(&reports).unwrap()).map_err(|e|e.to_string())?;
    Ok(())
}
