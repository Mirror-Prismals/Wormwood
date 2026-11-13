//! Persistent plant identity and reusable structure. Derived display detail never
//! owns life/damage state. Terrain samples are artistic inputs, not calibrated ecology.
#[path = "Understory.rs"]
pub(crate) mod understory;
#[path = "BranchGraph.rs"]
pub mod branch_graph;
#[path = "Geometry.rs"]
pub mod geometry;
#[path = "Math.rs"]
pub mod math;
#[path = "Packet.rs"]
pub mod packet;
#[path = "Forest.rs"]
pub(crate) mod forest;
#[path = "Profiles.rs"]
pub mod profiles;
#[path = "Species.rs"]
pub mod species;
#[path="Impostor.rs"]
pub mod impostor;
#[path = "Mechanics.rs"]
pub mod mechanics;
pub use branch_graph::Tree;
use math::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug)]
pub struct Site {
    pub id: u64,
    pub normal: V,
    pub origin: V,
    pub profile: usize,
    pub moisture: f64,
    pub soil_depth: f64,
}
#[derive(Clone, Debug)]
pub struct PlantState {
    pub age_years: f64,
    pub alive: bool,
    pub broken_branches: BTreeSet<u32>,
    pub vacant_years: f64,
    pub generation: u64,
}
#[derive(Clone, Default)]
pub struct State {
    pub revision: u64,
    pub plants: BTreeMap<u64, PlantState>,
    pub mechanics: mechanics::World,
    /// Session-only transfer to the game material solver; living identity stays intact.
    pub materialized: BTreeSet<u64>,
    /// Authored building footprints, session-only; plant identity is preserved.
    pub clearings: Vec<(V, f64)>,
}
pub fn initial_state(site: Site) -> PlantState {
    PlantState {
        age_years: 18. + random(site.id, 11) * 95.,
        alive: true,
        broken_branches: BTreeSet::new(),
        vacant_years: 0.,
        generation: 0,
    }
}
impl State {
    pub fn reserved(&self, site: Site) -> bool {
        self.clearings.iter().any(|(center,radius)| {
            let delta=sub(site.origin,*center);
            let tangent=sub(delta,mul(site.normal,dot(delta,site.normal)));
            length(tangent)<*radius
        })
    }
    pub fn plant(&self, site: Site) -> PlantState {
        self.plants
            .get(&site.id)
            .cloned()
            .unwrap_or_else(|| initial_state(site))
    }
    pub fn tree(&self, site: Site) -> Option<Tree> {
        if self.reserved(site) {return None;}
        if self.materialized.contains(&site.id) { return None; }
        let state = self.plant(site);
        if state.alive {
            if let Some(tree) = self.mechanics.rooted_tree(site.id) { return Some(tree); }
        }
        state
            .alive
            .then(|| Tree::generate(site, &state, profiles::profile(site.profile)))
    }
    pub fn snapshot(&self) -> Value {
        json!({"schema_version":2,"revision":self.revision,"mechanics":self.mechanics.snapshot(),"plants":self.plants.iter().map(|(id,p)|json!({"site_id":id,"age_years":p.age_years,"alive":p.alive,"broken_branches":p.broken_branches,"vacant_years":p.vacant_years,"generation":p.generation})).collect::<Vec<_>>()})
    }
    pub fn restore(&mut self, v: &Value) -> Result<(), String> {
        if v["schema_version"] == 1 {
            return Err("legacy vegetation IDs are ambiguous; snapshot preserved, explicit identity migration required".into());
        }
        if v["schema_version"] != 2 {
            return Err("vegetation snapshot version unsupported".into());
        }
        let rows = v["plants"].as_array().ok_or("plant array required")?;
        if rows.len() > 16384 {
            return Err("vegetation state budget exceeded".into());
        }
        let mut plants = BTreeMap::new();
        for row in rows {
            let id = row["site_id"].as_u64().ok_or("site_id required")?;
            let age = row["age_years"]
                .as_f64()
                .filter(|n| n.is_finite() && (0.0..=10000.).contains(n))
                .ok_or("invalid age")?;
            let alive = row["alive"].as_bool().ok_or("alive required")?;
            let ids = row["broken_branches"]
                .as_array()
                .filter(|r| r.len() <= branch_count_limit())
                .ok_or("invalid broken branches")?;
            let mut broken = BTreeSet::new();
            for id in ids {
                broken.insert(id.as_u64().filter(|n| *n < branch_count_limit() as u64).ok_or("invalid branch id")? as u32);
            }
            if plants
                .insert(
                    id,
                    PlantState {
                        age_years: age,
                        alive,
                        broken_branches: broken,
                        vacant_years: row["vacant_years"].as_f64().unwrap_or(0.),
                        generation: row["generation"].as_u64().unwrap_or(0),
                    },
                )
                .is_some()
            {
                return Err("duplicate plant identity".into());
            }
        }
        let revision = v["revision"].as_u64().ok_or("revision required")?;
        if plants.values().any(|p|!p.vacant_years.is_finite()||!(0.0..=10000.).contains(&p.vacant_years)){return Err("invalid recruitment clock".into());}
        let mechanics=mechanics::World::restore(&v["mechanics"])?;
        self.plants = plants;
        self.revision = revision;
        self.mechanics=mechanics;
        self.materialized.clear();
        Ok(())
    }
    pub fn damage(&mut self, site: Site, branch: Option<u32>) -> Result<(), String> {
        if branch.is_some_and(|b| b as usize >= branch_count(site.profile)) {
            return Err("invalid branch id".into());
        }
        if !self.plants.contains_key(&site.id) && self.plants.len() >= 16384 {
            return Err("vegetation state budget exceeded".into());
        }
        let p = self
            .plants
            .entry(site.id)
            .or_insert_with(|| initial_state(site));
        let changed = if let Some(b) = branch {
            p.broken_branches.insert(b)
        } else {
            let old = p.alive;
            p.alive = false;
            p.vacant_years=0.;
            old
        };
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
    /// Explicit ecology clock, never wall-clock aging of every rendered plant.
    /// Local light/soil/moisture competition and recruitment on stable sites.
    /// This is a deliberately reduced ecological model, not species calibration.
    pub fn grow(&mut self, sites: &[Site], years: f64) -> Result<(), String> {
        if !years.is_finite() || !(0.0..=1.).contains(&years) || sites.len() > 512 {
            return Err("bounded growth step required".into());
        }
        let missing = sites
            .iter()
            .map(|s| s.id)
            .collect::<BTreeSet<_>>()
            .iter()
            .filter(|id| !self.plants.contains_key(*id))
            .count();
        if self.plants.len() + missing > 16384 {
            return Err("vegetation state budget exceeded".into());
        }
        let canopy:Vec<_>=sites.iter().filter_map(|s|self.tree(*s)).collect();
        let mut changed = false;
        let mut seen = BTreeSet::new();
        for site in sites {
            if !seen.insert(site.id) {
                continue;
            }
            let p = self
                .plants
                .entry(site.id)
                .or_insert_with(|| initial_state(*site));
            let shade: f64=canopy.iter().filter(|t|t.site.id!=site.id).map(|t|{
                let distance=length(sub(t.site.origin,site.origin));
                let overlap=(1.-distance/(t.crown_radius+1.).max(0.1)).max(0.);
                overlap*overlap*(t.height/12.).min(1.)
            }).sum();
            let light=1./(1.+shade*4.);
            if p.alive && years > 0. {
                p.age_years = (p.age_years
                    + years * (0.35 + 0.65 * site.moisture) * site.soil_depth.min(1.) * light)
                .min(10000.);
                changed = true;
            } else if !p.alive && years>0. {
                p.vacant_years=(p.vacant_years+years).min(10000.);
                if p.vacant_years>=0.25 && site.moisture>=0.2 && site.soil_depth>=0.15 && light>=0.25 {
                    p.alive=true;p.age_years=0.;p.vacant_years=0.;p.broken_branches.clear();p.generation=p.generation.checked_add(1).ok_or("plant generation exhausted")?;
                }
                changed=true;
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
}

/// Six cube faces and two signed 20-bit coordinates fit without overlap in u64.
/// At 14m spacing Earth's cube-face coordinates fit within +/-455072. A hash of
/// XORed signed products is not injective: opposite odd pairs collide exactly.
fn site_identity(axis: usize, negative: bool, x: i64, z: i64) -> Option<u64> {
    const BIAS: i64 = 1 << 19;
    if axis >= 3 || !(-BIAS..BIAS).contains(&x) || !(-BIAS..BIAS).contains(&z) {
        return None;
    }
    let face = (axis * 2 + usize::from(negative)) as u64;
    Some((1u64 << 63) | (face << 40) | (((x + BIAS) as u64) << 20) | (z + BIAS) as u64)
}

/// Resolve one stable root directly, with no nearest-site ranking or render cap.
/// The placement inputs match discovery; state remains separate from placement.
pub fn site_by_id(id: u64) -> Option<Site> {
    if id >> 63 != 1 || ((id >> 43) & ((1u64 << 20) - 1)) != 0 {
        return None;
    }
    let face = ((id >> 40) & 7) as usize;
    if face >= 6 {
        return None;
    }
    let axis = face / 2;
    let sign = if face % 2 == 0 { 1. } else { -1. };
    let x = ((id >> 20) & ((1 << 20) - 1)) as i64 - (1 << 19);
    let z = (id & ((1 << 20) - 1)) as i64 - (1 << 19);
    let placement_seed = hash(
        (x as u64).wrapping_mul(0x632be59bd9b4e019)
            ^ (z as u64).wrapping_mul(0x9e3779b97f4a7c15)
            ^ ((axis as u64 + if sign > 0. { 7 } else { 11 }) << 48)
            ^ 0x74726565,
    );
    let mut raw = [0.; 3];
    raw[axis] = sign;
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    raw[u] = (x as f64 + 0.15 + 0.7 * random(placement_seed, 1)) * 14. / crate::terrain::RADIUS;
    raw[v] = (z as f64 + 0.15 + 0.7 * random(placement_seed, 2)) * 14. / crate::terrain::RADIUS;
    if raw[u].abs() >= 1. || raw[v].abs() >= 1. {
        return None;
    }
    let normal = unit(raw);
    if !crate::terrain::regions::forest(normal) { return None; }
    let sample = crate::terrain::sample(normal);
    let suitability = (0.55 + 0.45 * (sample.soil_depth / 0.7).clamp(0., 1.)) * (0.70 + 0.30 * sample.moisture);
    if random(placement_seed, 3) > suitability * 1.25 * crate::terrain::woodland::density(normal) {
        return None;
    }
    Some(Site {
        id,
        normal,
        origin: mul(normal, crate::terrain::RADIUS + sample.height),
        profile: if crate::terrain::regions::region(normal)==crate::terrain::regions::Region::Jungle {profiles::JUNGLE} else {profiles::select(placement_seed, sample.moisture, sample.soil_depth)},
        moisture: sample.moisture,
        soil_depth: sample.soil_depth,
    })
}

/// Cube-face ownership is fixed in world space. It is independent of rendering,
/// query order and LOD. Radius is metres; caller limit bounds returned records.
pub fn sites_near(position: V, radius: f64, limit: usize) -> Vec<Site> {
    if !position.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
        || length(position) < 1.
        || !radius.is_finite()
        || radius <= 0.
        || radius > 2048.
        || limit == 0
    {
        return vec![];
    }
    let normal = unit(position);
    let spacing = 14.;
    let maximum = normal.iter().map(|v| v.abs()).fold(0., f64::max);
    let mut sites = Vec::new();
    for axis in 0..3 {
        if normal[axis].abs() < maximum - radius * 4. / crate::terrain::RADIUS {
            continue;
        }
        let sign = normal[axis].signum();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let centre = [
            (normal[u] / normal[axis].abs() * crate::terrain::RADIUS / spacing).floor() as i64,
            (normal[v] / normal[axis].abs() * crate::terrain::RADIUS / spacing).floor() as i64,
        ];
        let count = (radius * 1.8 / spacing).ceil() as i64 + 2;
        for x in centre[0] - count..=centre[0] + count {
            for z in centre[1] - count..=centre[1] + count {
                let Some(id) = site_identity(axis, sign < 0., x, z) else {
                    continue;
                };
                // Preserve existing continuous root positions/habitat inputs;
                // this legacy noise seed is no longer an identity/state key.
                let placement_seed = hash(
                    (x as u64).wrapping_mul(0x632be59bd9b4e019)
                        ^ (z as u64).wrapping_mul(0x9e3779b97f4a7c15)
                        ^ ((axis as u64 + if sign > 0. { 7 } else { 11 }) << 48)
                        ^ 0x74726565,
                );
                let mut raw = [0.; 3];
                raw[axis] = sign;
                raw[u] = (x as f64 + 0.15 + 0.7 * random(placement_seed, 1)) * spacing
                    / crate::terrain::RADIUS;
                raw[v] = (z as f64 + 0.15 + 0.7 * random(placement_seed, 2)) * spacing
                    / crate::terrain::RADIUS;
                if raw[u].abs() >= 1. || raw[v].abs() >= 1. {
                    continue;
                }
                let n = unit(raw);
                let d = length(mul(sub(n, normal), crate::terrain::RADIUS));
                if d > radius {
                    continue;
                }
                if !crate::terrain::regions::forest(n) { continue; }
                let sample = crate::terrain::sample(n);
                // No hard loam-only exclusion: soil-bearing slopes can retain forest.
                let suitability =
                    (0.55 + 0.45 * (sample.soil_depth / 0.7).clamp(0., 1.)) * (0.70 + 0.30 * sample.moisture);
                if random(placement_seed, 3) > suitability * 1.25 * crate::terrain::woodland::density(n) {
                    continue;
                }
                let origin = mul(n, crate::terrain::RADIUS + sample.height);
                sites.push((
                    d,
                    Site {
                        id,
                        normal: n,
                        origin,
                        profile: if crate::terrain::regions::region(normal)==crate::terrain::regions::Region::Jungle {profiles::JUNGLE} else {profiles::select(placement_seed, sample.moisture, sample.soil_depth)},
                        moisture: sample.moisture,
                        soil_depth: sample.soil_depth,
                    },
                ));
            }
        }
    }
    sites.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    sites.truncate(limit.min(4096));
    sites.into_iter().map(|(_, s)| s).collect()
}

pub fn query_perches(state: &State, input: &Value) -> Result<Value, String> {
    if input.get("keys").is_some() {
        return refresh_perches(state, input);
    }
    let position: V =
        serde_json::from_value(input["position"].clone()).map_err(|e| format!("position: {e}"))?;
    let up: V = serde_json::from_value(input["up"].clone()).map_err(|e| format!("up: {e}"))?;
    let min = input["min_distance"]
        .as_f64()
        .ok_or("min_distance required")?;
    let max = input["max_distance"]
        .as_f64()
        .ok_or("max_distance required")?;
    let limit = input["limit"]
        .as_u64()
        .filter(|n| *n <= 12)
        .ok_or("perch limit must be <=12")? as usize;
    if !position
        .iter()
        .chain(up.iter())
        .all(|v| v.is_finite() && v.abs() <= 1e9)
        || length(position) < 1.
        || (length(up) - 1.).abs() > 1e-4
        || !min.is_finite()
        || !max.is_finite()
        || min < 0.
        || max < min
        || max > 1800.
    {
        return Err("invalid perch query bounds".into());
    }
    let seed = input["seed"].as_u64().unwrap_or(0);
    let time = input["time"].as_f64().unwrap_or(0.);
    if !time.is_finite() || time.abs() > 1e9 {
        return Err("invalid perch time".into());
    }
    let sites = sites_near(position, max + 32., 512);
    let mut candidates = Vec::new();
    for site in sites {
        let Some(tree) = state.tree(site) else {
            continue;
        };
        for b in tree
            .branches
            .iter()
            .filter(|b| b.order == 1 && b.radius > 0.025)
        {
            // Outer upper branch: clear of the trunk; use the same wind as wood.
            let local = add(b.start, mul(sub(b.end, b.start), 0.82));
            let p = tree.world(tree.wind(local, time));
            let distance = length(sub(p, position));
            if distance < min || distance > max {
                continue;
            }
            let occupied = tree
                .branches
                .iter()
                .filter(|other| other.id != b.id && other.parent != Some(b.id))
                .any(|other| {
                    length(sub(other.end, add(local, [0., 0.28, 0.]))) < 0.32 + other.radius
                });
            if occupied {
                continue;
            }
            candidates.push((
                hash(site.id ^ seed ^ (b.id as u64).wrapping_mul(0x9e3779b97f4a7c15)),
                site.id,
                b.id,
                p,
                site.normal,
            ));
        }
    }
    candidates.sort_by_key(|c| c.0);
    let mut accepted: Vec<Value> = Vec::new();
    let mut positions = Vec::new();
    for (_, id, branch, p, up) in candidates {
        if accepted.len() >= limit {
            break;
        }
        if positions.iter().any(|q| length(sub(*q, p)) < 7.) {
            continue;
        }
        positions.push(p);
        accepted
            .push(json!({"site_id":id,"branch_id":branch,"position":p,"up":up,"suitable":true}));
    }
    Ok(json!({"revision":state.revision,"perches":accepted}))
}

/// Refresh occupied habitat by identity, without distance-band/candidate ranking.
/// A bounded lookup miss is uncertainty, never evidence of plant removal.
fn refresh_perches(state: &State, input: &Value) -> Result<Value, String> {
    let keys = input["keys"]
        .as_array()
        .filter(|keys| keys.len() <= 12)
        .ok_or("perch refresh requires at most 12 keys")?;
    let time = input["time"]
        .as_f64()
        .filter(|t| t.is_finite() && t.abs() <= 1e9)
        .ok_or("explicit finite perch refresh time required")?;
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::new();
    for key in keys {
        let id = key["site_id"].as_u64().ok_or("perch site_id required")?;
        let branch = key["branch_id"]
            .as_u64()
            .filter(|id| *id < branch_count_limit() as u64)
            .ok_or("invalid perch branch_id")? as u32;
        let position: V = serde_json::from_value(key["position"].clone())
            .map_err(|_| "last perch position required")?;
        if !position.iter().all(|n| n.is_finite() && n.abs() <= 1e9) || length(position) < 1. {
            return Err("invalid last perch position".into());
        }
        if !seen.insert((id, branch)) {
            return Err("duplicate perch refresh key".into());
        }
        parsed.push((id, branch, position));
    }
    let mut perches = Vec::new();
    let mut removed = Vec::new();
    let mut unresolved = Vec::new();
    for (id, branch, position) in parsed {
        let key = json!({"site_id":id,"branch_id":branch});
        let Some(site) = site_by_id(id).filter(|site| length(sub(site.origin, position)) <= 48.)
        else {
            unresolved.push(key);
            continue;
        };
        let Some(tree) = state.tree(site) else {
            removed.push(key);
            continue;
        };
        let Some(b) = tree
            .branches
            .iter()
            .find(|b| b.id == branch && b.order == 1 && b.radius > 0.025)
        else {
            removed.push(key);
            continue;
        };
        let local = add(b.start, mul(sub(b.end, b.start), 0.82));
        let occupied = tree
            .branches
            .iter()
            .filter(|other| other.id != b.id && other.parent != Some(b.id))
            .any(|other| length(sub(other.end, add(local, [0., 0.28, 0.]))) < 0.32 + other.radius);
        if occupied {
            removed.push(key);
            continue;
        }
        perches.push(json!({"site_id":id,"branch_id":branch,"position":tree.world(tree.wind(local,time)),"up":site.normal,"suitable":true}));
    }
    Ok(
        json!({"revision":state.revision,"perches":perches,"removed":removed,"unresolved":unresolved}),
    )
}

#[cfg(test)]
pub(crate) fn forest_test_position()->V {
    let n=unit([4125.,crate::terrain::RADIUS,-125.]);
    mul(n,crate::terrain::RADIUS+crate::terrain::height(n)+1.7)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_high_branch_perches_refresh_and_damage_roundtrip() {
        let position=forest_test_position();
        let mut state=State::default();let mut high=None;
        for seed in 0..32 {
            let found=query_perches(&state,&json!({"position":position,"up":[0.,1.,0.],"min_distance":40.,"max_distance":100.,"limit":12,"seed":seed})).unwrap();
            for p in found["perches"].as_array().unwrap() {
                let result=refresh_perches(&state,&json!({"keys":[p],"time":2.})).unwrap();
                assert_eq!(result["perches"].as_array().unwrap().len(),1);
                if p["branch_id"].as_u64().unwrap()>=31 {high=Some(p.clone());}
            }
        }
        let p=high.expect("authored forest must exercise a branch above the legacy bound");
        let site=site_by_id(p["site_id"].as_u64().unwrap()).unwrap();
        state.damage(site,Some(p["branch_id"].as_u64().unwrap() as u32)).unwrap();
        let snapshot=state.snapshot();let mut restored=State::default();restored.restore(&snapshot).unwrap();assert_eq!(snapshot,restored.snapshot());
        let removed=refresh_perches(&restored,&json!({"keys":[p.clone()],"time":3.})).unwrap();
        assert_eq!(removed["removed"].as_array().unwrap().len(),1);
        let mut invalid=p;invalid["branch_id"]=json!(branch_count_limit());
        assert!(refresh_perches(&restored,&json!({"keys":[invalid],"time":3.})).is_err());
        let before=restored.snapshot();assert!(restored.damage(site,Some(branch_count(site.profile) as u32)).is_err());assert_eq!(before,restored.snapshot());
    }
    #[test]
    fn state_roundtrip_retains_age_damage_and_identity() {
        let site = sites_near(forest_test_position(), 100., 32)[0];
        let mut state = State::default();
        state.grow(&[site], 0.5).unwrap();
        state.damage(site, Some(4)).unwrap();
        let snapshot = state.snapshot();
        let mut restored = State::default();
        restored.restore(&snapshot).unwrap();
        assert_eq!(snapshot, restored.snapshot());
        let tree = restored.tree(site).unwrap();
        assert!(
            !tree
                .branches
                .iter()
                .any(|b| b.id == 4 || b.parent == Some(4))
        );
        let before = restored.snapshot();
        assert!(restored.restore(&json!({"schema_version":2,"revision":0,"plants":[{"site_id":42,"age_years":-2.,"alive":true,"broken_branches":[]}]})).is_err());
        assert_eq!(before, restored.snapshot());
        let mut legacy = snapshot.clone();
        legacy["schema_version"] = json!(1);
        assert!(restored.restore(&legacy).unwrap_err().contains("ambiguous"));
        assert_eq!(before, restored.snapshot());
    }
    #[test]
    fn tree_identity_is_not_a_render_query_index() {
        let position = forest_test_position();
        let sites = sites_near(position, 100., 128);
        assert!(!sites.is_empty());
        let moved = sites_near(add(forest_test_position(),[2.,0.,1.]), 110., 512);
        assert_eq!(
            sites
                .iter()
                .map(|site| site.id)
                .collect::<BTreeSet<_>>()
                .len(),
            sites.len()
        );
        assert_eq!(
            moved
                .iter()
                .map(|site| site.id)
                .collect::<BTreeSet<_>>()
                .len(),
            moved.len()
        );
        let mut state = State::default();
        state.grow(&sites, 0.5).unwrap();
        state.damage(sites[0], Some(4)).unwrap();
        let snapshot = state.snapshot();
        for s in sites {
            let same = moved
                .iter()
                .find(|other| other.id == s.id)
                .expect("shared region retains site identity");
            assert_eq!(s.origin, same.origin);
            let a = state.tree(s).unwrap();
            let b = state.tree(*same).unwrap();
            assert_eq!(a.site.origin, b.site.origin);
            assert_eq!(a.height, b.height);
            assert_eq!(
                a.branches
                    .iter()
                    .map(|branch| branch.id)
                    .collect::<Vec<_>>(),
                b.branches
                    .iter()
                    .map(|branch| branch.id)
                    .collect::<Vec<_>>()
            );
            let point = [0., a.height * 0.8, 0.];
            assert_ne!(a.wind(point, 0.), b.wind(point, 1.7));
            assert_eq!(a.site.origin, b.site.origin);
        }
        assert_eq!(state.snapshot(), snapshot);
    }

    #[test]
    fn plant_identity_encodes_face_and_signed_coordinates_without_xor_aliases() {
        let mut ids = BTreeSet::new();
        for axis in 0..3 {
            for negative in [false, true] {
                for x in [-524288, -455072, -1, 0, 1, 455071, 524287] {
                    for z in [-524288, -455072, -1, 0, 1, 455071, 524287] {
                        assert!(ids.insert(site_identity(axis, negative, x, z).unwrap()));
                    }
                }
            }
        }
        assert_ne!(
            site_identity(1, false, -1, -1),
            site_identity(1, false, 1, 1)
        );
        assert!(site_identity(1, false, 524288, 0).is_none());
        assert!(site_identity(1, false, 0, -524289).is_none());
    }

    #[test]
    fn direct_site_resolution_matches_discovery_roots_across_faces_and_seams() {
        let mut resolved = 0;
        for direction in [
            unit(forest_test_position()),
            [1., 0., 0.],
            [-1., 0., 0.],
            [0., 1., 0.],
            [0., -1., 0.],
            [0., 0., 1.],
            [0., 0., -1.],
            [1., 1., 0.],
            [-1., 1., 0.],
            [0., 1., 1.],
        ] {
            let position = mul(unit(direction), crate::terrain::RADIUS);
            for expected in sites_near(position, 100., 512) {
                let actual =
                    site_by_id(expected.id).expect("discovered identity must resolve directly");
                assert_eq!(actual.origin, expected.origin);
                assert_eq!(actual.normal, expected.normal);
                assert_eq!(actual.profile, expected.profile);
                assert_eq!(actual.moisture, expected.moisture);
                assert_eq!(actual.soil_depth, expected.soil_depth);
                resolved += 1;
            }
        }
        assert!(resolved > 0);
        assert!(site_by_id(42).is_none());
        assert!(site_by_id((1u64 << 63) | (6u64 << 40)).is_none());
        assert!(site_by_id((1u64 << 63) | (1u64 << 43)).is_none());
    }
    #[test]
    fn perches_obey_bounds_and_removed_tree_state() {
        let position = forest_test_position();
        let mut state = State::default();
        let query = json!({"position":position,"up":[0.,1.,0.],"min_distance":40.,"max_distance":60.,"limit":12,"seed":9});
        let before = query_perches(&state, &query).unwrap();
        assert!(
            !before["perches"].as_array().unwrap().is_empty(),
            "reference forest must supply actual habitat"
        );
        for p in before["perches"].as_array().unwrap() {
            let world: V = serde_json::from_value(p["position"].clone()).unwrap();
            let d = length(sub(world, position));
            assert!((40.0..=60.).contains(&d));
        }
        let ids: BTreeSet<_> = before["perches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["site_id"].as_u64().unwrap())
            .collect();
        for site in sites_near(position, 100., 512) {
            if ids.contains(&site.id) {
                state.damage(site, None).unwrap();
            }
        }
        let after = query_perches(&state, &query).unwrap();
        assert!(
            after["perches"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| !ids.contains(&p["site_id"].as_u64().unwrap()))
        );
        assert!(query_perches(&state,&json!({"position":position,"up":[0.,1.,0.],"min_distance":1.,"max_distance":-1.,"limit":12})).is_err());
    }

    #[test]
    fn severed_trunk_removes_transitive_wood_and_leaf_geometry() {
        let site = sites_near(forest_test_position(), 100., 32)[0];
        let mut state = State::default();
        let original = state.tree(site).unwrap();
        assert!(original.branches.iter().any(|b| b.order == 2));
        state.damage(site, Some(0)).unwrap();
        let severed = state.tree(site).unwrap();
        assert!(severed.branches.is_empty());
        for detail in [
            geometry::Detail::Near,
            geometry::Detail::Middle,
            geometry::Detail::Far,
        ] {
            assert_eq!(
                geometry::build(&severed, profiles::profile(site.profile), detail, 0.).triangles(),
                0
            );
        }
        let revision = state.revision;
        state.damage(site, Some(0)).unwrap();
        assert_eq!(state.revision, revision);
        let before = state.snapshot();
        assert!(state.damage(site, Some(branch_count(site.profile) as u32)).is_err());
        assert_eq!(state.snapshot(), before);
    }

    #[test]
    fn targeted_perches_follow_wind_and_distinguish_removal_from_lookup_miss() {
        let position = forest_test_position();
        let mut state = State::default();
        let found = query_perches(&state, &json!({"position":position,"up":[0.,1.,0.],"min_distance":40.,"max_distance":60.,"limit":12,"seed":9})).unwrap();
        let perch = found["perches"][0].clone();
        let key = json!({"site_id":perch["site_id"],"branch_id":perch["branch_id"],"position":perch["position"]});
        let refreshed = query_perches(&state, &json!({"time":1.7,"keys":[key.clone()]})).unwrap();
        assert_eq!(refreshed["perches"].as_array().unwrap().len(), 1);
        assert_ne!(perch["position"], refreshed["perches"][0]["position"]);
        assert!(refreshed["removed"].as_array().unwrap().is_empty());
        let site = sites_near(position, 100., 512)
            .into_iter()
            .find(|site| Some(site.id) == perch["site_id"].as_u64())
            .unwrap();
        state
            .damage(site, Some(perch["branch_id"].as_u64().unwrap() as u32))
            .unwrap();
        let removed = query_perches(&state, &json!({"time":1.7,"keys":[key.clone()]})).unwrap();
        assert_eq!(removed["removed"].as_array().unwrap().len(), 1);
        assert!(removed["perches"].as_array().unwrap().is_empty());
        let mut misplaced = key;
        misplaced["position"] = json!([0., -crate::terrain::RADIUS, 0.]);
        let unresolved = query_perches(&state, &json!({"time":1.7,"keys":[misplaced]})).unwrap();
        assert_eq!(unresolved["unresolved"].as_array().unwrap().len(), 1);
        assert!(unresolved["removed"].as_array().unwrap().is_empty());
    }
}

#[path = "Season.rs"]
pub mod season;

// QueryWood and acoustic ingestion must agree on complete authored tree graphs.
// The historical 4096-segment assumption covered 128 generic 31-branch trees,
// but omitted the new 88-branch conifers. Derive the bound; never truncate wood
// or disable live game audio just to make an embedded DAW benchmark succeed.
pub(crate) const WOOD_SITE_LIMIT:usize=128;
pub(crate) fn wood_segment_limit()->usize {
    WOOD_SITE_LIMIT*branch_count_limit()
}

// Authored species exceed the legacy 31-branch graph. All identity boundaries
// must derive their bound from the same assets that QueryPerches/Tree generate.
// In particular, rejecting a valid high branch here terminates Play Mirror when
// a bird refreshes its occupied perch. Never restore a fixed 31-branch limit.
pub(crate) fn branch_count(profile:usize)->usize {
    species::asset(profiles::profile(profile)).map_or(31,|a|a.branches.len())
}
pub(crate) fn branch_count_limit()->usize {
    (0..profiles::PROFILES.len()).map(branch_count).max().unwrap_or(31)
}
