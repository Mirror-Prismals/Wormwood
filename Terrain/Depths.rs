//! Cardinal's legacy Perlin cave field and unified Depths river profile.
//! Coordinates are the local world chart, in metres relative to sea level.
const PERM_A: [u8; 256] = [201,106,2,234,94,150,140,125,250,36,243,157,42,126,162,66,13,121,236,41,217,33,80,213,91,209,206,167,9,210,238,99,35,27,225,12,235,248,69,38,159,255,21,136,215,116,158,220,52,93,11,173,88,208,15,47,188,7,154,199,148,138,105,23,247,39,239,113,68,178,51,144,10,57,223,179,0,97,77,108,119,30,22,45,165,40,48,233,110,145,122,135,241,31,205,194,127,95,137,78,128,230,191,115,65,151,155,17,187,64,189,133,176,160,141,129,221,184,147,252,171,149,130,75,114,207,96,14,46,63,98,84,86,229,92,28,198,81,8,237,25,90,29,37,20,177,244,195,107,163,192,109,79,89,211,112,6,50,186,202,170,180,212,143,59,245,55,111,44,232,172,54,83,142,153,134,85,219,222,104,19,181,73,53,249,169,231,253,164,18,5,182,204,190,62,152,82,1,58,203,131,183,214,3,43,175,246,168,200,32,60,185,117,242,251,4,132,124,71,100,196,102,70,103,228,118,139,34,74,193,26,156,174,16,216,240,197,76,87,227,166,101,123,120,161,61,226,224,254,218,67,49,146,56,24,72,];
const PERM_B: [u8; 256] = [104,246,177,141,57,179,178,145,185,208,99,37,72,3,111,132,22,166,76,222,82,244,240,49,216,134,128,36,28,194,10,41,40,144,23,182,56,103,92,200,247,73,16,190,95,196,30,172,251,226,1,214,9,220,121,203,131,151,165,243,218,237,154,175,68,161,91,59,60,206,18,7,84,94,142,149,20,169,126,71,106,107,26,215,15,53,43,239,0,167,238,188,110,211,158,83,12,234,197,100,213,96,55,199,69,52,227,11,157,31,153,24,217,233,58,116,14,186,160,66,62,90,125,112,232,225,205,38,64,117,230,17,42,176,191,209,207,245,81,242,67,223,224,202,136,253,63,4,98,254,8,87,198,65,44,183,127,195,184,139,219,19,45,88,250,187,113,109,27,5,171,189,89,140,181,32,119,105,50,162,97,163,61,152,150,138,229,192,221,29,168,143,48,74,115,2,148,133,231,201,156,204,79,86,193,235,70,147,129,51,13,25,93,118,39,248,77,124,135,6,210,33,54,101,85,80,21,146,123,75,155,46,236,174,228,170,255,159,249,173,137,35,47,180,34,212,108,122,78,252,114,164,241,102,120,130,];
fn perlin(p: [f64; 3], perm: &[u8; 256]) -> f64 {
    let i = p.map(|x| x.floor() as i64);
    let f = std::array::from_fn::<_, 3, _>(|n| p[n] - p[n].floor());
    let fade = f.map(|t| t*t*t*(t*(t*6.-15.)+10.));
    let at = |x: i64| perm[x.rem_euclid(256) as usize] as i64;
    let grad = |hash: i64, x: f64, y: f64, z: f64| {
        let h=hash & 15;
        let u=if h<8{x}else{y};
        let v=if h<4{y}else if h==12 || h==14{x}else{z};
        (if h&1==0 {u} else {-u}) + (if h&2==0 {v} else {-v})
    };
    let mut result=0.;
    for x in 0..2 { for y in 0..2 { for z in 0..2 {
        let hash=at(at(at(i[0]+x)+i[1]+y)+i[2]+z);
        let w=(if x==0 {1.-fade[0]} else {fade[0]})*(if y==0 {1.-fade[1]} else {fade[1]})*(if z==0 {1.-fade[2]} else {fade[2]});
        result += w*grad(hash, f[0]-x as f64, f[1]-y as f64, f[2]-z as f64);
    } } }
    result
}
/// Positive is cave air. Continuous Perlin samples replace Cardinal's cached
/// four-metre quantized samples so the same field can be smoothly meshed.
fn noise_pair(p: [f64; 3]) -> (f64, f64) {
    let a=(perlin([p[0]/64.,p[1]/48.,p[2]/64.],&PERM_A)+1.)*0.5;
    let b=(perlin([p[0]/128.,p[1]/128.,p[2]/128.],&PERM_B)+1.)*0.5;
    (a,b)
}
/// Expansive pre-binary upper caves: broad 64/128 m Perlin lobes,
/// tapering below a three-metre surface roof. Continuous, not cached cells.
pub(crate) fn upper_score(p: [f64;3], depth:f64) -> f64 {
    let (a,b)=noise_pair(p);
    let t=(depth/24.).clamp(0.,1.);
    (a-(0.72-0.10*t)).max(b-(0.68-0.10*t))
}
pub(crate) fn cave_score(p: [f64; 3]) -> f64 {
    let (a,b)=noise_pair(p);
    let band=((p[1]+64.)/44.).clamp(0.,1.);
    (a-(0.57+0.07*band)).max(b-(0.53+0.08*band))
}
/// Cardinal scans the bottom cave band for lava seeds, then fills each
/// selected column down to the Depths floor. The configured lava top is -66,
/// but the seed scan reaches six metres above it and the floor is -64.
pub(crate) fn lava_top(x:f64,z:f64)->Option<f64> {
    (-63..=-60).rev().find(|&y| cave_score([x,y as f64,z])>0.).map(|y|y as f64)
}
/// Immediate water/lava contact is converted to obsidian. Cardinal also runs
/// a separate soft-body contact bake, which is not reproduced by this check.
pub(crate) fn lava_touches_river(x:f64,z:f64,top:f64)->bool {
    [(0.,0.),(1.,0.),(-1.,0.),(0.,1.),(0.,-1.)].into_iter().any(|(dx,dz)|
        river(x+dx,z+dz).is_some_and(|r|r.bottom<=top+1. && r.water>=-64.))
}
fn hash2(x: i32,z: i32)->u32 {
    let mut h=(x as u32).wrapping_mul(73856093) ^ (z as u32).wrapping_mul(19349663);
    h^=h>>13; h=h.wrapping_mul(1274126177); h^(h>>16)
}
fn noise(seed:i32,x:f64,z:f64)->f64 {
    let ix=x.floor() as i32; let iz=z.floor() as i32;
    let smooth=|t:f64|t*t*(3.-2.*t);
    let u=smooth(x-x.floor()); let v=smooth(z-z.floor());
    let sample=|a:i32,b:i32| (hash2(a.wrapping_add(seed.wrapping_mul(37)),b.wrapping_sub(seed.wrapping_mul(53))) & 0xffffff) as f64/16777215.;
    let lerp=|a:f64,b:f64,t:f64|a+(b-a)*t;
    lerp(lerp(sample(ix,iz),sample(ix+1,iz),u),lerp(sample(ix,iz+1),sample(ix+1,iz+1),u),v)
}
fn fbm(seed:i32,x:f64,z:f64,octaves:usize)->f64 {
    let (mut sum,mut amp,mut freq,mut norm)=(0.,1.,1.,0.);
    for i in 0..octaves {sum+=noise(seed+i as i32*911,x*freq,z*freq)*amp; norm+=amp; amp*=0.5; freq*=2.;} sum/norm
}
#[derive(Clone,Copy,Debug)]
pub(crate) struct River { pub bottom:f64, pub water:f64, pub ceiling:f64, pub shore:f64 }
pub(crate) const RIVER_WATER_RANGE:[f64;2]=[-61.,-23.];
/// Cardinal's configured Depths river width, bed, waterline and ceiling rules.
pub(crate) fn river(x:f64,z:f64)->Option<River> {river_sample(x,z,false)}
/// Presentation samples include a signed dry bank so triangles meet the shore.
/// Occupancy and cave carving continue to use river(), which excludes dry land.
pub(crate) fn river_boundary(x:f64,z:f64)->Option<River> {river_sample(x,z,true)}
fn river_sample(x:f64,z:f64,include_dry:bool)->Option<River> {
    let seed=9117; let sx=x/128.; let sz=z/128.;
    let primary=fbm(seed,sx,sz,4);
    let ridge=(primary*2.-1.).abs();
    let width=fbm(seed*19+31,sx*0.77+3.1,sz*0.77-2.4,2);
    let threshold=0.152+(0.340-0.152)*width;
    if ridge>=threshold && !include_dry {return None;}
    let inner=(1.-ridge/threshold).clamp(0.,1.);
    let terrain=fbm(seed*41+73,x/220.,z/220.,3);
    let local=(-33. + (24.*(terrain*2.-1.)).round()).clamp(-59.,-23.);
    let dn=fbm(seed*29+17,sx*0.9-7.5,sz*0.9+4.7,2);
    let channel=2.+(4.*dn).round()+10.;
    let base=local-channel;
    if base < -61. {return None;}
    let dbase=3.+(6.*dn).round();
    let depth=(dbase*3.*inner*inner).round().max(2.);
    let floor=base-depth;
    let water=(floor+2.).max(base-3.).clamp(RIVER_WATER_RANGE[0],RIVER_WATER_RANGE[1]);
    let depth=depth.clamp(1.,(water+62.).max(1.));
    // Cardinal uses a per-column 10..15 m ceiling erosion. A smooth
    // interpolation avoids block-height jitter on the continuous surface.
    let ceiling=(water+10.+5.*dn).min(-21.);
    Some(River{bottom:water-depth,water,ceiling,shore:(1.-ridge/threshold)*8.})
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn depth_field_and_rivers_are_deterministic_and_bounded(){
  assert_eq!(cave_score([12.,-45.,19.]),cave_score([12.,-45.,19.]));
  let rivers:Vec<_>=(-400..400).step_by(8).flat_map(|x|(-400..400).step_by(8).filter_map(move |z|river(x as f64,z as f64))).collect();
  assert!(!rivers.is_empty());
  assert!(rivers.iter().all(|r|r.bottom>=-62. && r.water<=-23. && r.ceiling<=-21. && r.bottom<r.water));
 }
 #[test] fn cardinal_ores_and_floor_patches_are_present_and_stable() {
  let mut copper=0; let mut lodestone=0; let mut purple=0;
  for x in -80..80 { for z in -80..80 {
   if purple_floor_patch(x as f64,z as f64) { purple+=1; }
   for y in (-63..=-21).step_by(2) {
    let p=[x as f64,y as f64,z as f64];
    let v=ore(p); assert_eq!(v,ore(p));
    match v {Some(Ore::Copper)=>copper+=1,Some(Ore::Lodestone(n))=>{assert!(n<7);lodestone+=1},None=>{}}
   }
  }}
  assert!(copper>0 && lodestone>0 && purple>0);
  assert_eq!(ore([1.,-19.,1.]),None);
 }
 #[test] fn rust_beams_are_deterministic_and_constrained_to_depths() {
  let mut found=false;
  for x in (0..160).step_by(2) { for z in (0..160).step_by(2) {
   for y in -63..=-54 {
    let p=[x as f64,y as f64,z as f64];
    if rust_beam(p) {assert!(rust_beam(p));found=true;break;}
   }
   if found {break;}
  } if found {break;} }
  assert!(found,"expected a beam near the seeded lava");
  assert!(!rust_beam([0.,-30.,0.]));
 }
}

pub(crate) fn hash3(x:i32,y:i32,z:i32)->u32 {
    let mut h=(x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663) ^ (z as u32).wrapping_mul(83492791);
    h^=h>>13; h=h.wrapping_mul(1274126177); h^(h>>16)
}
/// Cardinal's crystal selection weights: 36, 30, 18, 16.
pub(crate) fn crystal_tile(seed:u32, river_bank:bool)->usize {
    let pick=if river_bank {(seed>>8)%100} else {(seed>>16)%100};
    if pick<36 {24} else if pick<66 {25} else if pick<84 {26} else {27}
}
/// Cardinal's seeded 8-metre wall-crystal ellipses, by wall orientation.
pub(crate) fn wall_crystal_patch(p:[i32;3],facing:usize)->bool {
    let u=if facing<2 {p[2]} else {p[0]};
    let v=p[1]; let plane=if facing<2 {p[0]} else {p[2]};
    let (cu,cv,band)=(u.div_euclid(8),v.div_euclid(8),plane.div_euclid(4));
    for dv in -1..=1 { for du in -1..=1 {
        let (su,sv)=(cu+du,cv+dv); let f=facing as i32+1;
        let seed=hash3(su+1877*f,sv-2791*f,band+3253);
        if seed%100>=24 {continue;}
        let (pu,pv)=(su*8+((seed>>8)%8) as i32,sv*8+((seed>>12)%8) as i32);
        let (ru,rv)=(2+((seed>>17)%3) as i32,2+((seed>>20)%2) as i32);
        if (u-pu).pow(2)*rv*rv+(v-pv).pow(2)*ru*ru<=ru*ru*rv*rv {return true;}
    }} false
}
/// Rusted beams replace solid stone in short horizontal runs near lava.
pub(crate) fn rust_beam(p:[f64;3])->bool {
    let cell=p.map(|v|v.floor() as i32);
    if cell[1] < -64 || cell[1] > -53 {return false;}
    let dirs=[[1,0],[-1,0],[0,1],[0,-1]];
    for dir in dirs {for step in 0..7 {
        let sx=cell[0]-dir[0]*step; let sz=cell[2]-dir[1]*step;
        let seed=hash3(sx+24611*11,cell[1]-24611*7,sz+24611*13);
        if seed%100>=4 || dirs[((seed>>8)&3) as usize]!=dir || step>=3+((seed>>12)%5) as i32 {continue;}
        let near=[(0,0),(4,0),(-4,0),(0,4),(0,-4),(8,0),(-8,0),(0,8),(0,-8)].into_iter().any(|(dx,dz)|
            lava_top((sx+dx) as f64,(sz+dz) as f64).is_some_and(|top|(top-cell[1] as f64).abs()<=6.));
        if !near {continue;}
        let solid=(0..=step).all(|n| {
            let x=(sx+dir[0]*n) as f64; let z=(sz+dir[1]*n) as f64;
            cave_score([x,cell[1] as f64,z])<=0. && !river(x,z).is_some_and(|r|cell[1] as f64>=r.bottom && cell[1] as f64<=r.ceiling)
        });
        if solid {return true;}
    }} false
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Ore { Lodestone(u8), Copper }
/// Cardinal's seeded ellipsoidal Depths veins, sampled at exposed stone.
pub(crate) fn ore(p:[f64;3])->Option<Ore> {
    let [x,y,z]=p.map(|v|v.floor() as i32);
    if y <= -64 || y > -20 {return None;}
    fn vein(x:i32,y:i32,z:i32, copper:bool)->Option<u8> {
        let (seed,size,chance,rx,rz,yx,yz,syx,syz,rmin,rmax) = if copper {
            (17011,14,0.17,137,263,223,311,19,23,1.8,3.6)
        } else { (12037,16,0.23,179,211,241,337,13,17,2.0,4.5) };
        let mut best:Option<(f64,u8)>=None;
        for dz in -1..=1 { for dx in -1..=1 {
            let vx=x.div_euclid(size)+dx; let vz=z.div_euclid(size)+dz;
            let h=if copper {hash2(vx+seed*17,vz-seed*29)} else {hash2(vx+seed*19,vz-seed*23)};
            if ((h>>24)&255) as f64/255. > chance {continue;}
            let cx=(vx as f64+(h&255) as f64/255.)*size as f64;
            let cz=(vz as f64+((h>>8)&255) as f64/255.)*size as f64;
            let rh=hash2(vx*rx+seed,vz*rz-seed);
            let radius=rmin+(rmax-rmin)*(rh&255) as f64/255.;
            let radius_y=(radius*0.68).max(1.);
            let yh=hash2(vx*yx+seed*syx,vz*yz-seed*syz);
            let cy=-62+(yh%41) as i32;
            let score=((x as f64-cx).powi(2)+(z as f64-cz).powi(2))/(radius*radius)+((y-cy) as f64/radius_y).powi(2);
            if score<=1. && best.is_none_or(|v|score<v.0) {
                let variant=(hash2(vx*313+seed*7,vz*571-seed*11)%7) as u8;
                best=Some((score,variant));
            }
        }}
        best.map(|v|v.1)
    }
    if let Some(variant)=vein(x,y,z,false) {
        let h=hash3(x+12037*97,y+12037*131,z-12037*151);
        if ((h>>8)&255) as f64/255. <= 0.42 {return Some(Ore::Lodestone(variant));}
        // Cardinal gives lodestone precedence even if its replacement roll fails.
        return None;
    }
    if vein(x,y,z,true).is_some() {
        let h=hash3(x+17011*97,y+17011*131,z-17011*151);
        if ((h>>8)&255) as f64/255. <= 0.34 {return Some(Ore::Copper);}
    }
    None
}
/// Cardinal's purple dirt floor patch layout (12-metre cells, radius 2..5).
pub(crate) fn purple_floor_patch(x:f64,z:f64)->bool {
    let x=x.floor() as i32; let z=z.floor() as i32;
    let (cx,cz)=(x.div_euclid(12),z.div_euclid(12));
    for dz in -1..=1 {for dx in -1..=1 {
        let (vx,vz)=(cx+dx,cz+dz);
        if hash2(vx+911*37,vz-911*53)%100>=16 {continue;}
        let h=hash2(vx*131+911,vz*173-911);
        let px=vx*12+((h&255)*12/256) as i32;
        let pz=vz*12+(((h>>8)&255)*12/256) as i32;
        let radius=2+((h>>16)%4) as i32;
        if (x-px).pow(2)+(z-pz).pow(2)<=radius*radius {return true;}
    }} false
}

#[cfg(test)] mod river_shore_tests {
 use super::*;
 #[test] fn dry_bank_samples_preserve_occupancy_and_supply_signed_mesh_boundary() {
  let mut wet=0;let mut dry=0;
  for x in (-160..160).step_by(4) {for z in (-160..160).step_by(4) {
   if let Some(sample)=river_boundary(x as f64,z as f64) {
    if sample.shore>0. {let water=river(x as f64,z as f64).unwrap();assert_eq!(water.water,sample.water);assert_eq!(water.shore,sample.shore);wet+=1;}
    else {assert!(river(x as f64,z as f64).is_none());dry+=1;}
   }
  }}
  assert!(wet>0&&dry>0);
 }
}
