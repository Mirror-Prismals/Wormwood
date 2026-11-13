//! Finite native material review using the production venue loader, projection and shaders.
use super::Session;
use serde_json::json;
use std::path::Path;
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String>{
    if std::env::var("WORMWOOD_CHECK_NIGHT_SKY").as_deref()==Ok("1"){return super::night_sky_check::run(root,output,backend);}
    if std::env::var("WORMWOOD_CHECK_MONASTERY").as_deref()==Ok("1"){return super::monastery_check::run(root,output,backend);}
    if std::env::var("WORMWOOD_CHECK_LABORATORY").as_deref()==Ok("1"){return super::laboratory_check::run(root,output,backend);}
    if std::env::var("WORMWOOD_CHECK_REGIONS").as_deref()==Ok("1"){return super::regional_check::run(root,output,backend);}
    if std::env::var("WORMWOOD_CHECK_NIGHTWORLD").as_deref()==Ok("1"){return super::nightworld_check::run(root,output,backend);}
    use crate::audio::{add,sub,unit,cross};
    std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
    let mut venue=crate::theatre::load(root)?;
    let civic_check=std::env::var("WORMWOOD_CHECK_CIVIC").as_deref()==Ok("1");
    let mut civic_floor=0.;
    if civic_check {
        let origin=crate::theatre::waterpark::theatre_origin();
        venue.add_approach(-1.)?;venue.add_waterpark(root,crate::theatre::waterpark::site_offset(origin),-1.,origin)?;
        venue.add_town(root,origin)?;civic_floor=venue.add_civic_buildings(root,origin,-1.)?;venue.add_marsh(root,origin,civic_floor)?;
    }
    let park_check=std::env::var("WORMWOOD_CHECK_WATERPARK").as_deref()==Ok("1");
    if park_check {let origin=crate::theatre::waterpark::theatre_origin();venue.add_approach(-1.)?;venue.add_waterpark(root,crate::theatre::waterpark::site_offset(origin),-1.,origin)?;}
    let entity=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
    let mut session=Session::prepare(root,output,backend,&entity)?;
    session.environment("planet_surface");
    session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
    let mut reports=vec![];
    let lighting_check=std::env::var("WORMWOOD_CHECK_STAGE_LIGHTING").as_deref()==Ok("1");
    let cases=if civic_check {vec![
        ("civic-church-exterior",[-385.,civic_floor+90.,-245.],[-240.,civic_floor+35.,0.],0.5),
        ("civic-sanctuary",[-240.,civic_floor+8.,-30.],[-240.,civic_floor+23.,87.],0.5),
        ("civic-balcony",[-240.,civic_floor+14.,-60.],[-240.,civic_floor+20.,75.],0.5),
        ("civic-commissary",[130.,civic_floor+140.,-225.],[330.,civic_floor+10.,0.],0.5),
        ("civic-food-hall",[325.,civic_floor+5.,-80.],[225.,civic_floor+3.,30.],0.5),
        ("civic-greenhouse",[350.,civic_floor+9.,35.],[440.,civic_floor+5.,100.],0.5),
    ]}else if park_check {vec![
        ("waterpark-overview",[-110.,110.,-110.],[0.,40.,-247.],0.5),
        ("waterpark-road",[0.,3.,-99.],[0.,26.,-202.],0.5),
        ("waterpark-launch",[-15.,70.,-207.],[0.,65.,-231.],0.5),
        ("waterpark-cave",[25.,-64.,-272.],[-10.,-72.,-292.],0.5),
        ("waterpark-stair-exit",[44.,-65.,-264.],[44.,-55.,-257.],0.5),
        ("waterpark-splash",[-35.,20.,-205.],[-22.,16.,-231.],0.5),
    ]} else if lighting_check {vec![
        ("stage-default",[0.,11.,7.],[0.,4.,35.],0.5),
        ("stage-red",[0.,11.,7.],[0.,4.,35.],0.5),
        ("stage-blue",[0.,11.,7.],[0.,4.,35.],0.5),
        ("stage-blackout",[0.,11.,7.],[0.,4.,35.],0.5),
        ("fixture-close",[-14.,14.,14.],[-15.,14.,17.],0.5),
    ]}else{vec![
        ("entry-day",[13.,2.5,-91.],[0.,7.,-75.],0.5),
        ("entry-night",[13.,2.5,-91.],[0.,7.,-75.],0.0),
        ("stone-close",[11.,3.,-79.],[10.,3.4,-75.],0.5),
        ("entry-far",[24.,6.,-125.],[0.,8.,-75.],0.5),
        ("interior",[1.,9.,-52.],[0.,7.,24.],0.5),
        ("carpet-close",[5.,1.65,-7.],[5.,1.,-7.8],0.5),
        ("carpet-central-ramp",[0.,2.2,-3.],[0.,1.,0.],0.5),
        ("carpet-foh-ramp",[0.,7.,-43.],[0.,5.2,-42.],0.5),
        ("carpet-lobby-ramp",[0.,4.5,-64.],[0.,3.,-63.5],0.5),
    ]};
    for (name,eye,target,fraction) in cases {
        let forward=unit(sub(target,eye));let right=unit(cross(forward,[0.,1.,0.]));let up=unit(cross(right,forward));
        let venue_origin=if park_check || civic_check {crate::theatre::waterpark::theatre_origin()}else{[0.;3]};
        let mut view=json!({"entity":1,"position":add(eye,venue_origin),"table_eye":eye,"yaw":0.,"pitch":0.,"right":right,"up":up,"forward":forward,"aspect":1.5,"day_night":{"fraction":fraction}});
        if lighting_check {
            let mut levels=crate::theatre::lighting::defaults();
            for l in &mut levels {if name=="stage-red"{*l=[1.,0.,0.,0.8];}if name=="stage-blue"{*l=[0.,0.,1.,0.8];}if name=="stage-blackout"{l[3]=0.;}}
            view["stage_lighting"]=json!(levels);
        }
        let mut triangles=crate::base_system::layer_four::appearance_system::theatre_mesh(venue_origin,&venue.vertices,&view)?;
        if park_check && name=="waterpark-cave" {
            let profile:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/caves.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            if let Some(field)=crate::terrain::underground::Field::parse(&profile)? {
                let mesh=field.mesh(add(eye,venue_origin),&[])?;
                let vertices:Vec<[f32;7]>=mesh.iter().flatten().map(|v|[(v[0]-venue_origin[0]) as f32,(v[1]-venue_origin[1]) as f32,(v[2]-venue_origin[2]) as f32,v[3] as f32,v[4] as f32,v[5] as f32,0.]).collect();
                triangles.extend(crate::base_system::layer_four::appearance_system::theatre_mesh(venue_origin,&vertices,&view)?);
            }
        }
        if park_check || civic_check {
            let sky=crate::base_system::layer_four::atmosphere::Lighting::from_view(&view).sky(&view);
            crate::base_system::layer_four::appearance_system::insert_sky_after_opaque(&mut triangles,sky);
        }
        // A fully clipped exterior is correctly absent from an interior view.
        if !venue.vertices.iter().any(|v|v[6]==112.){return Err("exterior tag missing from venue".into());}
        session.table_eye(&view)?;
        if std::env::var("WORMWOOD_CHECK_SCENE_ORDER").as_deref()==Ok("1") {
            let sky=crate::base_system::layer_four::atmosphere::Lighting::from_view(&view).sky(&view);
            let mut reference=sky.clone();reference.extend_from_slice(&triangles);
            session.set_batch(&reference)?;session.capture_next();session.pump(true,true)?;
            let capture=output.join(format!("triangle-{backend}.ppm"));
            let before=std::fs::read(&capture).map_err(|e|e.to_string())?;
            std::fs::write(output.join(format!("{name}-sky-first.ppm")),&before).map_err(|e|e.to_string())?;
            let mut ordered=triangles.clone();
            crate::base_system::layer_four::appearance_system::insert_sky_after_opaque(&mut ordered,sky);
            session.set_batch(&ordered)?;session.capture_next();session.pump(true,true)?;
            let after=std::fs::read(&capture).map_err(|e|e.to_string())?;
            std::fs::write(output.join(format!("{name}-opaque-first.ppm")),&after).map_err(|e|e.to_string())?;
            if before!=after{return Err(format!("{name}: sky draw reordering changed captured pixels"));}
        }
        session.set_batch(&triangles)?;
        if park_check {
            let park=venue.park.as_ref().unwrap();
            let bodies=crate::water::bodies::State::parse(&json!({"ocean":false,"generated":false,"rivers":[],"sources":[],"pools":park.pools.iter().map(|(min,max)|json!({"min":add(*min,venue_origin),"max":add(*max,venue_origin)})).collect::<Vec<_>>(),"sheets":park.water_sheets(venue_origin)}))?;
            let (frame,mesh)=crate::water::surface_mesh::free_surfaces(add(eye,venue_origin),forward,&bodies,0.,&Default::default(),&Default::default());
            let mut packet=crate::water::runtime::mesh_packet(frame,&mesh);
            let o=crate::water::optics::Settings::default();
            packet["optics"]=json!({"time":2.,"murkiness":0.,"sky_color_intensity":o.sky_color_intensity,"neutral_reflection_intensity":o.neutral_reflection_intensity,"sky_top":o.sky_top,"sky_bottom":o.sky_bottom,"light":o.light,"planar_reflection":false,"micro_phase":[0.,0.,0.],"mean_height":0.});
            packet["ripples"]=json!([]);
            session.set_water(root,&json!([packet]),&view,&triangles)?;
        }
        session.capture_next();let report=session.pump(true,true)?;
        std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
        reports.push(json!({"name":name,"frame":report,"stage_lighting":view["stage_lighting"],"venue_triangles":venue.vertices.len()/3}));
    }
    session.finish()?;
    std::fs::write(output.join("review.json"),serde_json::to_vec_pretty(&reports).unwrap()).map_err(|e|e.to_string())?;
    Ok(())
}
