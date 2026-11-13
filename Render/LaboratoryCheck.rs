//! Finite native visual review. State-machine gameplay is tested separately.
use super::Session;
use crate::audio::{add,sub,unit,cross};
use serde_json::{Value,json};
use std::{path::Path,sync::Arc,collections::HashMap};
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String> {
    std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
    let mut venue=crate::theatre::load(root)?;let origin=crate::theatre::waterpark::theatre_origin();
    let n=unit([0.,crate::terrain::RADIUS,-94.]);let outside=(n[1]*(crate::terrain::RADIUS+crate::terrain::height(n))-origin[1]).min(0.);
    venue.add_approach(outside)?;venue.add_waterpark(root,crate::theatre::waterpark::site_offset(origin),outside,origin)?;venue.add_town(root,origin)?;
    let civic=venue.add_civic_buildings(root,origin,outside)?;venue.add_marsh(root,origin,civic)?;venue.add_plan(root,origin,civic)?;venue.add_laboratory(root,origin)?;
    let venue=Arc::new(venue);let site=venue.laboratory.as_ref().unwrap();
    let base=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
    let mesh:Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/Assets/Laboratory/prismasqueek.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mouse=crate::base_entity::Entity::from_spec(2,&json!({"kind":"visual","shader":"material_surface","vertices":mesh["vertices"]}))?;
    let researcher=crate::base_entity::Entity::from_spec(3,&crate::base_system::layer_four::npc_appearance::spec([0.2,0.85,0.65]))?;
    let entities=HashMap::from([(2,Arc::new(mouse)),(3,Arc::new(researcher))]);
    let mut session=Session::prepare(root,output,backend,&base)?;session.environment("planet_surface");session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
    let cases=[
        ("laboratory-front",add(site.lab,[0.,4.,-25.]),add(site.lab,[0.,3.,0.]),site.mouse_start(),false),
        ("laboratory-meeting",add(site.lab,[0.,2.,-6.]),add(site.lab,[-2.,0.9,3.]),site.mouse_start(),false),
        ("prismasqueek",add(site.mouse_start(),[1.4,0.9,-2.3]),add(site.mouse_start(),[0.,0.65,0.]),site.mouse_start(),false),
        ("yarn-portal-to-zoo",add(site.lab_portal,[0.,0.2,-5.]),site.lab_portal,add(site.zoo,[0.,0.,11.]),true),
        ("zoo-home",add(site.zoo,[8.,2.8,-12.]),add(site.zoo_home(),[0.,0.6,0.]),site.zoo_home(),true),
        ("yarn-portal-to-lab",add(site.zoo_portal,[0.,0.2,-5.]),site.zoo_portal,site.zoo_home(),true),
    ];
    let mut reports=Vec::new();
    for (name,eye,target,mouse,open) in cases {
        let forward=unit(sub(target,eye));let right=unit(cross(forward,[0.,1.,0.]));let up=unit(cross(right,forward));
        let actors=json!([{"entity":2,"origin":add(origin,mouse),"laboratory":true,"prismasqueek":true,"yaw":if mouse==site.mouse_start(){std::f64::consts::PI}else{0.},"gait":0.},{"entity":3,"origin":add(origin,site.researcher()),"laboratory":true}]);
        let view=json!({"entity":1,"position":add(origin,eye),"table_eye":eye,"yaw":0.,"pitch":0.,"right":right,"up":up,"forward":forward,"aspect":4./3.,"day_night":{"fraction":0.5},"laboratory_portals":open,"instances":actors});
        let mut triangles=crate::base_system::layer_four::appearance_system::theatre_shared_mesh(origin,&venue,&view)?;
        for actor in view["instances"].as_array().unwrap() {
            let e=&entities[&actor["entity"].as_u64().unwrap()];let crate::base_entity::Appearance::Visual{vertices,..}=&e.appearance else{continue;};let mut vertices=vertices.clone();
            if actor["prismasqueek"]==true {crate::base_system::layer_four::laboratory_appearance::shade_mouse(&mut vertices,actor,&view)?;}
            let p=crate::audio::vector(&actor["origin"])?;let camera=crate::audio::vector(&view["position"])?;
            for v in &mut vertices{for k in 0..3{v[k]=(v[k]as f64+p[k]-camera[k])as f32;}}
            let mut local=view.clone();local["position"]=json!([0.,0.,0.]);local["two_sided"]=json!(true);
            triangles.extend(crate::base_system::layer_four::appearance_system::projection::project(&vertices,&local));
        }
        triangles.extend(crate::base_system::layer_four::laboratory_appearance::portal_view(site,origin,&view,&entities)?);
        let sky=crate::base_system::layer_four::atmosphere::Lighting::from_view(&view).sky(&view);
        crate::base_system::layer_four::appearance_system::insert_sky_after_opaque(&mut triangles,sky);
        session.table_eye(&view)?;session.set_batch(&triangles)?;session.capture_next();let frame=session.pump(true,true)?;
        std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
        reports.push(json!({"name":name,"triangles":triangles.len(),"frame":frame}));
    }
    session.finish()?;
    std::fs::write(output.join("laboratory-review.json"),serde_json::to_vec_pretty(&reports).unwrap()).map_err(|e|e.to_string())?;
    Ok(())
}
