//! Blender-authored normalized branch graphs and leaf outlines, loaded off the audio thread.
use super::{branch_graph::Branch, profiles::Profile};
#[derive(serde::Deserialize)]
pub struct Asset { pub name:String, pub branches:Vec<Branch>, pub outline:Vec<[f64;2]> }
pub fn asset(profile: Profile) -> Option<&'static Asset> {
    static ASSETS:std::sync::OnceLock<Vec<Asset>>=std::sync::OnceLock::new();
    ASSETS.get_or_init(||serde_json::from_str(include_str!("../Art/Trees/species.json")).expect("validated authored tree assets"))
        .iter().find(|a|a.name==profile.name)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vegetation::{Site,Tree,initial_state,geometry::{self,Detail},profiles::{PROFILES,select},season::Foliage};
    #[test]
    fn pa_species_structure_damage_and_distinct_shapes() {
        let mut extents=Vec::new();
        for index in 2..PROFILES.len() {
            let p=PROFILES[index];let a=asset(p).unwrap();assert!(a.outline.len()>=6);
            for (i,b) in a.branches.iter().enumerate() {assert_eq!(b.id,i as u32);assert!(b.parent.is_none_or(|p|p<b.id));assert!(b.radius>0.);}
            let site=Site{id:77,normal:[0.,1.,0.],origin:[0.;3],profile:index,moisture:0.5,soil_depth:0.6};
            let tree=Tree::generate(site,&initial_state(site),p);
            let near=geometry::build(&tree,p,Detail::Near,0.);let middle=geometry::build(&tree,p,Detail::Middle,0.);
            assert!(near.triangles()>middle.triangles());assert!(near.vertices.iter().all(|v|v.position.iter().all(|x|x.is_finite())));
            extents.push(tree.branches.iter().map(|b|b.end).collect::<Vec<_>>());
            let mut damaged=initial_state(site);damaged.broken_branches.insert(4);
            let cut=Tree::generate(site,&damaged,p);assert!(cut.branches.iter().all(|b|b.id!=4&&b.parent!=Some(4)));
        }
        assert!(extents.windows(2).all(|w|w[0].len()!=w[1].len() || w[0].iter().zip(&w[1]).any(|(a,b)|a.iter().zip(b).any(|(x,y)|(x-y).abs()>0.1))));
    }
    #[test]
    fn pa_species_winter_and_habitat_mix() {
        let winter=serde_json::json!({"calendar":{"day_of_year":15,"days_in_year":365}});
        for p in PROFILES.iter().skip(2) {let f=Foliage::for_profile(&winter,77,*p);assert_eq!(f.cover,if p.evergreen(){1.}else{0.});if p.evergreen(){assert_eq!(f.color,p.leaf_color);assert_eq!(f.fallen,0.);}}
        let mut counts=[0;9];for seed in 0..10000 {let i=select(seed,0.5,0.6);assert!((2..PROFILES.len()).contains(&i));counts[i]+=1;}
        assert!(counts[2..].iter().all(|n|*n>300));assert!((0..10000).filter(|s|select(*s,0.9,0.6)==5).count()>2000);
    }
}
