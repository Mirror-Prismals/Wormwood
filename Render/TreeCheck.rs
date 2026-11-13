//! Native review of the actual species meshes/materials, separate from authoring renders.
use super::Session;
use serde_json::json;
use std::path::Path;
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String>{
    use crate::vegetation::{Site,Tree,initial_state,profiles::PROFILES,geometry::{self,Detail},packet::{Packet,Scene}};
    std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
    let entity=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
    let mut session=Session::prepare(root,output,backend,&entity)?;session.environment("planet_surface");
    session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;session.set_batch(&[])?;
    let mut reports=vec![];
    for (season,day) in [("summer",190),("winter",15)] {
        for (detail_name,detail,distance) in [("near",Detail::Near,30.),("middle",Detail::Middle,38.),("far",Detail::Far,50.),("lod",Detail::Far,30.)] {
            for index in 2..PROFILES.len() {
                let p=PROFILES[index];let site=Site{id:77,normal:[0.,1.,0.],origin:[0.;3],profile:index,moisture:0.5,soil_depth:0.6};
                let tree=Tree::generate(site,&initial_state(site),p);
                let eye=[0.,tree.height*0.45,distance];
                let view=json!({"position":eye,"yaw":0.,"pitch":0.,"aspect":1.5,"day_night":{"fraction":0.5},"calendar":{"day_of_year":day,"days_in_year":365}});
                let f=crate::vegetation::season::Foliage::for_profile(&view,77,p);let mut colored=p;colored.leaf_color=f.color;
                let (mesh,impostor_tile)=if detail_name=="lod" {
                    let (quad,tile)=crate::vegetation::impostor::quad(&tree,eye,colored,f.cover);
                    (geometry::Mesh{vertices:quad.into_iter().collect(),indices:vec![0,1,2,0,2,3]},Some(tile))
                } else {(geometry::build_seasonal(&tree,colored,detail,0.,f.cover),None)};
                let vertices=mesh.vertices.iter().map(|v|[
                    (v.position[0]-eye[0])as f32,(v.position[1]-eye[1])as f32,(v.position[2]-eye[2])as f32,
                    v.normal[0]as f32,v.normal[1]as f32,v.normal[2]as f32,v.uv[0],v.uv[1],v.color[0],v.color[1],v.color[2],v.material as f32,v.transmission,v.roughness,v.occlusion,impostor_tile.unwrap_or(p.bark_tile()*1000.+p.bark_ridge_scale as f32)]).collect();
                let packet=Packet{origin:eye,revision:0,vertices,indices:mesh.indices.clone(),scene:Scene::from_view(&view)?};
                packet.validate()?;
                session.set_vegetation(root,Some(packet))?;session.capture_next();let frame=session.pump(true,true)?;
                let name=format!("{}-{season}-{detail_name}",p.name);
                std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
                reports.push(json!({"name":name,"triangles":mesh.triangles(),"frame":frame}));
            }
        }
    }
    // Exercise the real scene selection and total budget, including the fallback
    // that previously inserted a solid cone beside detailed nearby trees.
    {
        use crate::vegetation::math::*;
        let state=crate::vegetation::State::default();
        let normal=unit([4125.,crate::terrain::RADIUS,-125.]);
        let position=mul(normal,crate::terrain::RADIUS+crate::terrain::height(normal)+20.);
        let site=crate::vegetation::sites_near(position,400.,4096).into_iter().find(|s|s.profile==5).ok_or("hemlock review site unavailable")?;
        let (_,north)=basis(site.normal);
        for distance in [24.,48.,80.] {
            let eye=add(site.origin,add(mul(north,distance),mul(site.normal,3.)));
            let target=add(site.origin,mul(site.normal,10.));let forward=unit(sub(target,eye));let right=unit(cross(forward,site.normal));let up=unit(cross(right,forward));
            let view=json!({"position":eye,"yaw":0.,"pitch":0.,"right":right,"up":up,"forward":forward,"aspect":1.5,"time":0.,"max_distance":6000.,"max_triangles":16384,"day_night":{"fraction":0.5},"calendar":{"day_of_year":312,"days_in_year":365,"year":2026}});
            let geometry=crate::base_system::layer_two::vegetation_system::geometry_numeric(&state,&view)?;
            let metadata=geometry.metadata.clone();let triangles=geometry.indices.len()/3;
            if !geometry.vertices.iter().any(|v|v[11]==6.)||geometry.vertices.iter().any(|v|v[11]==5.){return Err("scene LOD did not replace legacy cones".into());}
            let packet=Packet::from_numeric(geometry,Scene::from_view(&view)?)?;
            session.set_vegetation(root,Some(packet))?;session.capture_next();let frame=session.pump(true,true)?;
            let name=format!("forest-{distance:.0}m");
            std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
            reports.push(json!({"name":name,"triangles":triangles,"site_id":site.id,"selection":metadata,"frame":frame}));
        }
    }
    // Review actual production colony placement and meshes at walking height.
    {
        use crate::vegetation::math::*;
        let state=crate::vegetation::State::default();
        let n=unit([4125.,crate::terrain::RADIUS,-125.]);let base=mul(n,crate::terrain::RADIUS+crate::terrain::height(n)+1.7);
        let mut colonies=crate::vegetation::understory::Understory::default();
        for (species_index,species) in ["christmas_fern","mountain_laurel","mayapple","witch_hazel","wood_sorrel","pennsylvania_sedge","sandstone","shale","streamstone"].iter().enumerate() {
            let mut root_position=None;
            for x in -2..=2 { for z in -2..=2 {
                let search=add(base,[x as f64*50.,0.,z as f64*50.]);
                if root_position.is_none() {root_position=colonies.review_root(search,species_index);}
            }}
            let root_position=root_position.ok_or_else(||format!("no {species} colony in native review"))?;
            let normal=unit(root_position);let (_,north)=basis(normal);
            let eye=add(root_position,add(mul(north,3.),mul(normal,1.5)));
            let forward=unit(sub(add(root_position,mul(normal,0.4)),eye));let right=unit(cross(forward,normal));let up=unit(cross(right,forward));
            let view=json!({"position":eye,"right":right,"up":up,"forward":forward,"yaw":0.,"pitch":0.,"aspect":1.5,"time":0.,"max_distance":6000.,"max_triangles":16384,"day_night":{"fraction":0.5},"calendar":{"day_of_year":180}});
            let geometry=crate::base_system::layer_two::vegetation_system::geometry_numeric(&state,&view)?;
            let metadata=geometry.metadata.clone();let triangles=geometry.indices.len()/3;
            let packet=Packet::from_numeric(geometry,Scene::from_view(&view)?)?;
            session.set_vegetation(root,Some(packet))?;session.capture_next();let frame=session.pump(true,true)?;
            let name=format!("understory-{species}");
            std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
            reports.push(json!({"name":name,"triangles":triangles,"selection":metadata,"frame":frame}));
        }
    }
    session.finish()?;std::fs::write(output.join("report.json"),serde_json::to_vec_pretty(&reports).unwrap()).map_err(|e|e.to_string())?;Ok(())
}
