//! Offline silhouettes of production tree meshes. A cached camera-facing plane per site
//! retain the actual crown outline instead of inventing a conical replacement.
use super::{Tree, math::*, profiles::Profile};
pub const WIDTH:usize=192;
pub const HEIGHT:usize=256;
pub const VIEWS:usize=16;
pub const ATLAS_WIDTH:usize=WIDTH*VIEWS;
pub const ATLAS_HEIGHT:usize=HEIGHT*7*2;
pub const BYTES:&[u8]=include_bytes!("../Art/Trees/LOD/impostors.rgba");
#[derive(serde::Deserialize)]
struct Bounds { width:f64, bottom:f64, top:f64 }
fn atlas_profile(profile:usize)->usize { if profile==super::profiles::JUNGLE {2} else {profile} }
fn bounds(profile:usize)->&'static Bounds {
    static BOUNDS:std::sync::OnceLock<Vec<Bounds>>=std::sync::OnceLock::new();
    &BOUNDS.get_or_init(||serde_json::from_str(include_str!("../Art/Trees/LOD/bounds.json")).expect("authored LOD bounds"))[atlas_profile(profile)-2]
}
pub fn supported(tree:&Tree)->bool { !cfg!(target_os="ios") && ((2..9).contains(&tree.site.profile) || tree.site.profile==super::profiles::JUNGLE) && tree.frame.is_none() }
/// Local camera-facing quad with the same root, growth scale and yaw as Tree::generate.
pub fn quad(tree:&Tree, eye:V, profile:Profile, cover:f64)->([super::geometry::Vertex;4],f32) {
    let b=bounds(tree.site.profile);
    let toward=unit(sub(eye,tree.site.origin));
    let local=[dot(toward,tree.world_direction([1.,0.,0.])),dot(toward,tree.site.normal),dot(toward,tree.world_direction([0.,0.,1.]))];
    let yaw=random(tree.site.id,18)*std::f64::consts::TAU;
    let source_yaw=random(77,18)*std::f64::consts::TAU;
    let angle=local[0].atan2(local[2])-yaw+source_yaw;
    let view=(angle.rem_euclid(std::f64::consts::TAU)*VIEWS as f64/std::f64::consts::TAU).round() as usize % VIEWS;
    let right=unit(cross(tree.site.normal,toward));
    let right=if length(right)<0.1 {tree.world_direction([1.,0.,0.])} else {right};
    let normal=unit(cross(right,tree.site.normal));
    let points=[[-0.5,0.], [0.5,0.], [0.5,1.],[-0.5,1.]];
    let vertices=points.map(|p|super::geometry::Vertex{
        position:add(tree.site.origin,add(mul(right,p[0]*b.width*tree.height),mul(tree.site.normal,(b.bottom+(b.top-b.bottom)*p[1])*tree.height))),
        normal,uv:[(0.5-p[0])as f32,p[1]as f32],color:profile.leaf_color,material:6,
        transmission:cover as f32,roughness:profile.roughness,occlusion:0.70,branch_id:0,
    });
    (vertices,((atlas_profile(tree.site.profile)-2)*VIEWS+view) as f32)
}
#[cfg(test)]
#[path="ImpostorBake.rs"]
mod bake;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vegetation::{Site,initial_state,profiles::PROFILES};
    #[test]
    fn baked_views_have_real_wood_foliage_and_growth_aligned_root_bounds() {
        assert_eq!(BYTES.len(),8+ATLAS_WIDTH*ATLAS_HEIGHT*4);
        assert_eq!(u32::from_le_bytes(BYTES[..4].try_into().unwrap())as usize,ATLAS_WIDTH);
        for species in 0..7 {for view in 0..VIEWS {for layer in 0..2 {
            let mut covered=0;
            for y in 0..HEIGHT {for x in 0..WIDTH {let i=8+(((layer*7+species)*HEIGHT+y)*ATLAS_WIDTH+view*WIDTH+x)*4;if BYTES[i+3]>0{covered+=1;}}}
            assert!(covered>30,"empty baked layer {species}/{view}/{layer}");
        }}}
        for profile in 2..9 {
            let site=Site{id:77,origin:[0.,100.,0.],normal:[0.,1.,0.],profile,moisture:0.5,soil_depth:0.6};
            let tree=Tree::generate(site,&initial_state(site),PROFILES[profile]);
            for eye in [[0.,110.,30.],[30.,110.,0.],[0.,110.,-30.],[-30.,110.,0.]] {
                let (q,tile)=quad(&tree,eye,PROFILES[profile],0.25);
                assert_eq!(q.len(),4);assert!(q.iter().all(|v|v.material==6&&v.transmission==0.25));
                assert!((0. ..112.).contains(&tile));
                let b=bounds(profile);
                assert!((q[0].position[1]-100.-b.bottom*tree.height).abs()<1e-8);
                assert!((q[2].position[1]-100.-b.top*tree.height).abs()<1e-8);
                assert!(dot(q[0].normal,sub(eye,site.origin))>0.);
            }
        }
    }
    #[test]
    fn baked_wood_projects_onto_the_same_branches_without_mirroring() {
        for profile in 2..9 {
            let site=Site{id:77,origin:[0.,100.,0.],normal:[0.,1.,0.],profile,moisture:0.5,soil_depth:0.6};
            let tree=Tree::generate(site,&initial_state(site),PROFILES[profile]);
            for eye in [[0.,110.,40.],[40.,110.,0.],[0.,110.,-40.],[-40.,110.,0.]] {
                let (q,tile)=quad(&tree,eye,PROFILES[profile],0.);
                let right=unit(sub(q[1].position,q[0].position));let width=length(sub(q[1].position,q[0].position));let b=bounds(profile);
                for branch in tree.branches.iter().filter(|b|b.order<=1) {
                    let world=tree.world(mul(add(branch.start,branch.end),0.5));
                    let uv=[0.5-dot(sub(world,site.origin),right)/width,(world[1]-site.origin[1]-b.bottom*tree.height)/((b.top-b.bottom)*tree.height)];
                    let x=(uv[0]*WIDTH as f64)as i32;let y=(uv[1]*HEIGHT as f64)as i32;let index=tile as usize;
                    let found=(-2..=2).any(|dy|(-2..=2).any(|dx|{
                        let x=(x+dx).clamp(0,WIDTH as i32-1)as usize;let y=(y+dy).clamp(0,HEIGHT as i32-1)as usize;
                        let i=8+(((index/VIEWS)*HEIGHT+y)*ATLAS_WIDTH+(index%VIEWS)*WIDTH+x)*4;
                        BYTES[i+3]>0
                    }));
                    assert!(found,"misaligned branch {} for species {profile}, angle {tile}",branch.id);
                }
            }
        }
    }

}
