//! Distant woodland silhouettes. The quadtree selects display resolution only;
//! stable 14 m tree sites, habitat and gameplay wood remain authoritative.
use super::{Site, State, math::*, packet::Geometry};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct Cell {
    face: usize,
    x: i64,
    z: i64,
    size: i64,
}
struct Crown {
    origin: V,
    vertices: Vec<[f32; 16]>,
    samples: usize,
    impostors: Vec<(super::Tree, super::profiles::Profile, f64)>,
}
impl Crown { fn triangles(&self)->usize {self.vertices.len()/3+self.impostors.len()*2} }
#[derive(Default)]
pub(crate) struct Forest {
    crowns: HashMap<Cell, Option<Crown>>,
    centers: HashMap<Cell, V>,
    revision: u64,
    season: u64,
}

/// Conservative frustum common to full trees and canopy envelopes.
pub(crate) struct View {
    pub eye: V,
    forward: V,
    right: V,
    up: V,
    aspect: f64,
}
impl View {
    pub fn from_input(input: &Value) -> Result<Self, String> {
        let eye = serde_json::from_value(input["position"].clone())
            .map_err(|_| "forest position required")?;
        let yaw = input["yaw"].as_f64().unwrap_or(0.);
        let pitch = input["pitch"].as_f64().unwrap_or(0.);
        let axis =
            |key: &str, fallback: V| serde_json::from_value(input[key].clone()).unwrap_or(fallback);
        let v = Self {
            eye,
            forward: axis(
                "forward",
                [
                    yaw.sin() * pitch.cos(),
                    pitch.sin(),
                    -yaw.cos() * pitch.cos(),
                ],
            ),
            right: axis("right", [yaw.cos(), 0., yaw.sin()]),
            up: axis(
                "up",
                [
                    -yaw.sin() * pitch.sin(),
                    pitch.cos(),
                    yaw.cos() * pitch.sin(),
                ],
            ),
            aspect: input["aspect"].as_f64().unwrap_or(4. / 3.),
        };
        if !v
            .eye
            .iter()
            .chain(&v.forward)
            .chain(&v.right)
            .chain(&v.up)
            .all(|v| v.is_finite())
            || length(v.eye) < 1.
            || !(0.2..=8.).contains(&v.aspect)
        {
            return Err("invalid forest view".into());
        }
        Ok(v)
    }
    pub fn visible(&self, p: V, radius: f64) -> bool {
        let p = sub(p, self.eye);
        let z = dot(p, self.forward);
        let slope = 35f64.to_radians().tan();
        z + radius > 0.
            && dot(p, self.right).abs()
                < z.max(0.) * slope * self.aspect + radius * (1. + slope * self.aspect)
            && dot(p, self.up).abs() < z.max(0.) * slope + radius * (1. + slope)
    }
}
impl Cell {
    fn normal(self, u: f64, v: f64) -> V {
        let axis = self.face / 2;
        let mut p = [0.; 3];
        p[axis] = if self.face % 2 == 0 { 1. } else { -1. };
        p[(axis + 1) % 3] = (self.x as f64 + u * self.size as f64) * 14. / crate::terrain::RADIUS;
        p[(axis + 2) % 3] = (self.z as f64 + v * self.size as f64) * 14. / crate::terrain::RADIUS;
        unit(p)
    }
    fn id(self, x: i64, z: i64) -> Option<u64> {
        super::site_identity(self.face / 2, self.face % 2 == 1, x, z)
    }
    fn sampled_ids(self) -> Vec<u64> {
        let count = if self.size == 1 { 1 } else { 2 };
        (0..count).flat_map(|z| (0..count).filter_map(move |x|
            self.id(self.x + x * self.size / count, self.z + z * self.size / count)
        )).collect()
    }
    fn contains(self, id: u64) -> bool {
        let face = ((id >> 40) & 7) as usize;
        let x = ((id >> 20) & ((1 << 20) - 1)) as i64 - (1 << 19);
        let z = (id & ((1 << 20) - 1)) as i64 - (1 << 19);
        face == self.face
            && x >= self.x
            && x < self.x + self.size
            && z >= self.z
            && z < self.z + self.size
    }
    fn candidates(
        self,
        view: &View,
        range: f64,
        detail: f64,
        centers: &mut HashMap<Cell, V>,
        out: &mut Vec<Cell>,
    ) {
        let n = self.normal(0.5, 0.5);
        let p = *centers
            .entry(self)
            .or_insert_with(|| mul(n, crate::terrain::RADIUS + crate::terrain::height(n)));
        let width = self.size as f64 * 14.;
        let distance = length(sub(p, view.eye));
        if distance > range + width * 1.5 || !view.visible(p, width * 1.5 + 32.) {
            return;
        }
        if self.size > 1 && (distance < width * detail || distance < 192. + width * 1.5) {
            let h = self.size / 2;
            for z in [self.z, self.z + h] {
                for x in [self.x, self.x + h] {
                    Self {
                        x,
                        z,
                        size: h,
                        ..self
                    }
                    .candidates(view, range, detail, centers, out);
                }
            }
        } else {
            out.push(self);
        }
    }
}
impl Forest {
    pub fn append(
        &mut self,
        state: &State,
        input: &Value,
        view: &View,
        range: f64,
        budget: usize,
        replaced: &HashSet<u64>,
        out: &mut Geometry,
    ) -> Result<(), String> {
        // A cell is excluded when it contains any already drawn site. Index
        // the eight possible ancestors once instead of scanning every drawn
        // site for every candidate at every detail-budget retry.
        let indexed=std::env::var("WORMWOOD_FOREST_ANCESTORS").as_deref()!=Ok("0");
        let mut blocked=HashSet::with_capacity(if indexed{replaced.len()*8}else{0});
        for &id in replaced.iter().filter(|_|indexed) {
            let face=((id>>40)&7)as usize;
            let x=((id>>20)&((1<<20)-1))as i64-(1<<19);
            let z=(id&((1<<20)-1))as i64-(1<<19);
            for size in [1,2,4,8,16,32,64,128] {blocked.insert(Cell{face,x:x.div_euclid(size)*size,z:z.div_euclid(size)*size,size});}
        }
        let season = input["calendar"]["day_of_year"]
            .as_f64()
            .unwrap_or(180.)
            .to_bits()
            ^ input["calendar"]["year"].as_u64().unwrap_or(0);
        if self.revision != state.revision || self.season != season {
            self.crowns.clear();
            self.revision = state.revision;
            self.season = season;
        }
        let normal = unit(view.eye);
        let maximum = normal.iter().map(|v| v.abs()).fold(0., f64::max);
        if self.centers.len() > 60000 {
            self.centers.clear();
        }
        let mut cells = Vec::new();
        let mut detail = 16.;
        // Reduce representation uniformly if the view is exceptionally wide;
        // never spend the budget nearest-first and drop distant hills entirely.
        loop {
            cells.clear();
            for axis in 0..3 {
                if normal[axis].abs() < maximum - range * 4. / crate::terrain::RADIUS {
                    continue;
                }
                let face = axis * 2 + usize::from(normal[axis] < 0.);
                let x = (normal[(axis + 1) % 3] / normal[axis].abs() * crate::terrain::RADIUS / 14.)
                    .floor() as i64;
                let z = (normal[(axis + 2) % 3] / normal[axis].abs() * crate::terrain::RADIUS / 14.)
                    .floor() as i64;
                let root = 128;
                let reach = (range * 1.8 / (14. * root as f64)).ceil() as i64 + 1;
                for rz in z.div_euclid(root) - reach..=z.div_euclid(root) + reach {
                    for rx in x.div_euclid(root) - reach..=x.div_euclid(root) + reach {
                        Cell {
                            face,
                            x: rx * root,
                            z: rz * root,
                            size: root,
                        }
                        .candidates(
                            view,
                            range,
                            detail,
                            &mut self.centers,
                            &mut cells,
                        );
                    }
                }
            }
            let mut triangles = 0;
            for &cell in &cells {
                if if indexed{blocked.contains(&cell)}else{replaced.iter().any(|&id|cell.contains(id))} {
                    continue;
                }
                if let Some(crown) = self
                    .crowns
                    .entry(cell)
                    .or_insert_with(|| build(cell, state, input))
                {
                    triangles += crown.triangles();
                    if triangles > budget {
                        break;
                    }
                }
            }
            if triangles <= budget || detail <= 0.75 {
                break;
            }
            detail *= 0.80;
        }
        // Keep the cache local to the current view; no growth along a flight.
        if self.crowns.len() > 12000 {
            let keep: HashSet<_> = cells.iter().copied().collect();
            self.crowns.retain(|k, _| keep.contains(k));
        }
        let limit = out.indices.len() / 3 + budget;
        let mut clusters = 0;
        let mut sampled_trees = 0;
        let mut omitted = 0;
        for cell in cells {
            // A coarser emergency LOD must never envelop an already drawn tree.
            if if indexed{blocked.contains(&cell)}else{replaced.iter().any(|&id|cell.contains(id))} {
                continue;
            }
            let crown = self
                .crowns
                .entry(cell)
                .or_insert_with(|| build(cell, state, input));
            let Some(crown) = crown else {
                continue;
            };
            if !view.visible(
                add(crown.origin, mul(unit(crown.origin), 12.)),
                cell.size as f64 * 20. + 28.,
            ) {
                continue;
            }
            if out.indices.len() / 3 + crown.triangles() > limit {
                omitted += 1;
                continue;
            }
            let shift = sub(crown.origin, view.eye);
            let start = out.vertices.len() as u32;
            for v in &crown.vertices {
                let mut v = *v;
                for k in 0..3 {
                    v[k] = (v[k] as f64 + shift[k]) as f32;
                }
                out.vertices.push(v);
            }
            out.indices
                .extend((0..crown.vertices.len() as u32).map(|i| start + i));
            for (tree,profile,cover) in &crown.impostors {
                let (quad,tile)=super::impostor::quad(tree,view.eye,*profile,*cover);
                let start=out.vertices.len()as u32;
                for v in quad {
                    let p=sub(v.position,view.eye);
                    out.vertices.push([p[0]as f32,p[1]as f32,p[2]as f32,v.normal[0]as f32,v.normal[1]as f32,v.normal[2]as f32,v.uv[0],v.uv[1],v.color[0],v.color[1],v.color[2],6.,v.transmission,v.roughness,v.occlusion,tile]);
                }
                out.indices.extend([start,start+1,start+2,start,start+2,start+3]);
            }
            clusters += 1;
            sampled_trees += crown.samples;
        }
        out.metadata["forest"] = json!({"range_m":range,"clusters":clusters,"sampled_sites":sampled_trees,
            "cached_cells":self.crowns.len(),"budget_omitted_cells":omitted,"detail_ratio":detail,"representation":"multi-view silhouettes baked from detailed tree meshes"});
        Ok(())
    }
}
fn build(cell: Cell, state: &State, input: &Value) -> Option<Crown> {
    // Dyadic sites are nested: every representative in a parent cell is
    // also sampled by its children. Refinement adds trees at their authored
    // positions instead of relocating a meadow-centre sample at every LOD.
    let mut sites = Vec::<Site>::new();
    for id in cell.sampled_ids() {
        let Some(site) = super::site_by_id(id) else { continue; };
        if state.materialized.contains(&site.id)
            || !state.plant(site).alive
            || state.reserved(site)
        { continue; }
        sites.push(site);
    }
    if sites.is_empty() {
        return None;
    }
    let samples = sites.len();
    let origin = sites[0].origin;
    let mut vertices = Vec::new();
    let mut impostors=Vec::new();
    for site in &sites {
        let plant=state.plant(*site);
        let mut profile=super::profiles::profile(site.profile);
        let foliage=super::season::Foliage::for_profile(input,site.id,profile);
        profile.leaf_color=foliage.color;
        let Some(mut tree)=state.tree(*site) else{continue;};
        if super::impostor::supported(&tree)&&plant.broken_branches.is_empty() {
            // Stored cards need only the exact root/basis and growth scale.
            // Damaged/moving individuals retain their actual surviving geometry.
            tree.branches=Vec::new();
            impostors.push((tree,profile,foliage.cover));
        }else{
            let mesh=super::geometry::build_seasonal(&tree,profile,super::geometry::Detail::Far,0.,foliage.cover);
            for index in mesh.indices {
                let v=mesh.vertices[index as usize];let p=sub(tree.world(v.position),origin);let n=tree.world_direction(v.normal);
                vertices.push([p[0]as f32,p[1]as f32,p[2]as f32,n[0]as f32,n[1]as f32,n[2]as f32,v.uv[0],v.uv[1],v.color[0],v.color[1],v.color[2],v.material as f32,v.transmission,v.roughness,v.occlusion,profile.bark_tile()*1000.+profile.bark_ridge_scale as f32]);
            }
        }
    }
    if vertices.is_empty()&&impostors.is_empty(){return None;}
    Some(Crown {origin,vertices,samples,impostors})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forest_refinement_preserves_parent_site_identities() {
        for size in [2, 4, 8, 16, 32] {
            for face in 0..6 {
                let parent = Cell { face, x: -64, z: 32, size };
                let h = size / 2;
                let children: HashSet<_> = [0, h].into_iter().flat_map(|z|
                    [0, h].into_iter().flat_map(move |x|
                        Cell { x: parent.x + x, z: parent.z + z, size: h, ..parent }.sampled_ids()
                    )
                ).collect();
                assert!(parent.sampled_ids().iter().all(|id| children.contains(id)));
            }
        }
    }

    #[test]
    fn distant_forest_emits_wood_and_separate_foliage_within_budget() {
        let state = State::default();
        let input = json!({"position":crate::vegetation::forest_test_position(),
            "calendar":{"day_of_year":290,"year":2026}});
        let view = View::from_input(&input).unwrap();
        let mut forest = Forest::default();
        let mut out = Geometry {
            origin: view.eye,
            revision: state.revision,
            vertices: Vec::new(),
            indices: Vec::new(),
            metadata: json!({}),
        };
        forest
            .append(
                &state,
                &input,
                &view,
                6000.,
                8192,
                &HashSet::new(),
                &mut out,
            )
            .unwrap();
        assert!(out.indices.len() <= 8192 * 3);
        assert!(out.vertices.iter().any(|v| v[11] == 6.));
        assert!(out.vertices.iter().all(|v|v[11]!=5.));
        assert!(out.vertices.iter().flatten().all(|v| v.is_finite()));
        assert_eq!(out.metadata["forest"]["budget_omitted_cells"], 0);
        // Moving the camera and turning must not exhaust the reserved forest
        // budget and introduce whole missing cells at the representation seam.
        for (x, yaw) in [(100., 1.57), (400., 3.14), (-400., -1.57)] {
            let mut input = input.clone();
            input["position"][0] = json!(4125.+x);
            input["yaw"] = json!(yaw);
            let view = View::from_input(&input).unwrap();
            out.vertices.clear();
            out.indices.clear();
            out.origin = view.eye;
            forest.append(&state, &input, &view, 6000., 8192, &HashSet::new(), &mut out).unwrap();
            assert!(out.indices.len() <= 8192 * 3);
            assert_eq!(out.metadata["forest"]["budget_omitted_cells"], 0);
        }
    }
    #[test]
    fn damaged_tree_never_reappears_as_an_undamaged_baked_crown() {
        let site=super::super::sites_near(crate::vegetation::forest_test_position(),400.,4096).into_iter().find(|s|s.profile==5).unwrap();
        let cell=Cell{face:((site.id>>40)&7)as usize,x:((site.id>>20)&((1<<20)-1))as i64-(1<<19),z:(site.id&((1<<20)-1))as i64-(1<<19),size:1};
        let mut state=State::default();let input=json!({"calendar":{"day_of_year":190,"days_in_year":365}});
        let healthy=build(cell,&state,&input).unwrap();assert_eq!(healthy.impostors.len(),1);
        let mut plant=super::super::initial_state(site);plant.broken_branches.insert(4);state.plants.insert(site.id,plant);
        let damaged=build(cell,&state,&input).unwrap();assert!(damaged.impostors.is_empty());assert!(!damaged.vertices.is_empty());assert!(damaged.vertices.iter().all(|v|v[11]!=5.&&v[11]!=6.));
        state.plants.get_mut(&site.id).unwrap().broken_branches.insert(0);
        assert!(build(cell,&state,&input).is_none());
    }

}

#[cfg(test)]mod ancestor_tests {
 use super::*;
 #[test]fn indexed_exclusions_match_all_original_negative_and_positive_cells(){
  let ids:HashSet<_>=(0..300).filter_map(|i|super::super::site_identity(i%3,i%2==1,(i as i64*73)%501-250,(i as i64*31)%601-300)).collect();
  let mut blocked=HashSet::new();for &id in &ids {let face=((id>>40)&7)as usize;let x=((id>>20)&((1<<20)-1))as i64-(1<<19);let z=(id&((1<<20)-1))as i64-(1<<19);for size in [1,2,4,8,16,32,64,128]{blocked.insert(Cell{face,x:x.div_euclid(size)*size,z:z.div_euclid(size)*size,size});}}
  for face in 0..6 {for size in [1,2,4,8,16,32,64,128]{for x in -10..10 {for z in -10..10{let cell=Cell{face,x:x*size,z:z*size,size};assert_eq!(blocked.contains(&cell),ids.iter().any(|&id|cell.contains(id)));}}}}
 }
}
