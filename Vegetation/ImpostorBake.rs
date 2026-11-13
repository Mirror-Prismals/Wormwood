//! CPU offline rasterization of the actual detailed tree mesh, including the
//! production cutout equations. Stores surface normals and separate leaf/wood
//! coverage so winter still reveals branches behind summer foliage.
use super::*;
use crate::vegetation::{Site,PlantState,geometry::{self,Detail},profiles::PROFILES};
use std::collections::BTreeSet;
fn mask(uv:[f64;2],kind:u32)->f64 {
    if kind<3 {return geometry::coverage(uv,kind==2);}
    let p=[uv[0]-0.5,uv[1]-0.5];
    let envelope=1.-(p[0]/0.5).powi(2)-(p[1]/0.47).powi(2);
    let cell=[(uv[0]*48.).floor(),(uv[1]*40.).floor()];
    let fract=|x:f64|x-x.floor();let seed=fract((cell[0]*127.1+cell[1]*311.7).sin()*43758.5453);
    let q=[fract(uv[0]*48.)-0.5-(seed-0.5)*0.5,fract(uv[1]*40.)-0.5-(fract(seed*17.)-0.5)*0.5];
    let mut needle:f64=-1.;
    for i in 0..5 {let a=seed*std::f64::consts::TAU+i as f64*0.52;let x=q[0]*a.cos()-q[1]*a.sin();let y=q[0]*a.sin()+q[1]*a.cos();needle=needle.max(1.-(x/0.47).powi(2)-(y/if kind==3 {0.045}else{0.085}).powi(2));}
    envelope.min(needle)
}
#[test]
#[ignore="offline asset authoring; writes Art/Trees/LOD assets"]
fn bake_tree_lod_from_production_meshes() {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Art/Trees/LOD");
    let mut atlas=vec![0u8;8+ATLAS_WIDTH*ATLAS_HEIGHT*4];atlas[..4].copy_from_slice(&(ATLAS_WIDTH as u32).to_le_bytes());atlas[4..8].copy_from_slice(&(ATLAS_HEIGHT as u32).to_le_bytes());
    let mut bounds=Vec::new();
    for profile in 2..9 {
        let site=Site{id:77,normal:[0.,1.,0.],origin:[0.;3],profile,moisture:0.5,soil_depth:0.6};
        let plant=PlantState{age_years:60.,alive:true,broken_branches:BTreeSet::new(),vacant_years:0.,generation:0};
        let tree=Tree::generate(site,&plant,PROFILES[profile]);let mut mesh=geometry::build(&tree,PROFILES[profile],Detail::Near,0.);
        // Match the rest meshes cached by the actual live adapter.
        for v in &mut mesh.vertices {v.position=sub(v.position,sub(tree.wind(v.position,0.),v.position));}
        let radius=mesh.vertices.iter().map(|v|(v.position[0].powi(2)+v.position[2].powi(2)).sqrt()/tree.height).fold(0.,f64::max)*1.02;
        let bottom=mesh.vertices.iter().map(|v|v.position[1]/tree.height).fold(f64::INFINITY,f64::min)-0.005;
        let top=mesh.vertices.iter().map(|v|v.position[1]/tree.height).fold(f64::NEG_INFINITY,f64::max)+0.005;
        bounds.push(serde_json::json!({"width":radius*2.,"bottom":bottom,"top":top}));
        for view in 0..VIEWS {
            let angle=view as f64*std::f64::consts::TAU/VIEWS as f64;
            let forward=[angle.sin(),0.,angle.cos()];let right=[angle.cos(),0.,-angle.sin()];
            let mut depth=vec![f64::NEG_INFINITY;WIDTH*HEIGHT*2];
            for triangle in mesh.indices.chunks_exact(3) {
                let v=[mesh.vertices[triangle[0] as usize],mesh.vertices[triangle[1] as usize],mesh.vertices[triangle[2] as usize]];
                let p=v.map(|v|[(dot(v.position,right)/tree.height/(radius*2.)+0.5)*WIDTH as f64,(v.position[1]/tree.height-bottom)/(top-bottom)*HEIGHT as f64,dot(v.position,forward)]);
                let denom=(p[1][1]-p[2][1])*(p[0][0]-p[2][0])+(p[2][0]-p[1][0])*(p[0][1]-p[2][1]);if denom.abs()<1e-8{continue;}
                let minx=p.iter().map(|p|p[0].floor()as i32).min().unwrap().max(0)as usize;let maxx=p.iter().map(|p|p[0].ceil()as i32).max().unwrap().clamp(0,WIDTH as i32-1)as usize;
                let miny=p.iter().map(|p|p[1].floor()as i32).min().unwrap().max(0)as usize;let maxy=p.iter().map(|p|p[1].ceil()as i32).max().unwrap().clamp(0,HEIGHT as i32-1)as usize;
                let layer=usize::from(v[0].material>0);
                for y in miny..=maxy {for x in minx..=maxx {
                    let a=((p[1][1]-p[2][1])*(x as f64+0.5-p[2][0])+(p[2][0]-p[1][0])*(y as f64+0.5-p[2][1]))/denom;
                    let b=((p[2][1]-p[0][1])*(x as f64+0.5-p[2][0])+(p[0][0]-p[2][0])*(y as f64+0.5-p[2][1]))/denom;let c=1.-a-b;
                    if a<0. || b<0. || c<0. {continue;}
                    let uv=std::array::from_fn(|j|a*v[0].uv[j]as f64+b*v[1].uv[j]as f64+c*v[2].uv[j]as f64);
                    if layer==1&&mask(uv,v[0].material)<0.{continue;}
                    let z=a*p[0][2]+b*p[1][2]+c*p[2][2];let index=layer*WIDTH*HEIGHT+y*WIDTH+x;if z<=depth[index]{continue;}depth[index]=z;
                    let mut n=unit(add(add(mul(v[0].normal,a),mul(v[1].normal,b)),mul(v[2].normal,c)));if layer==1&&dot(n,forward)<0.{n=mul(n,-1.);}
                    let normal=[dot(n,right),n[1],dot(n,forward)];let rgba=[((normal[0]*0.5+0.5)*255.)as u8,((normal[1]*0.5+0.5)*255.)as u8,((normal[2]*0.5+0.5)*255.)as u8,255];
                    let offset=8+(((layer*7+profile-2)*HEIGHT+y)*ATLAS_WIDTH+view*WIDTH+x)*4;atlas[offset..offset+4].copy_from_slice(&rgba);
                }}
            }
        }
    }
    std::fs::write(root.join("impostors.rgba"),atlas).unwrap();std::fs::write(root.join("bounds.json"),serde_json::to_vec_pretty(&bounds).unwrap()).unwrap();
}
