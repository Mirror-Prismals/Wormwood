//! Authored east/west geography. One unit is one metre; +X is east and -Z is north.
use super::{Point, RADIUS};
pub(crate) const TOWN_X: f64 = 125.;
pub(crate) const TOWN_Z: f64 = -125.;
pub(crate) const FOREST_START: f64 = 1_000.;
pub(crate) const DESERT_START: f64 = 21_000.;
pub(crate) const COAST: f64 = 28_000.;
pub(crate) const RIDGE: f64 = -8_000.;
pub(crate) const WEST_CITY: f64 = -11_000.;
pub(crate) const EAST_CITY: f64 = 24_500.;
// Northern distances are measured from town, along -Z.
pub(crate) const NORTH_FOREST_START: f64 = 1_000.;
pub(crate) const NORTH_SEA_START: f64 = 6_000.;
pub(crate) const NORTH_SEA_END: f64 = 11_000.;
pub(crate) const POLAR_START: f64 = 41_000.;
pub(crate) const NORTH_WEST: f64 = -9_000.;
pub(crate) const NORTH_EAST: f64 = 21_000.;
pub(crate) const ICE_CITY_NORTH: f64 = 43_000.;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    Plains,
    Forest,
    Desert,
    Coast,
    Ocean,
    Polar,
    Snow,
    Jungle,
}
pub(crate) fn coordinates(n: Point) -> (f64, f64) {
    let n = super::unit(n);
    (n[0] * RADIUS - TOWN_X, n[2] * RADIUS - TOWN_Z)
}
pub(crate) fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
pub(crate) fn shoreline(z: f64) -> f64 {
    COAST + 180. * (z / 1_600.).sin()
}
/// Signed distance to the nearest authored sea shore (positive into water),
/// and the inward direction in the existing X/Z chart.
pub(crate) fn sea_shore(n: Point) -> (f64, [f64; 2]) {
    if southern(n).is_some() {
        let (x,z)=southern_coordinates(n);
        let d=ocean_distance(n);
        let dx=ocean_distance(southern_normal(x+1.,z))-ocean_distance(southern_normal(x-1.,z));
        let dz=ocean_distance(southern_normal(x,z+1.))-ocean_distance(southern_normal(x,z-1.));
        let length=dx.hypot(dz).max(1e-9);
        return (d,[dx/length,dz/length]);
    }
    let (x, z) = coordinates(n);
    let north = -z;
    let rectangle = [
        (x - NORTH_WEST, [1., 0.]),
        (NORTH_EAST - x, [-1., 0.]),
        (north - NORTH_SEA_START, [0., -1.]),
        (NORTH_SEA_END - north, [0., 1.]),
    ].into_iter().min_by(|a,b| a.0.total_cmp(&b.0)).unwrap();
    let east = (x - shoreline(z), [1., 0.]);
    let east = if POLAR_START - north < east.0 {
        (POLAR_START - north, [0., 1.])
    } else { east };
    if rectangle.0 > east.0 { rectangle } else { east }
}
pub(crate) fn polar(n: Point) -> bool {
    !super::nightworld::active(n) && -coordinates(n).1 >= POLAR_START
}
pub(crate) fn region(n: Point) -> Region {
    if let Some((r, _)) = southern(n) { return r; }
    let (x, z) = coordinates(n);
    let north = -z;
    if north >= POLAR_START { return Region::Polar; }
    if (NORTH_WEST..=NORTH_EAST).contains(&x) {
        if (NORTH_SEA_START..NORTH_SEA_END).contains(&north) { return Region::Ocean; }
        if (NORTH_SEA_END..POLAR_START).contains(&north) { return Region::Forest; }
        // The western edge sweeps northwest from the eastern forest edge.
        if (NORTH_FOREST_START..NORTH_SEA_START).contains(&north)
            && x >= FOREST_START - (north - NORTH_FOREST_START) {
            return Region::Forest;
        }
    }
    if x < FOREST_START {
        Region::Plains
    } else if x < DESERT_START {
        Region::Forest
    } else if x < shoreline(z) - 350. {
        Region::Desert
    } else if x < shoreline(z) + 160. {
        Region::Coast
    } else {
        Region::Ocean
    }
}
pub(crate) fn forest(n: Point) -> bool {
    if region(n)==Region::Jungle {let (x,s)=southern_coordinates(n);if x.hypot(s-JUNGLE_CITY_SOUTH)<650. {return false;}}
    !super::nightworld::active(n) && matches!(region(n), Region::Forest | Region::Jungle)
}
pub(crate) fn sandy(n: Point) -> bool {
    !super::nightworld::active(n)
        && matches!(region(n), Region::Desert | Region::Coast | Region::Ocean)
}
pub(crate) fn ocean(n: Point) -> bool {
    !super::nightworld::active(n) && if let Some((r, _)) = southern(n) { matches!(r, Region::Ocean | Region::Coast) } else { sea_shore(n).0 > -350. }
}
// The southern chart uses surface arc lengths, avoiding mirrored continents on
// the back hemisphere and chord distortion across the four-million-metre island.
pub(crate) const SOUTH_COAST: f64 = 16_000.;
pub(crate) const POLAR_NORTH: f64 = 56_000.;
pub(crate) const POLAR_SOUTH: f64 = 156_000.;
pub(crate) const JUNGLE_NORTH: f64 = 256_000.;
pub(crate) const JUNGLE_SIZE: f64 = 4_000_000.;
pub(crate) const JUNGLE_CITY_SOUTH: f64 = JUNGLE_NORTH + 1_000.;
pub(crate) fn southern_coordinates(n: Point) -> (f64, f64) {
    let n = super::unit(n);
    (n[0].atan2(n[1]) * RADIUS - TOWN_X,
     n[2].atan2(n[0].hypot(n[1])) * RADIUS - TOWN_Z)
}
pub(crate) fn southern_normal(east: f64, south: f64) -> Point {
    let longitude = (east + TOWN_X) / RADIUS;
    let latitude = (south + TOWN_Z) / RADIUS;
    [latitude.cos()*longitude.sin(), latitude.cos()*longitude.cos(), latitude.sin()]
}
// Positive inside; straight broad shores with small deterministic coves. Rounded
// corners keep each landmass an island rather than a planet-spanning strip.
fn island_distance(x: f64, z: f64, half_x: f64, half_z: f64, radius: f64) -> f64 {
    let a=x.abs()-(half_x-radius); let b=z.abs()-(half_z-radius);
    radius - a.max(0.).hypot(b.max(0.)) - a.max(b).min(0.)
}
pub(crate) fn southern(n: Point) -> Option<(Region, f64)> {
    let (x, south) = southern_coordinates(n);
    let shore = SOUTH_COAST + 100.*(x/2_300.).sin();
    if south < shore - 500. { return None; }
    if south < shore + 200. {
        let h=3.*(1.-smooth((south-shore+500.)/700.))-24.*smooth((south-shore)/200.);
        return Some((Region::Coast,h));
    }
    let polar=island_distance(x,south-106_000.,50_000.,50_000.,8_000.);
    if polar > -300. {
        let h=-24.+(32.+130.*(x/17_000.).cos().powi(2)*(south/23_000.).sin().powi(2))*smooth((polar+300.)/1_000.);
        return Some((if polar>400. {Region::Snow} else {Region::Coast},h));
    }
    let jungle=island_distance(x,south-(JUNGLE_NORTH+JUNGLE_SIZE/2.),JUNGLE_SIZE/2.,JUNGLE_SIZE/2.,80_000.);
    if jungle > -300. {
        let relief=35.+75.*(x/14_000.).sin().powi(2)+100.*(south/19_000.).sin().powi(2);
        let pad=1.-smooth((x.hypot(south-JUNGLE_CITY_SOUTH)-650.)/250.);
        let land=relief*(1.-pad)+22.*pad;
        let h=-24.+(land+24.)*smooth((jungle+300.)/900.);
        return Some((if jungle>700. {Region::Jungle} else {Region::Coast},h));
    }
    Some((Region::Ocean,-24.))
}
/// Approximate signed distance to the nearest authored shore for waves/caching.
pub(crate) fn ocean_distance(n: Point) -> f64 {
    if southern(n).is_none() { return sea_shore(n).0; }
    let (x,s)=southern_coordinates(n);
    let polar=island_distance(x,s-106_000.,50_000.,50_000.,8_000.);
    let jungle=island_distance(x,s-(JUNGLE_NORTH+JUNGLE_SIZE/2.),JUNGLE_SIZE/2.,JUNGLE_SIZE/2.,80_000.);
    (s-SOUTH_COAST-100.*(x/2_300.).sin()).min(-polar).min(-jungle)
}
fn plains(x: f64, z: f64) -> f64 {
    let wx = x + TOWN_X;
    let wz = z + TOWN_Z;
    2.5 * (x / 1_100.).sin() * (z / 1_400.).cos()
        + 0.5 * (z / 270.).sin()
        + 0.09 * (wx / 1.3).sin() * (wz / 1.7).sin()
        + 0.035 * (wx / 0.41 + wz / 0.53).sin()
}
/// Regional height is shared by collision, drainage, vegetation and rendering.
pub(crate) fn height(n: Point, old: f64) -> f64 {
    if let Some((_, h)) = southern(n) { return h; }
    let (x, z) = coordinates(n);
    let base = east_west_height(x, z, old);
    let north = -z;
    let land = 28. + 4. * (x / 1_500.).sin() * (north / 1_800.).cos();
    if north >= POLAR_START {
        let city = 1. - smooth((x.hypot(north - ICE_CITY_NORTH) - 650.) / 300.);
        let snow = land * (1. - city) + 28. * city;
        let edge = (x - NORTH_WEST).min(NORTH_EAST - x);
        let previous = base + (land - base) * smooth(edge / 200.);
        return previous + (snow - previous) * smooth((north - POLAR_START) / 300.);
    }
    let side = (x - NORTH_WEST).min(NORTH_EAST - x);
    if side >= 0. && north >= NORTH_SEA_START - 250. {
        // Lower only the interior of the rectangle. The original landform
        // meets each bank continuously, including the western mountain ridge.
        if north < NORTH_SEA_END {
            let distance = side.min(north - NORTH_SEA_START).min(NORTH_SEA_END - north);
            let bank = base + (land - base) * smooth(side / 200.)
                * smooth((north - NORTH_SEA_START + 250.) / 250.);
            return bank + (-14. - bank) * smooth(distance / 180.);
        }
        return base + (land - base) * smooth(side / 200.);
    }
    if (NORTH_FOREST_START..NORTH_SEA_START).contains(&north) && x < FOREST_START {
        let edge = FOREST_START - (north - NORTH_FOREST_START);
        let weight = smooth((x - edge) / 200.) * smooth((north - NORTH_FOREST_START) / 200.);
        return base + (old - base) * weight;
    }
    base
}
fn east_west_height(x: f64, z: f64, old: f64) -> f64 {
    let p = plains(x, z) - plains(-TOWN_X, -TOWN_Z);
    let ridge_center = RIDGE + 180. * (z / 4_700.).sin();
    let d = (x - ridge_center) / 760.;
    let ridge = 1_550. * (-d * d).exp() * (0.88 + 0.12 * (z / 1_900.).cos())
        + 210. * (-((x - ridge_center + 750.) / 470.).powi(2)).exp();
    if x < FOREST_START {
        return p + ridge;
    }
    if x < DESERT_START {
        return p + (old - p) * smooth((x - FOREST_START) / 200.);
    }
    let dunes =
        42. + 12. * (x / 680. + 0.6 * (z / 1_100.).sin()).sin() + 6. * (x / 193. + z / 710.).sin();
    let pad = 1. - smooth((((x - EAST_CITY).hypot(z)) - 650.) / 300.);
    let desert = dunes * (1. - pad) + 42. * pad;
    let beach = smooth((0.25 + (z / 1_100.).sin()) / 0.65);
    let shore = shoreline(z);
    let cliff = desert * (1. - smooth((x - shore + 35.) / 70.));
    let strand = desert * (1. - smooth((x - shore + 350.) / 510.));
    let coast = cliff * (1. - beach) + strand * beach;
    let sea = -14. * smooth((x - shore) / 160.);
    let target = coast + sea;
    old + (target - old) * smooth((x - DESERT_START) / 250.)
}
#[cfg(test)]
mod tests {
    use super::*;
    pub fn normal(x: f64, z: f64) -> Point {
        let x = (x + TOWN_X) / RADIUS;
        let z = (z + TOWN_Z) / RADIUS;
        [x, (1. - x * x - z * z).sqrt(), z]
    }
    #[test]
    fn northern_geography_has_connected_forest_sea_and_treeless_pole() {
        for (x,north,expected) in [
            (0.,900.,Region::Plains), (0.,2_500.,Region::Forest),
            (-3_000.,5_500.,Region::Forest), (-5_000.,5_500.,Region::Plains),
            (10_000.,3_000.,Region::Forest), (0.,8_000.,Region::Ocean),
            (0.,11_500.,Region::Forest), (0.,40_999.,Region::Forest),
            (0.,41_000.,Region::Polar), (0.,ICE_CITY_NORTH,Region::Polar),
        ] {
            assert_eq!(region(normal(x,-north)),expected,"{x}/{north}");
        }
        assert_eq!(NORTH_EAST-NORTH_WEST,30_000.);
        assert_eq!(NORTH_SEA_END-NORTH_SEA_START,5_000.);
        assert_eq!(POLAR_START-NORTH_SEA_END,30_000.);
        for x in [-8_500.,0.,20_500.] {
            let n=normal(x,-8_000.);
            assert!(ocean(n));
            assert!(super::super::height(n) < -13.9);
            assert!(!forest(n));
        }
        let pole=normal(0.,-ICE_CITY_NORTH);
        assert!(!forest(pole));
        assert!(!ocean(pole));
        let point=pole.map(|v|v*(RADIUS+super::super::height(pole)-0.1));
        assert_eq!(crate::physics::matter::world_field::substance(point),
            crate::physics::matter::world_field::Substance::Snow);
    }
    #[test]
    fn northern_banks_are_continuous_and_wave_directions_follow_each_shore() {
        for (x,north,dir) in [
            (NORTH_WEST+20.,8_000.,[1.,0.]),
            (NORTH_EAST-20.,8_000.,[-1.,0.]),
            (0.,NORTH_SEA_START+20.,[0.,-1.]),
            (0.,NORTH_SEA_END-20.,[0.,1.]),
        ] {
            assert_eq!(sea_shore(normal(x,-north)).1,dir);
        }
        for (x,north,dx,dn) in [
            (0.,NORTH_SEA_START,0.,0.01),
            (0.,NORTH_SEA_END,0.,0.01),
            (NORTH_WEST,8_000.,0.01,0.),
            (NORTH_EAST,8_000.,0.01,0.),
            (0.,POLAR_START,0.,0.01),
        ] {
            let a=super::super::height(normal(x-dx,-north+dn));
            let b=super::super::height(normal(x+dx,-north-dn));
            assert!((a-b).abs()<0.2,"bank discontinuity {x}/{north}: {a}/{b}");
        }
    }
    #[test]
    fn northern_sea_supplies_physical_water_and_visible_horizon() {
        use crate::water::{bodies,hydrology,ocean,surface_mesh,types};
        let n=normal(0.,-8_000.);
        let focus=n.map(|v|v*(RADIUS-3.5));
        let points=[(-100.,-8_100.),(100.,-8_100.),(100.,-7_900.),(-100.,-7_900.)];
        let vertices=points.map(|(x,z)| {
            let up=normal(x,z);
            let p=up.map(|v|v*(RADIUS+super::super::height(up)));
            std::array::from_fn(|i|(p[i]-focus[i])as f32)
        }).to_vec();
        let mut terrain=hydrology::State::default();
        terrain.update(types::Snapshot{origin:focus,vertices,triangles:vec![[0,2,1],[0,3,2]],revision:1,min:[-200.;3],max:[200.;3]},&Default::default(),0);
        let bodies=bodies::State::parse(&serde_json::json!({"ocean":true,"generated":true,"rivers":[],"sources":[]})).unwrap();
        let wave=ocean::State::default();
        let water=bodies.natural_candidate(focus,-3.5,&wave,&terrain).expect("northern sea must contain physical water");
        assert!(matches!(water.kind,crate::water::flow::Kind::Ocean));
        assert!(water.bed_height < -13.9);
        let (_,mesh)=surface_mesh::free_surfaces(n.map(|v|v*(RADIUS+80.)),[1.,0.,0.],&bodies,-3.5,&wave,&terrain);
        assert!(!mesh.truncated);
        assert!(!mesh.vertices.is_empty());
        assert!(mesh.vertices.len()<196608);
    }
    #[test]
    fn regional_ocean_uses_the_real_ground_and_moves() {
        use crate::water::{bodies, hydrology, ocean, types};
        let point = |x: f64, z: f64, h: f64| {
            let n = normal(x, z);
            n.map(|v| v * (RADIUS + h))
        };
        let center = point(30_000., 0., -3.5);
        let ground = |x: f64, z: f64| {
            let n = normal(x, z);
            point(x, z, super::super::height(n))
        };
        let vertices = [
            ground(29_800., -200.),
            ground(30_200., -200.),
            ground(30_200., 200.),
            ground(29_800., 200.),
        ]
        .map(|p| std::array::from_fn(|i| (p[i] - center[i]) as f32))
        .to_vec();
        let snapshot = types::Snapshot {
            origin: center,
            vertices,
            triangles: vec![[0, 2, 1], [0, 3, 2]],
            revision: 1,
            min: [-500.; 3],
            max: [500.; 3],
        };
        let mut terrain = hydrology::State::default();
        terrain.update(snapshot, &Default::default(), 0);
        let bodies = bodies::State::parse(
            &serde_json::json!({"ocean":true,"generated":true,"rivers":[],"sources":[]}),
        )
        .unwrap();
        let mut wave = ocean::State::default();
        wave.settings.amplitude = 2.2;
        let a = bodies
            .natural_candidate(center, -3.5, &wave, &terrain)
            .expect("eastern ocean must reach real terrain beyond old drainage bounds");
        assert!(a.bed_height < -13.9 && a.bed_height > -14.1);
        assert!(matches!(a.kind, crate::water::flow::Kind::Ocean));
        wave.time = 2.;
        let b = bodies
            .natural_candidate(center, -3.5, &wave, &terrain)
            .unwrap();
        assert!((a.surface_height - b.surface_height).abs() > 0.01);
        terrain.bodies = bodies;
        let _ = terrain.free_surface(center, [1., 0., 0.], -3.5, &wave);
        assert!(
            terrain.surface_cache.is_none(),
            "coastal waves must not be frozen"
        );
        let inland = point(-40_000., 0., 2.);
        let _ = terrain.free_surface(inland, [1., 0., 0.], -3.5, &wave);
        assert!(
            terrain.surface_cache.is_some(),
            "inland water keeps its static cache"
        );
    }
    #[test]
    fn regional_ocean_reaches_the_horizon_without_covering_inland_ground() {
        use crate::water::{bodies, hydrology, ocean, surface_mesh};
        let n = normal(COAST - 100., 0.);
        let focus = n.map(|v| v * (RADIUS + 90.));
        let bodies = bodies::State::parse(
            &serde_json::json!({"ocean":true,"generated":true,"rivers":[],"sources":[]}),
        )
        .unwrap();
        let (_, mesh) = surface_mesh::free_surfaces(
            focus,
            [1., 0., 0.],
            &bodies,
            -3.5,
            &ocean::State::default(),
            &hydrology::State::default(),
        );
        assert!(!mesh.truncated);
        assert!(mesh.vertices.len() < 196608);
        assert!(
            mesh.vertices
                .iter()
                .any(|v| coordinates(v.axis).0 > COAST + 5_000.)
        );
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.position.iter().all(|n| n.is_finite()))
        );
        let n = normal(0., 0.);
        let focus = n.map(|v| v * RADIUS);
        let (_, inland) = surface_mesh::free_surfaces(
            focus,
            [1., 0., 0.],
            &bodies,
            -3.5,
            &ocean::State::default(),
            &hydrology::State::default(),
        );
        assert!(
            inland.vertices.iter().filter(|v| v.class == 1).all(|v| {
                let (x,z)=coordinates(v.axis);
                x.hypot(z)>1_000.
            }),
            "the visible northern sea must not cover town's inland ground"
        );
    }
    #[test]
    fn regional_live_tree_membership_and_sand() {
        for x in [-11_000., -8_000., 0., 900., 21_100., 24_500., 29_000.] {
            let n = normal(x, 0.);
            let p = n.map(|v| v * (RADIUS + super::super::height(n) + 1.7));
            assert!(
                crate::vegetation::sites_near(p, 50., 64).is_empty(),
                "trees outside forest at {x}"
            );
        }
        let n = normal(4_000., 0.);
        let p = n.map(|v| v * (RADIUS + super::super::height(n) + 1.7));
        let sites = crate::vegetation::sites_near(p, 100., 128);
        assert!(!sites.is_empty());
        for s in sites {
            assert!(crate::vegetation::site_by_id(s.id).is_some());
        }
        let n = normal(EAST_CITY, 0.);
        let p = n.map(|v| v * (RADIUS + super::super::height(n) - 0.05));
        assert_eq!(
            crate::physics::matter::world_field::substance(p),
            crate::physics::matter::world_field::Substance::FineSand
        );
    }
    #[test]
    fn regional_boundaries_and_forest_extent() {
        for (x, r) in [
            (0., Region::Plains),
            (999.9, Region::Plains),
            (1_000., Region::Forest),
            (20_999., Region::Forest),
            (21_000., Region::Desert),
            (28_500., Region::Ocean),
            (-11_000., Region::Plains),
        ] {
            assert_eq!(region(normal(x, 0.)), r);
        }
        for z in [-1_000., 0., 1_000.] {
            assert!(forest(normal(10_000., z)));
            assert!(!forest(normal(-5_000., z)));
        }
    }
    #[test]
    fn regional_high_north_south_ridge_and_western_plain() {
        for z in [-1_000., 0., 1_000.] {
            assert!(height(normal(RIDGE, z), 0.) > 1_100.);
            assert!(height(normal(WEST_CITY, z), 0.).abs() < 5.);
        }
        assert!(height(normal(0., 0.), 0.).abs() < 5.);
    }
    #[test]
    fn regional_ocean_bed_and_city_pad() {
        assert!((height(normal(EAST_CITY, 0.), 0.) - 42.).abs() < 1e-6);
        assert_eq!(height(normal(COAST + 1_000., 0.), 0.), -14.);
        assert!(sandy(normal(EAST_CITY, 0.)));
    }
}

#[cfg(test)]
mod southern_tests {
    use super::*;
    #[test]
    fn southern_distances_islands_and_no_antipodal_copy() {
        for (x,z,want) in [(0.,0.,Region::Plains),(0.,30_000.,Region::Ocean),
            (0.,60_000.,Region::Snow),(0.,150_000.,Region::Snow),
            (60_000.,100_000.,Region::Ocean),(0.,200_000.,Region::Ocean),
            (0.,257_000.,Region::Jungle),(0.,2_000_000.,Region::Jungle),
            (2_100_000.,2_000_000.,Region::Ocean),(0.,4_300_000.,Region::Ocean)] {
            let n=southern_normal(x,z);assert_eq!(region(n),want,"{x},{z}");
            let c=southern_coordinates(n);assert!((c.0-x).abs()<1e-6 && (c.1-z).abs()<1e-6);
            let h=super::super::height(n);assert!(h.is_finite());
            if want==Region::Ocean {assert!(h< -3.5 && ocean(n));}else if want==Region::Snow || want==Region::Jungle {assert!(h>0. && !ocean(n));}
        }
        assert_eq!(JUNGLE_NORTH-SOUTH_COAST,240_000.);
        assert_eq!(POLAR_SOUTH-POLAR_NORTH,100_000.);
        assert_ne!(region(southern_normal(0.,2_000_000.).map(|v|-v)),Region::Jungle);
    }
    #[test]
    fn southern_trees_and_snow_use_shared_samples() {
        let snow=southern_normal(0.,106_000.);let jungle=southern_normal(2_000.,260_000.);
        assert!(!forest(snow));assert!(forest(jungle));
        assert!(super::super::sample(snow).color[0]>0.8);
        assert_eq!(crate::physics::matter::world_field::substance(snow.map(|v|v*(RADIUS+super::super::height(snow)-0.1))),crate::physics::matter::world_field::Substance::Snow);
        assert!(super::super::sample(jungle).moisture>0.8);
        let trees=crate::vegetation::sites_near(jungle.map(|v|v*(RADIUS+super::super::height(jungle))),100.,128);
        assert!(!trees.is_empty());
        for tree in trees {
            assert!(crate::vegetation::site_by_id(tree.id).is_some());
            let profile=crate::vegetation::profiles::profile(tree.profile);
            assert_eq!(profile.name,"jungle_broadleaf");
            assert!(crate::vegetation::season::Foliage::for_profile(&serde_json::json!({"calendar":{"day_of_year":15}}),tree.id,profile).cover>0.99);
        }
    }
    #[test]
    fn southern_original_surface_uses_biome_material() {
        let data=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let field=super::super::underground::Field::parse(&data).unwrap().unwrap();
        let sampler=super::super::surface_cover::Sampler::new(&field).unwrap();
        for (x,south) in [(0.,106_000.),(2000.,260_000.),(0.,36_000.)] {
            let n=southern_normal(x,south);
            let p=n.map(|v|v*(RADIUS+super::super::height(n)-0.01));
            let expected=crate::physics::matter::world_field::substance(p).color_at(p);
            assert_eq!(sampler.color(p),Some(expected));
        }
    }
    #[test]
    #[ignore = "Explicit offline export of actual terrain sampler for globe illustration"]
    fn export_southern_globe() {
        use std::io::Write;
        let out=std::path::Path::new("output/southern-world");std::fs::create_dir_all(out).unwrap();
        let color=|n:Point| -> [u8;3] {
            let s=super::super::sample(n);
            if !super::super::nightworld::active(n) && ocean(n) && s.height< -3.5 {return [18,67,108];}
            s.color.map(|v|(v.clamp(0.,1.)*255.) as u8)
        };
        let mut globe=std::io::BufWriter::new(std::fs::File::create(out.join("sampler-globe.ppm")).unwrap());
        write!(globe,"P6\n1600 800\n255\n").unwrap();
        for y in 0..800 {for x in 0..1600 {
            let latitude=(y as f64+0.5)/800.*std::f64::consts::PI-std::f64::consts::FRAC_PI_2;
            let longitude=(x as f64+0.5)/1600.*std::f64::consts::TAU-std::f64::consts::PI;
            let n=[latitude.cos()*longitude.sin(),latitude.cos()*longitude.cos(),latitude.sin()];
            globe.write_all(&color(n)).unwrap();
        }}
        let mut detail=std::io::BufWriter::new(std::fs::File::create(out.join("sampler-detail.ppm")).unwrap());
        write!(detail,"P6\n600 1000\n255\n").unwrap();
        for y in 0..1000 {for x in 0..600 {detail.write_all(&color(southern_normal((x as f64/600.-0.5)*180_000.,y as f64/1000.*300_000.))).unwrap();}}
    }
}
