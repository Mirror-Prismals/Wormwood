//! Bonded, volume-preserving material cells. Compliant interfaces carry load
//! and fracture work; each surviving piece retains its material-space shape.
//! This reduced solid model is not molecular dynamics or calibrated FEM.
use super::solid_geometry::{self, INTERIOR};
use super::*;
use crate::physics::{cut_geometry::Poly, rigid_contact};
use std::ops::Range;
#[path="ice.rs"]
mod ice;

#[derive(Clone)]
struct Cell {
    // Material-space tint survives fracture and rigid movement.
    mineral_color: Option<V>,
    material_name: &'static str,
    poly: Poly,
    melted: bool,
    enthalpy: f32,
    range: Range<usize>,
    model: Vec<V>,
    body: rigid_contact::Body,
    anchored: bool,
    joints: Vec<Option<usize>>,
    crown_faces: Vec<[usize;3]>,
}
#[derive(Clone)]
struct Interface {
    cells: [usize; 2],
    pairs: Vec<[usize; 2]>,
    area: f32,
    normal: V,
    lambda: Vec<V>,
    damage: f32,
    work: f32,
    peak_elastic_energy: f32,
    peak_stress: f32,
    broken: bool,
    contact_ready: bool,
    phase_removed:bool,
}
#[derive(Clone)]
struct WoodPiece {
    vertices: Vec<usize>,
    model: Vec<V>,
    body: rigid_contact::Body,
    anchored: bool,
}
#[derive(Clone)]
pub(super) struct Solid {
    cells: Vec<Cell>,
    joints: Vec<Interface>,
    pub p: Vec<V>,
    velocity: Vec<V>,
    weights: Vec<f32>,
    kind: SolidKind,
    grain: V,
    bark: V,
    source: &'static str,
    volume: f64,
    pub(super) dirty: bool,
    // OGC needs a finite clearance before a newly opened, formerly coincident
    // interface can use its contact barrier. Exclusions retire after opening.
    opening: Vec<[usize; 2]>,
    labels: Vec<usize>,
    peak_pressure: f32,
    tool_work: f32,
    initial_bonds: usize,
    wood_pieces: Vec<WoodPiece>,
    crack_links: Vec<Vec<usize>>,
    crown: Option<(usize,Vec<[f32;6]>)>,
    crown_roots: Vec<usize>,
    // Only the player's prepared cut band uses compliant cross-grain bonds.
    // The rest of the longitudinal fibres retain their stiff bundle model.
    felling_band: Option<[f32; 2]>,
}
fn centroid(p: &[V]) -> V {
    scale(p.iter().copied().fold([0.; 3], add), 1. / p.len() as f32)
}
fn polygon_normal(p: &[V]) -> V {
    unit((1..p.len() - 1).fold([0.; 3], |sum, k| {
        add(sum, cross(sub(p[k], p[0]), sub(p[k + 1], p[0])))
    }))
}
impl Solid {
    pub fn new(case: &str) -> Result<Self, String> {
        Self::from_specimen(solid_geometry::specimen(case)?,case)
    }
    pub(super) fn from_specimen(shape:solid_geometry::Specimen,case:&str)->Result<Self,String>{
        let mut s = Self {
            cells: vec![],
            joints: vec![],
            p: vec![],
            velocity: vec![],
            weights: vec![],
            kind: shape.kind,
            grain: shape.grain,
            bark: shape.bark,
            source: shape.source,
            volume: shape.volume,
            dirty: true,
            opening: vec![],
            labels: vec![],
            peak_pressure: 0.,
            tool_work: 0.,
            initial_bonds: 0,
            wood_pieces: vec![],
            crack_links: vec![],
            crown: None,
            crown_roots: Vec::new(),
            felling_band: None,
        };
        let mut faces = BTreeMap::<u64, Vec<(usize, usize)>>::new();
        for poly in shape.cells {
            let id = s.cells.len();
            let measure = poly.measure(true);
            let density = s.kind.density();
            let model: Vec<V> = poly
                .vertices
                .iter()
                .map(|v| v.material.map(|x| x as f32))
                .collect();
            // This is a cut log resting on the contact table. Pinning every
            // bottom fibre made both halves remain immovable after splitting.
            let anchored = (case == "matter_layers" && measure.center[1] < 0.07) || (case=="game_tree" && model.iter().any(|p|p[1]<0.02)) || (case=="game_stone" && model.iter().any(|p|p[1]<0.005));
            let start = s.p.len();
            s.p.extend(&model);
            s.velocity.extend(vec![[0.; 3]; model.len()]);
            s.weights.extend(poly.mass_weights().iter().map(|m| {
                if anchored {
                    0.
                } else {
                    1. / (*m * density) as f32
                }
            }));
            for (i, f) in poly.faces.iter().enumerate() {
                if f.tag >= INTERIOR {
                    faces.entry(f.tag).or_default().push((id, i));
                }
            }
            let joints = vec![None; poly.faces.len()];
            let body = rigid_contact::Body {
                mass: measure.volume * density,
                center: measure.center,
                inertia: measure.inertia.map(|r| r.map(|v| v * density)),
                max_force: 1e12,
                max_torque: 1e12,
            };
            s.cells.push(Cell {
                mineral_color:None,
                material_name:"stone",
                melted:false, enthalpy:-5.*2106.,
                poly,
                range: start..s.p.len(),
                model,
                body,
                anchored,
                joints,
                crown_faces:vec![],
            });
        }
        if s.cells.is_empty() || s.cells.len() > 96 || s.p.len() > 7000 {
            return Err("solid specimen exceeds bounded material resolution".into());
        }
        for entries in faces.values() {
            if entries.len() != 2 {
                return Err("solid partition has an unmatched internal face".into());
            }
            let [(a, fa), (b, fb)] = [entries[0], entries[1]];
            let ca = &s.cells[a];
            let cb = &s.cells[b];
            let face = &ca.poly.faces[fa];
            let mut pairs = Vec::new();
            for &i in &face.nodes {
                let nearest = cb.poly.faces[fb]
                    .nodes
                    .iter()
                    .copied()
                    .min_by(|&x, &y| {
                        length(sub(ca.model[i], cb.model[x]))
                            .total_cmp(&length(sub(ca.model[i], cb.model[y])))
                    })
                    .unwrap();
                if length(sub(ca.model[i], cb.model[nearest])) > 1e-5 {
                    return Err("solid interface coordinates do not match".into());
                }
                pairs.push([ca.range.start + i, cb.range.start + nearest]);
            }
            let p0 = ca.model[face.nodes[0]];
            let mut area = 0.;
            for k in 1..face.nodes.len() - 1 {
                area += 0.5
                    * length(cross(
                        sub(ca.model[face.nodes[k]], p0),
                        sub(ca.model[face.nodes[k + 1]], p0),
                    ));
            }
            let normal = ca.poly.face_normal(face, true).map(|v| v as f32);
            let id = s.joints.len();
            s.joints.push(Interface {
                cells: [a, b],
                lambda: vec![[0.; 3]; pairs.len()],
                pairs,
                area,
                normal,
                damage: 0.,
                work: 0.,
                peak_elastic_energy: 0.,
                peak_stress: 0.,
                broken: false,
                contact_ready: false,
                phase_removed:false,
            });
            s.cells[a].joints[fa] = Some(id);
            s.cells[b].joints[fb] = Some(id);
        }
        s.initial_bonds = s.joints.len();
        s.crack_links = vec![Vec::new(); s.joints.len()];
        // Crack fronts are linked in material space, not by the current gap.
        // Opening/rotating the first part of a split must not disconnect its
        // tip from the still intact part of that same fibre interface.
        if s.kind == SolidKind::Wood {
            for a in 0..s.joints.len() {
                for b in a + 1..s.joints.len() {
                    let ja = &s.joints[a];
                    let jb = &s.joints[b];
                    if dot(ja.normal, s.grain).abs() > 0.35
                        || dot(ja.normal, jb.normal).abs() < 0.94
                    {
                        continue;
                    }
                    let shared = ja
                        .pairs
                        .iter()
                        .filter(|p| {
                            jb.pairs
                                .iter()
                                .any(|q| length(sub(s.p[p[0]], s.p[q[0]])) < 1e-5)
                        })
                        .count();
                    if shared >= 2 {
                        s.crack_links[a].push(b);
                        s.crack_links[b].push(a);
                    }
                }
            }
        }
        s.labels = s.components();
        s.rebuild_wood_pieces();
        Ok(s)
    }
    pub(super) fn prepare_felling(&mut self, bottom: f32, top: f32) {
        self.felling_band = Some([bottom, top]);
        self.rebuild_wood_pieces();
        self.dirty = true;
    }
    fn felling_interface(&self, j: &Interface) -> bool {
        self.felling_band.is_some_and(|[bottom, top]| {
            dot(j.normal, self.grain).abs() > 0.65
                && j.pairs.iter().any(|p| {
                    let cell = &self.cells[j.cells[0]];
                    let y = cell.model[p[0] - cell.range.start][1];
                    y >= bottom && y <= top
                })
        })
    }
    fn rebuild_wood_pieces(&mut self) {
        if self.kind != SolidKind::Wood {
            return;
        }
        self.wood_pieces.clear();
        // Stiff longitudinal fibres can separate through their weaker matrix.
        // A single rigid connected log prevented partial cracks from opening.
        let mut fibres: Vec<_> = (0..self.cells.len()).collect();
        fn root(p: &[usize], mut i: usize) -> usize {
            while p[i] != i {
                i = p[i];
            }
            i
        }
        for j in self
            .joints
            .iter()
            .filter(|j| !j.broken && dot(j.normal, self.grain).abs() > 0.65
                && !self.felling_interface(j))
        {
            let a = root(&fibres, j.cells[0]);
            let b = root(&fibres, j.cells[1]);
            fibres[b] = a;
        }
        // The unresolved upper tree is one intact cross-section, supported by
        // ALL fibres reaching that section. Attaching its entire mass to one
        // arbitrary strand let it tip when only that strand was nicked.
        if let Some(&first)=self.crown_roots.first() {
            for &i in &self.crown_roots {let a=root(&fibres,first);let b=root(&fibres,i);fibres[b]=a;}
        }
        for i in 0..fibres.len() {
            fibres[i] = root(&fibres, i);
        }
        let labels: std::collections::BTreeSet<_> = fibres.iter().copied().collect();
        for label in labels {
            let cells: Vec<_> = self
                .cells
                .iter()
                .enumerate()
                .filter(|(i, _)| fibres[*i] == label)
                .map(|(_, c)| c)
                .collect();
            let mass = cells.iter().filter(|c|!c.melted).map(|c| c.body.mass).sum::<f64>();
            let center: [f64; 3] = std::array::from_fn(|k| {
                cells
                    .iter()
                    .map(|c| c.body.mass * c.body.center[k])
                    .sum::<f64>()
                    / mass
            });
            let mut inertia = [[0.; 3]; 3];
            for c in &cells {
                let delta: [f64; 3] = std::array::from_fn(|k| c.body.center[k] - center[k]);
                let r2 = delta.iter().map(|x| x * x).sum::<f64>();
                for i in 0..3 {
                    for j in 0..3 {
                        inertia[i][j] += c.body.inertia[i][j]
                            + c.body.mass * (if i == j { r2 } else { 0. } - delta[i] * delta[j]);
                    }
                }
            }
            self.wood_pieces.push(WoodPiece {
                vertices: cells.iter().flat_map(|c| c.range.clone()).collect(),
                model: cells.iter().flat_map(|c| c.model.iter().copied()).collect(),
                anchored: cells.iter().any(|c| c.anchored),
                body: rigid_contact::Body {
                    mass,
                    center,
                    inertia,
                    max_force: 1e12,
                    max_torque: 1e12,
                },
            });
        }
    }
    /// Attach retained upper trunk/branches to the upper fibre bundle. These
    /// are rigidly supported wood, with their own closed contact shapes and
    /// mass/inertia. Leaves follow the same accepted transform.
    pub(super) fn attach_crown(&mut self,polys:Vec<Poly>,visual:Vec<[f32;6]>) {
        let top=self.cells.iter().flat_map(|c|&c.model).map(|p|p[1]).fold(f32::NEG_INFINITY,f32::max);
        self.crown_roots=self.cells.iter().enumerate().filter(|(_,c)|c.model.iter().any(|p|p[1]>top-0.001)).map(|(i,_)|i).collect();
        let id=(0..self.cells.len()).max_by(|&a,&b|centroid(&self.cells[a].model)[1].total_cmp(&centroid(&self.cells[b].model)[1])).unwrap();
        let old:Vec<_>=self.cells.iter().map(|c|c.range.clone()).collect();
        let c=&mut self.cells[id];
        for poly in polys {
            let m=poly.measure(true);if m.volume<=1e-10{continue;}
            let density=self.kind.density();let mass=m.volume*density;
            let total=c.body.mass+mass;
            let center=std::array::from_fn(|k|(c.body.center[k]*c.body.mass+m.center[k]*mass)/total);
            let mut inertia=[[0.;3];3];
            for (weight,p,tensor) in [(c.body.mass,c.body.center,c.body.inertia),(mass,m.center,m.inertia.map(|r|r.map(|v|v*density)))] {
                let delta: [f64;3]=std::array::from_fn(|k|p[k]-center[k]);let r2=delta.iter().map(|v|v*v).sum::<f64>();
                for a in 0..3{for b in 0..3{inertia[a][b]+=tensor[a][b]+weight*((if a==b{r2}else{0.})-delta[a]*delta[b]);}}
            }
            c.body.mass=total;c.body.center=center;c.body.inertia=inertia;self.volume+=m.volume;
            let base=c.model.len();c.model.extend(poly.vertices.iter().map(|v|v.material.map(|x|x as f32)));
            for face in poly.faces{for k in 1..face.nodes.len()-1{c.crown_faces.push([face.nodes[0]+base,face.nodes[k]+base,face.nodes[k+1]+base]);}}
        }
        self.p.clear();self.velocity.clear();self.weights.clear();
        for c in &mut self.cells{
            let start=self.p.len();self.p.extend(&c.model);c.range=start..self.p.len();
            self.velocity.extend(vec![[0.;3];c.model.len()]);
            self.weights.extend(vec![if c.anchored{0.}else{c.model.len()as f32/c.body.mass as f32};c.model.len()]);
        }
        for j in &mut self.joints {for pair in &mut j.pairs {for k in 0..2{let cell=j.cells[k];pair[k]=pair[k]-old[cell].start+self.cells[cell].range.start;}}}
        self.crown=Some((id,visual));self.rebuild_wood_pieces();self.dirty=true;
    }
    pub fn crosscut_height(&self) -> f32 {
        // The workshop has prepared axial sections, not arbitrary remeshing.
        // Aim through an available fibre section instead of missing it by
        // several centimetres with a hard-coded world-space stroke height.
        self.joints
            .iter()
            .filter(|j| dot(j.normal, self.grain).abs() > 0.65)
            .map(|j| j.pairs.iter().map(|p| self.p[p[0]][1]).sum::<f32>() / j.pairs.len() as f32)
            .min_by(|a, b| (a - 0.4).abs().total_cmp(&(b - 0.4).abs()))
            .unwrap_or(0.4)
    }
    pub fn splitting_target(&self) -> (V, f32) {
        // Aim the workshop's axe at an actual longitudinal interface on the
        // end grain. The material is still intact; the swing must supply work
        // and the wedge must physically open its faces.
        let top = self.p.iter().map(|p| p[1]).fold(0f32, f32::max);
        self.joints
            .iter()
            .filter(|j| dot(j.normal, self.grain).abs() < 0.20)
            .filter(|j| j.pairs.iter().any(|p| self.p[p[0]][1] > top - 0.04))
            .min_by(|a, b| {
                let score = |j: &Interface| {
                    let c = centroid(&j.pairs.iter().map(|p| self.p[p[0]]).collect::<Vec<_>>());
                    c[0] * c[0] + c[2] * c[2] + 0.01 * (1. - j.normal[2].abs())
                };
                score(a).total_cmp(&score(b))
            })
            .map(|j| {
                // Aim at the seam's end-grain intersection, not its centroid
                // lower inside the tilted trunk.
                let points: Vec<_> = j
                    .pairs
                    .iter()
                    .map(|p| self.p[p[0]])
                    .filter(|p| p[1] > top - 0.04)
                    .collect();
                let c = centroid(&points);
                ([c[0], 0., c[2]], j.normal[0].atan2(j.normal[2]))
            })
            .unwrap_or(([0.; 3], 0.))
    }
    pub(super) fn distance(&self, p: V) -> f32 {
        self.cells
            .iter()
            .enumerate()
            .flat_map(|(i, c)| {
                c.poly
                    .faces
                    .iter()
                    .enumerate()
                    .filter(move |(f, _)| self.exposed(i, *f))
                    .flat_map(move |(_, f)| {
                        (1..f.nodes.len() - 1).map(move |k| {
                            tools::point_triangle(
                                p,
                                [f.nodes[0], f.nodes[k], f.nodes[k + 1]]
                                    .map(|v| self.p[c.range.start + v]),
                            )
                        })
                    })
            })
            .fold(f32::INFINITY, f32::min)
    }
    fn components(&self) -> Vec<usize> {
        let mut parent: Vec<_> = (0..self.cells.len()).collect();
        fn root(p: &[usize], mut i: usize) -> usize {
            while p[i] != i {
                i = p[i];
            }
            i
        }
        for b in self.joints.iter().filter(|b| !b.broken) {
            let a = root(&parent, b.cells[0]);
            let c = root(&parent, b.cells[1]);
            parent[c] = a;
        }
        if let Some(&first)=self.crown_roots.first() {for &i in &self.crown_roots {let a=root(&parent,first);let b=root(&parent,i);parent[b]=a;}}
        (0..parent.len()).map(|i| root(&parent, i)).collect()
    }
    /// Only a severed crown that has physically tipped near horizontal is craftable.
    pub(super) fn fallen_crown(&self) -> Option<V> {
        let id=self.crown.as_ref()?.0;
        let labels=self.components();
        if self.cells.iter().enumerate().any(|(i,c)|c.anchored&&labels[i]==labels[id]) {return None;}
        let c=&self.cells[id];
        let (_,axes)=crate::physics::rigid_fit::fit(&c.model,&self.p[c.range.clone()])?;
        if axes[1][1].abs()>0.4 {return None;}
        Some(centroid(&self.p[c.range.clone()]))
    }
    fn exposed(&self, cell: usize, face: usize) -> bool {
        if self.cells[cell].melted {return false;}
        self.cells[cell].joints[face].is_none_or(|j| self.joints[j].broken)
    }
    pub fn positions(&self, tool: &Tool) -> Vec<V> {
        let mut p = self.p.clone();
        p.extend(&tool.p);
        p.extend(&tool.ground);
        p
    }
    pub(super) fn contact_surface(&self, start: usize) -> Value {
        let n = self.p.len();
        let mut faces = Vec::new();
        let mut active = vec![false; n];
        let mut elements = vec![0u32; n];
        for (id, c) in self.cells.iter().enumerate() {
            for k in c.range.clone() {
                elements[k] = id as u32;
            }
            for face in &c.crown_faces {for &v in face{active[c.range.start+v]=true;}faces.push(face.map(|v|(c.range.start+v)as u32));}
            for (f, face) in c.poly.faces.iter().enumerate() {
                if !self.exposed(id, f) {
                    continue;
                }
                for &v in &face.nodes {
                    active[c.range.start + v] = true;
                }
                for k in 1..face.nodes.len() - 1 {
                    faces.push(
                        [face.nodes[0], face.nodes[k], face.nodes[k + 1]]
                            .map(|i| (c.range.start + i) as u32),
                    );
                }
            }
        }
        let mut excluded: Vec<[u32; 2]> =
            self.opening.iter().map(|p| p.map(|i| i as u32)).collect();
        for a in 0..self.cells.len() {
            for b in a + 1..self.cells.len() {
                if self.labels[a] == self.labels[b] {
                    excluded.push([a as u32, b as u32]);
                }
            }
        }
        // An intact connected specimen has no eligible self-contact pairs.
        // Express that directly so OGC can use its cached tool/table path.
        let self_contact = self.labels.iter().any(|id| Some(id) != self.labels.first());
        json!({"start":start,"count":n,"triangles":faces,"motion":0,"self_collision":self_contact,"friction":0.72,"active_vertices":active,"element_ids":elements,"excluded_pairs":excluded,"movable_vertices":self.weights.iter().map(|w|*w>0.).collect::<Vec<_>>()})
    }
    pub fn configure(&mut self, host: &mut Host, tool: &Tool) -> Result<(), String> {
        let n = self.p.len();
        host.invoke("OGCConnectionSystem.Configure",json!({"positions":self.positions(tool),"contact_radius":0.0012,"contact_stiffness":45000.,"surfaces":[
            self.contact_surface(0), coupling::tool_surface(tool,n), coupling::table_surface(tool,n+tool.p.len())]}))?;
        self.dirty = false;
        Ok(())
    }
    fn separation(&self, a: usize, b: usize) -> f32 {
        let mut best = f32::NEG_INFINITY;
        for c in [&self.cells[a], &self.cells[b]] {
            for face in &c.poly.faces {
                let n = polygon_normal(
                    &face
                        .nodes
                        .iter()
                        .map(|&k| self.p[c.range.start + k])
                        .collect::<Vec<_>>(),
                );
                let interval = |cell: &Cell| {
                    let mut lo = f32::INFINITY;
                    let mut hi = f32::NEG_INFINITY;
                    for p in &self.p[cell.range.clone()] {
                        let t = dot(*p, n);
                        lo = lo.min(t);
                        hi = hi.max(t);
                    }
                    (lo, hi)
                };
                let (al, ah) = interval(&self.cells[a]);
                let (bl, bh) = interval(&self.cells[b]);
                best = best.max((bl - ah).max(al - bh));
            }
        }
        best
    }
    pub(super) fn refresh_interfaces(&mut self) {
        if self.dirty {
            self.rebuild_wood_pieces();
        }
        for i in 0..self.joints.len() {
            let j = &self.joints[i];
            if j.broken && !j.contact_ready && self.separation(j.cells[0], j.cells[1]) >= 0.003 {
                self.joints[i].contact_ready = true;
            }
        }
        let next = self.components();
        if next != self.labels {
            for a in 0..self.cells.len() {
                for b in a + 1..self.cells.len() {
                    if self.labels[a] == self.labels[b]
                        && next[a] != next[b]
                        && self.separation(a, b) < 0.003
                    {
                        self.opening.push([a, b]);
                    }
                }
            }
            self.labels = next;
            self.rebuild_wood_pieces();
            self.dirty = true;
        }
        let old = std::mem::take(&mut self.opening);
        for p in old {
            if self.separation(p[0], p[1]) < 0.003 {
                self.opening.push(p);
            } else {
                self.dirty = true;
            }
        }
    }
    fn joint_properties(&self, j: &Interface) -> (f32, f32, f32) {
        let across = dot(j.normal, self.grain).abs();
        if self.kind == SolidKind::Ice { return (900_000_000.,650_000.,4.5); }
        if self.kind == SolidKind::Wood {
            let center = centroid(
                &j.pairs
                    .iter()
                    .map(|p| {
                        let cell = &self.cells[j.cells[0]];
                        cell.model[p[0] - cell.range.start]
                    })
                    .collect::<Vec<_>>(),
            );
            let radial = unit(sub(center, scale(self.grain, dot(center, self.grain))));
            // Growth-ring direction changes cleavage resistance. Rays favor
            // radial splitting; transverse fibres retain their high cutting
            // cost. These workshop values are not a measured wood species.
            let ring_factor = 0.75 + 0.50 * dot(j.normal, radial).abs();
            // Opening parallel to the fibres cuts them; longitudinal splitting
            // breaks the much weaker cross-fibre matrix.
            (
                600_000_000. + 9_400_000_000. * across.powi(2),
                180_000. * ring_factor + 1_800_000. * across.powi(2),
                75. * ring_factor + 900. * across.powi(2),
            )
        } else {
            (800_000_000., 1_600_000., 65.)
        }
    }
    fn solve_interfaces(&mut self, q: &mut [V], dt: f32) {
        for id in 0..self.joints.len() {
            if self.joints[id].broken {
                let j = &self.joints[id];
                if !j.contact_ready {
                    // Coincident newborn crack faces cannot initialize an OGC
                    // barrier yet. Keep unilateral normal support until they
                    // open: fracture removes adhesion, never occupied volume.
                    let normal =
                        polygon_normal(&j.pairs.iter().map(|p| q[p[0]]).collect::<Vec<_>>());
                    for &[a, b] in &j.pairs {
                        let gap = dot(sub(q[b], q[a]), normal);
                        let weight = self.weights[a] + self.weights[b];
                        if gap < 0. && weight > 0. {
                            let correction = scale(normal, -gap / weight);
                            q[a] = sub(q[a], scale(correction, self.weights[a]));
                            q[b] = add(q[b], scale(correction, self.weights[b]));
                        }
                    }
                }
                continue;
            }
            // Continuous fibres are solved as stiff bundles. The weaker
            // matrix between bundles retains a brittle cohesive interface.
            if self.kind == SolidKind::Wood && dot(self.joints[id].normal, self.grain).abs() > 0.65
                && !self.felling_interface(&self.joints[id])
            {
                continue;
            }
            let felling = self.felling_interface(&self.joints[id]);
            let (young, strength, _) = self.joint_properties(&self.joints[id]);
            let j = &mut self.joints[id];
            let thickness = length(sub(
                self.cells[j.cells[0]].body.center.map(|v| v as f32),
                self.cells[j.cells[1]].body.center.map(|v| v as f32),
            ))
            .max(0.01);
            let intact_alpha =
                thickness / (young * j.area / j.pairs.len() as f32).max(1.) / (dt * dt);
            let normal = polygon_normal(&j.pairs.iter().map(|p| q[p[0]]).collect::<Vec<_>>());
            let mut traction = [0.; 3];
            let mut energy = 0.;
            let mut bending_stress = 0f32;
            for (pair, lambda) in j.pairs.iter().zip(&mut j.lambda) {
                let [a, b] = *pair;
                let error = sub(q[b], q[a]);
                // A growing crack loses tensile/shear stiffness, allowing the
                // wedge to open it. Compression still carries load. Previously
                // partial damage had no mechanical effect until instant failure.
                let alpha = if (self.kind != SolidKind::Wood || self.felling_band.is_some()) && dot(error, normal) >= 0. {
                    intact_alpha / (1. - j.damage).powi(2).max(0.01)
                } else {
                    intact_alpha
                };
                let w = self.weights[a] + self.weights[b];
                if w == 0. {
                    continue;
                }
                let dl = scale(add(error, scale(*lambda, alpha)), -1. / (w + alpha));
                *lambda = add(*lambda, dl);
                q[a] = sub(q[a], scale(dl, self.weights[a]));
                q[b] = add(q[b], scale(dl, self.weights[b]));
                let force = scale(*lambda, -1. / (dt * dt));
                traction = add(traction, force);
                energy += (0.5 * dot(force, error)).max(0.);
                if felling {
                    // Bending puts opposite edges in tension/compression.
                    // Summing their forces alone cancels the bending load and
                    // leaves a weakened hinge unable to fail under crown weight.
                    let tension = dot(force, normal);
                    let shear = length(sub(force, scale(normal, tension)));
                    let area = j.area / j.pairs.len() as f32;
                    bending_stress = bending_stress.max(
                        (tension.max(0.).powi(2) + (0.65 * shear).powi(2)).sqrt()
                            / area.max(1e-8));
                }
            }
            let normal_load = dot(traction, normal);
            let shear = length(sub(traction, scale(normal, normal_load)));
            // Pure compression carries weight; tension/shear drive cracks.
            let stress =
                ((normal_load.max(0.).powi(2) + (0.65 * shear).powi(2)).sqrt() / j.area.max(1e-8))
                    .max(bending_stress);
            j.peak_stress = j.peak_stress.max(stress);
            if stress > strength * (1. - j.damage).max(0.05) && energy > j.peak_elastic_energy {
                j.work += energy - j.peak_elastic_energy;
                j.peak_elastic_energy = energy;
            }
        }
    }
    fn contact_work(
        &mut self,
        tool: &Tool,
        old: &[V],
        responses: &[[f32; 13]],
        impact_energy: f32,
    ) {
        let cutting_plane = tool.cutting_normal();
        let total_load = responses
            .iter()
            .map(|r| length([r[0], r[1], r[2]]))
            .sum::<f32>();
        for (k, r) in responses.iter().enumerate() {
            let force = [r[0], r[1], r[2]];
            let load = length(force);
            if load < 0.05 {
                continue;
            }
            let mut nearest = (f32::INFINITY, 0usize, [[0.; 3]; 3]);
            for (ci, c) in self.cells.iter().enumerate() {
                for (fi, face) in c.poly.faces.iter().enumerate() {
                    if !self.exposed(ci, fi) {
                        continue;
                    }
                    for f in 1..face.nodes.len() - 1 {
                        let triangle = [face.nodes[0], face.nodes[f], face.nodes[f + 1]]
                            .map(|v| self.p[c.range.start + v]);
                        let d = tools::point_triangle(tool.p[k], triangle);
                        if d < nearest.0 {
                            nearest = (d, ci, triangle);
                        }
                    }
                }
            }
            if nearest.0 > 0.012 {
                continue;
            }
            let pressure = load / tool.projected_area(k, force, nearest.2, 0.0024).max(1e-7);
            self.peak_pressure = self.peak_pressure.max(pressure);
            // Do not double count force-distance work and the same impact's
            // kinetic loss. Allocate one head's finite energy across contacts.
            let work = (-dot(force, sub(tool.p[k], old[k])))
                .max(0.)
                .max(impact_energy * load / total_load.max(1e-8));
            self.tool_work += work;
            if work <= 0. {
                continue;
            }
            let mut candidates = Vec::new();
            let mut sum = 0.;
            let mut frontier = vec![false; self.joints.len()];
            let mut reached = vec![false; self.joints.len()];
            let mut queue: Vec<_> = self
                .joints
                .iter()
                .enumerate()
                .filter(|(_, j)| j.broken && j.cells.contains(&nearest.1))
                .map(|(i, _)| i)
                .collect();
            while let Some(i) = queue.pop() {
                if reached[i] {
                    continue;
                }
                reached[i] = true;
                for &next in &self.crack_links[i] {
                    if self.joints[next].broken {
                        queue.push(next);
                    } else {
                        frontier[next] = true;
                    }
                }
            }
            let contact_grain = crate::physics::rigid_fit::fit(
                &self.cells[nearest.1].model,
                &self.p[self.cells[nearest.1].range.clone()],
            )
            .map(|(_, axes)| {
                std::array::from_fn(|i| (0..3).map(|j| axes[j][i] as f32 * self.grain[j]).sum())
            })
            .unwrap_or(self.grain);
            for (i, j) in self.joints.iter().enumerate().filter(|(_, j)| !j.broken) {
                let adjacent = j.cells.contains(&nearest.1);
                let across = dot(j.normal, self.grain).abs();
                let center = scale(
                    j.pairs.iter().map(|p| self.p[p[0]]).fold([0.; 3], add),
                    1. / j.pairs.len() as f32,
                );
                let world_normal =
                    polygon_normal(&j.pairs.iter().map(|p| self.p[p[0]]).collect::<Vec<_>>());
                // A split propagates down the same grain plane from an
                // existing crack. Contact work is shared, never duplicated
                // along the front; remote intact planes receive no damage.
                let front = self.kind == SolidKind::Wood && across < 0.35 && frontier[i];
                if !adjacent && !front {
                    continue;
                }
                let (_, strength, toughness) = self.joint_properties(j);
                let alignment = if self.kind == SolidKind::Wood {
                    dot(world_normal, cutting_plane).abs()
                } else {
                    1.
                };
                if self.kind == SolidKind::Wood && alignment < 0.45 {
                    continue;
                }
                // A localized compressive contact drives splitting tension.
                // The same load spread over a bucket's area stays subcritical.
                if pressure * 0.32 <= strength * (1. - j.damage).max(0.05) {
                    continue;
                }
                if self.kind == SolidKind::Wood && across > 0.65 {
                    // Cross-grain fibres must meet the cutting edge. A wedge
                    // pressing elsewhere cannot remotely chop the whole log.
                    let edge_distance = (1..j.pairs.len() - 1)
                        .map(|f| {
                            tools::point_triangle(
                                tool.p[k],
                                [0, f, f + 1].map(|v| self.p[j.pairs[v][0]]),
                            )
                        })
                        .fold(f32::INFINITY, f32::min);
                    if !tool.is_cutting_vertex(k) || edge_distance > 0.030 {
                        continue;
                    }
                }
                // Fracture localizes into an existing weak crack instead of
                // distributing equal damage through an entire neighbourhood.
                // Resistance is the remaining material interface's toughness
                // and area; no tool name or predetermined split is involved.
                let resistance = (toughness * j.area).max(1e-6);
                let localization = (1. + 4. * (j.work / resistance).min(1.)).powi(2);
                let delta = sub(center, tool.p[k]);
                let distance = if front {
                    // Stress travels farther along fibres than across them.
                    let axial = dot(delta, contact_grain);
                    length(sub(delta, scale(contact_grain, axial))) + 0.18 * axial.abs()
                } else {
                    length(delta)
                };
                let weight =
                    localization * alignment.powi(6) / ((0.015 + distance).powi(2) * resistance);
                candidates.push((i, weight));
                sum += weight;
            }
            if self.kind == SolidKind::Wood {
                // Follow the most strongly loaded admissible crack first.
                // Smearing one blow over every neighbour only decorated the
                // log with many partial cracks. Each face consumes its own
                // remaining fracture energy before any excess can continue.
                candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                let mut remaining = work;
                for (i, _) in candidates {
                    let (_, _, toughness) = self.joint_properties(&self.joints[i]);
                    let need = (toughness * self.joints[i].area - self.joints[i].work).max(0.);
                    let share = remaining.min(need);
                    self.joints[i].work += share;
                    remaining -= share;
                    if remaining <= 1e-7 {
                        break;
                    }
                }
            } else {
                for (i, weight) in candidates {
                    self.joints[i].work += work * weight / sum.max(1e-8);
                }
            }
        }
        for i in 0..self.joints.len() {
            let (_, strength, toughness) = self.joint_properties(&self.joints[i]);
            let normal = polygon_normal(
                &self.joints[i]
                    .pairs
                    .iter()
                    .map(|p| self.p[p[0]])
                    .collect::<Vec<_>>(),
            );
            let opening = self.joints[i]
                .pairs
                .iter()
                .map(|&[a, b]| {
                    let delta = sub(self.p[b], self.p[a]);
                    let n = dot(delta, normal);
                    let shear = sub(delta, scale(normal, n));
                    (n.max(0.).powi(2) + 0.45 * dot(shear, shear)).sqrt()
                })
                .fold(0f32, f32::max);
            let j = &mut self.joints[i];
            if j.broken {
                continue;
            }
            // Critical opening follows cohesive energy/strength. A loaded
            // interface fails before visible stretch; solver compliance is
            // not interpreted as unlimited elastic elongation of wood.
            if self.kind == SolidKind::Wood
                && opening > 2. * toughness / strength
                && (j.work > 0. || j.peak_stress > strength)
            {
                j.work = j.work.max(toughness * j.area);
            }
            j.damage = (j.work / (toughness * j.area).max(1e-6)).min(1.);
            if j.damage >= 1. {
                j.broken = true;
                self.dirty = true;
            }
        }
    }
    pub(super) fn begin_step(&mut self, dt: f32) -> coupling::Step {
        for j in &mut self.joints {
            j.lambda.fill([0.; 3]);
            j.peak_stress = 0.;
        }
        coupling::Step::new(&self.p, &self.velocity, &self.weights, dt)
    }
    pub(super) fn material_target(&mut self, step: &mut coupling::Step, dt: f32) -> Vec<V> {
        let mut q = self.p.clone();
        self.solve_interfaces(&mut q, dt);
        step.target(&self.p, &mut q);
        q
    }
    pub(super) fn accept_contact(
        &mut self,
        q: &[V],
        response: &[[f32; 13]],
        dt: f32,
    ) -> Result<(), String> {
        if self.kind == SolidKind::Wood {
            for piece in &self.wood_pieces {
                if piece.anchored {
                    continue;
                }
                let current: Vec<_> = piece.vertices.iter().map(|&i| self.p[i]).collect();
                let target: Vec<_> = piece.vertices.iter().map(|&i| q[i]).collect();
                let response: Vec<_> = piece.vertices.iter().map(|&i| response[i]).collect();
                let result = rigid_contact::project(
                    &piece.model,
                    &current,
                    &target,
                    &response,
                    &piece.body,
                    None,
                    dt as f64,
                )?;
                for (&i, &p) in piece.vertices.iter().zip(&result.positions) {
                    self.p[i] = p;
                }
            }
        } else {
            for c in &self.cells {
                if c.anchored || c.melted {
                    continue;
                }
                let result = rigid_contact::project(
                    &c.model,
                    &self.p[c.range.clone()],
                    &q[c.range.clone()],
                    &response[c.range.clone()],
                    &c.body,
                    None,
                    dt as f64,
                )?;
                self.p[c.range.clone()].copy_from_slice(&result.positions);
            }
        }
        Ok(())
    }
    pub(super) fn finish_step(
        &mut self,
        step: &coupling::Step,
        tool: &Tool,
        tool_old: &[V],
        response: &[[f32; 13]],
        dt: f32,
        impact: f32,
    ) -> Result<(), String> {
        self.contact_work(tool, tool_old, response, impact);
        for i in 0..self.p.len() {
            self.velocity[i] = scale(sub(self.p[i], step.old[i]), (-0.7 * dt).exp() / dt);
        }
        if self
            .p
            .iter()
            .chain(&self.velocity)
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err("non-finite structural material state".into());
        }
        Ok(())
    }
    pub fn substep(
        &mut self,
        host: &mut Host,
        tool: &mut Tool,
        time: f32,
        dt: f32,
    ) -> Result<(u64, f32, [f64; 4]), String> {
        self.refresh_interfaces();
        if self.dirty {
            self.configure(host, tool)?;
        }
        let previous = self.positions(tool);
        let tool_old = tool.p.clone();
        let mut step = self.begin_step(dt);
        let goal = tool.goal(time, dt);
        let mut timings = [0.; 4];
        let mut contacts = 0;
        let mut peak = 0f32;
        let mut final_response = vec![[0.; 13]; tool.p.len()];
        for _ in 0..5 {
            let material_start = std::time::Instant::now();
            let q = self.material_target(&mut step, dt);
            timings[0] += material_start.elapsed().as_secs_f64() * 1000.;
            let started = std::time::Instant::now();
            let batch = host.contact_responses(&self.positions(tool), &previous)?;
            timings[1] += started.elapsed().as_secs_f64() * 1000.;
            timings[2] += batch.broad_ms;
            timings[3] += batch.narrow_ms;
            contacts = contacts.max(batch.contacts);
            self.accept_contact(&q, &batch.responses[..self.p.len()], dt)?;
            let n = self.p.len();
            final_response.copy_from_slice(&batch.responses[n..n + tool.p.len()]);
            peak = peak.max(
                final_response
                    .iter()
                    .map(|r| length([r[0], r[1], r[2]]))
                    .sum(),
            );
            tool.project(&goal, &final_response, dt)?;
        }
        self.finish_step(
            &step,
            tool,
            &tool_old,
            &final_response,
            dt,
            tool.impact_energy(&tool_old, dt),
        )?;
        tool.record_contact(&tool_old, &final_response, dt);
        tool.finish(&tool_old, dt);
        Ok((contacts, peak, timings))
    }
    pub(super) fn shear(&mut self,slope:[f32;2])->Result<(),String>{
        let anchored:Vec<_>=self.cells.iter().map(|c|c.anchored).collect();
        let mut cells:Vec<_>=self.cells.iter().map(|c|c.poly.clone()).collect();
        for p in cells.iter_mut().flat_map(|p|&mut p.vertices){p.position[1]+=slope[0]as f64*p.position[0]+slope[1]as f64*p.position[2];p.material=p.position;}
        *self=Self::from_specimen(solid_geometry::Specimen{cells,kind:self.kind,grain:self.grain,bark:self.bark,source:self.source,volume:self.volume},"matter_layers")?;for (c,anchored) in self.cells.iter_mut().zip(anchored){c.anchored=anchored;for i in c.range.clone(){self.weights[i]=if anchored{0.}else{c.model.len()as f32/c.body.mass as f32};}}Ok(())
    }
    pub(super) fn moving(&self)->bool {self.velocity.iter().any(|v|length(*v)>0.08)}
    pub fn lifted_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| {
                !c.anchored && !c.melted && centroid(&self.p[c.range.clone()])[1] - centroid(&c.model)[1] > 0.05
            })
            .count()
    }
    pub fn report(&self) -> Value {
        let components = self.components();
        let count = components
            .iter()
            .enumerate().filter(|(i,_)|!self.cells[*i].melted).map(|(_,id)|*id)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let mass = self.cells.iter().filter(|c|!c.melted).map(|c| c.body.mass).sum::<f64>();
        let lifted = self
            .cells
            .iter()
            .filter(|c| {
                !c.anchored && !c.melted && centroid(&self.p[c.range.clone()])[1] - centroid(&c.model)[1] > 0.05
            })
            .count();
        let mut volume = 0.;
        for c in self.cells.iter().filter(|c|!c.melted) {
            let mut p = c.poly.clone();
            for (v, now) in p.vertices.iter_mut().zip(&self.p[c.range.clone()]) {
                v.position = now.map(|x| x as f64);
            }
            volume += p.measure(false).volume;
        }
        let mut minerals=std::collections::BTreeMap::<&str,f64>::new();
        for c in self.cells.iter().filter(|c|!c.melted && c.mineral_color.is_some() && c.material_name!="stone") {
            *minerals.entry(c.material_name).or_default()+=c.body.mass;
        }
        json!({"mineral_mass_kg":minerals,"source":self.source,"cells":self.cells.len(),"interfaces":self.initial_bonds,"broken_interfaces":self.joints.iter().filter(|j|j.broken&&!j.phase_removed).count(),
            "components":count,"lifted_cells":lifted,"mass_kg":mass,"initial_volume_m3":self.volume,"melted_cells":self.cells.iter().filter(|c|c.melted).count(),"current_volume_m3":volume,
            "peak_contact_pressure_pa":self.peak_pressure,"tool_contact_work_j":self.tool_work,"opening_contact_pairs":self.opening.len(),
            "maximum_interface_damage":self.joints.iter().map(|j|j.damage).fold(0f32, f32::max),
            "fracture_work_j":self.joints.iter().map(|j|j.work as f64).sum::<f64>(),
            "maximum_vertex_speed_m_s":self.velocity.iter().map(|v|length(*v)).fold(0f32,f32::max),
            "kinetic_energy_j":self.velocity.iter().zip(&self.weights).filter(|(_,w)|**w>0.).map(|(v,w)|0.5*dot(*v,*v)/w).sum::<f32>(),
            "model":if self.kind == SolidKind::Wood { "stiff fibre bundles, brittle finite-opening matrix, grain-directed fracture and localized cross-grain cutting" } else { "shape-preserving convex cells with compliant, traction- and work-driven cohesive fracture" },
            "grain_splits":self.joints.iter().filter(|j|j.broken && dot(j.normal,self.grain).abs()<0.35).count(),
            "cross_grain_cuts":self.joints.iter().filter(|j|j.broken && dot(j.normal,self.grain).abs()>0.65).count(),
            "cross_grain_interfaces":self.joints.iter().filter(|j|dot(j.normal,self.grain).abs()>0.65).count(),
            "cross_grain_work_j":self.joints.iter().filter(|j|dot(j.normal,self.grain).abs()>0.65).map(|j|j.work).sum::<f32>(),
            "cross_grain_maximum_damage":self.joints.iter().filter(|j|dot(j.normal,self.grain).abs()>0.65).map(|j|j.damage).fold(0f32,f32::max),
            "grain_axis":self.grain,
            "fracture_resolution":"prepartitioned irregular material interfaces; no on-demand subdivision yet"})
    }
    fn wood_color(&self, p: V, tag: u64) -> V {
        if tag == 1 {
            let angle = p[2].atan2(p[0]);
            let ridges = (angle * 37. + 0.4 * (p[1] * 23.).sin()).sin().abs();
            self.bark.map(|v| v * (0.55 + 0.55 * ridges))
        } else {
            let radius = (p[0] * p[0] + p[2] * p[2]).sqrt();
            let rings = (radius * 430.).sin();
            let grain = (p[0] * 560. + p[2] * 103.).sin();
            [0.76, 0.57, 0.32].map(|v| v * (0.88 + 0.10 * rings + 0.04 * grain))
        }
    }
    fn wood_triangle(&self, out: &mut Vec<[f32; 6]>, p: [V; 3], rest: [V; 3], tag: u64, depth: u8) {
        let area = 0.5 * length(cross(sub(rest[1], rest[0]), sub(rest[2], rest[0])));
        if area > 0.0006 && depth < if self.source.starts_with("game "){1}else{4} {
            let edge = (0..3)
                .max_by(|&a, &b| {
                    length(sub(rest[a], rest[(a + 1) % 3]))
                        .total_cmp(&length(sub(rest[b], rest[(b + 1) % 3])))
                })
                .unwrap();
            let [a, b, c] = [edge, (edge + 1) % 3, (edge + 2) % 3];
            let middle = scale(add(p[a], p[b]), 0.5);
            let material = scale(add(rest[a], rest[b]), 0.5);
            self.wood_triangle(
                out,
                [p[a], middle, p[c]],
                [rest[a], material, rest[c]],
                tag,
                depth + 1,
            );
            self.wood_triangle(
                out,
                [middle, p[b], p[c]],
                [material, rest[b], rest[c]],
                tag,
                depth + 1,
            );
            return;
        }
        let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        if length(n) < 0.5 {
            return;
        }
        let light = 0.46 + 0.54 * dot(n, unit([-0.4, 0.8, 0.55])).abs();
        for (p, material) in p.into_iter().zip(rest) {
            let color = self.wood_color(material, tag);
            out.push([
                p[0],
                p[1],
                p[2],
                color[0] * light,
                color[1] * light,
                color[2] * light,
            ]);
        }
    }
    pub(super) fn collision(&self)->Vec<[V;3]> {
        let mut out=Vec::new();for (i,c) in self.cells.iter().enumerate(){
            for (j,f) in c.poly.faces.iter().enumerate(){if !self.exposed(i,j){continue;}for k in 1..f.nodes.len()-1{out.push([f.nodes[0],f.nodes[k],f.nodes[k+1]].map(|v|self.p[c.range.start+v]));}}
            for f in &c.crown_faces{out.push(f.map(|v|self.p[c.range.start+v]));}
        }out
    }
    pub(super) fn sample_stone_materials(&mut self, material:impl Fn(V)->(&'static str,V)) {
        for cell in &mut self.cells {let(name,color)=material(centroid(&cell.model));cell.material_name=name;cell.mineral_color=Some(color);}
    }
    pub fn render(&self, out: &mut Vec<[f32; 6]>) {
        if let Some((id,visual))=&self.crown {let c=&self.cells[*id];if let Some((o,axes))=crate::physics::rigid_fit::fit(&c.model,&self.p[c.range.clone()]) {for v in visual {let p:[f32;3]=std::array::from_fn(|k|(o[k]+(0..3).map(|j|axes[j][k]*v[j]as f64).sum::<f64>())as f32);out.push([p[0],p[1],p[2],v[3],v[4],v[5]]);}}}

        for (i, c) in self.cells.iter().enumerate() {
            for (f, face) in c.poly.faces.iter().enumerate() {
                if !self.exposed(i, f) {
                    continue;
                }
                let color = {
                    let variation = 0.90 + 0.10 * noise(i as i32 * 73 + f as i32);
                    (if self.kind==SolidKind::Ice { [0.58,0.82,0.91] } else if let Some(color)=c.mineral_color {color} else if face.tag >= INTERIOR {
                        [0.57, 0.59, 0.61]
                    } else {
                        world_field::STONE_COLOR
                    })
                    .map(|v| v * variation)
                };
                for k in 1..face.nodes.len() - 1 {
                    let ids = [face.nodes[0], face.nodes[k], face.nodes[k + 1]];
                    let positions = ids.map(|v| self.p[c.range.start + v]);
                    if self.kind == SolidKind::Wood {
                        self.wood_triangle(out, positions, ids.map(|v| c.model[v]), face.tag, 0);
                        continue;
                    }
                    skin::triangle(out, positions, color);
                }
            }
        }
        // A fracture can reach the surface while neighbouring fibres still
        // hold the piece in place. Show its actual seam even when the opening
        // is subpixel; otherwise broken wood looks pristine after unloading.
        for j in self.joints.iter().filter(|j| j.broken&&!j.phase_removed) {
            let c = &self.cells[j.cells[0]];
            for k in 0..j.pairs.len() {
                let a = j.pairs[k][0] - c.range.start;
                let b = j.pairs[(k + 1) % j.pairs.len()][0] - c.range.start;
                let Some(face) = c
                    .poly
                    .faces
                    .iter()
                    .find(|f| f.tag < INTERIOR && f.nodes.contains(&a) && f.nodes.contains(&b))
                else {
                    continue;
                };
                let normal = polygon_normal(
                    &face
                        .nodes
                        .iter()
                        .map(|&v| self.p[c.range.start + v])
                        .collect::<Vec<_>>(),
                );
                let offset = scale(normal, 0.0007);
                let pa = add(self.p[c.range.start + a], offset);
                let pb = add(self.p[c.range.start + b], offset);
                let side = scale(unit(cross(sub(pb, pa), normal)), 0.0008);
                let color = if self.kind == SolidKind::Wood {
                    [0.10, 0.055, 0.025]
                } else {
                    [0.10, 0.12, 0.13]
                };
                skin::triangle(out, [add(pa, side), sub(pa, side), sub(pb, side)], color);
                skin::triangle(out, [add(pa, side), sub(pb, side), add(pb, side)], color);
            }
        }
    }
}


#[cfg(test)]
mod felling_checks {
    use super::*;

    fn trunk() -> Solid {
        let shape = Poly::box_from([-0.15, 0., -0.15], [0.15, 3., 0.15]);
        let cells = solid_geometry::partition(&shape,
            &[[0., 0.5, 0.], [0., 1.5, 0.], [0., 2.5, 0.]], [1.; 3], 0).unwrap();
        let mut solid = Solid::from_specimen(solid_geometry::Specimen {
            cells, kind: SolidKind::Wood, grain: [0., 1., 0.],
            bark: [0.3, 0.2, 0.1], source: "felling regression", volume: 0.27,
        }, "game_tree").unwrap();
        solid.prepare_felling(0.9, 1.1);
        solid.attach_crown(vec![Poly::box_from([-0.15, 3., -0.15], [1.35, 4., 0.15])], vec![]);
        solid
    }

    fn free_step(s: &mut Solid) {
        let dt = 1. / 120.;
        let mut step = s.begin_step(dt);
        // No contact forces, with ample certified motion clearance. A zero
        // response bound means blocked motion in the real OGC protocol.
        let mut response = vec![[0.; 13]; s.p.len()];
        for r in &mut response { r[12] = 10.; }
        for _ in 0..5 {
            let q = s.material_target(&mut step, dt);
            s.accept_contact(&q, &response, dt).unwrap();
        }
        let tool = Tool::held("matter_wood");
        s.finish_step(&step, &tool, &tool.p, &[], dt, 0.).unwrap();
        s.refresh_interfaces();
    }

    #[test]
    fn felling_band_carries_load_without_prebreaking_wood() {
        let mut s = trunk();
        assert_eq!(s.joints.iter().filter(|j| s.felling_interface(j)).count(), 1);
        assert!(s.joints.iter().all(|j| !j.broken && j.damage == 0. && j.work == 0.));
        assert_eq!(s.wood_pieces.len(), 2);
        let rest = s.p.clone();
        for _ in 0..120 { free_step(&mut s); }
        let crown = s.crown.as_ref().unwrap().0;
        let top = s.cells[crown].range.clone();
        let drop = centroid(&rest[top.clone()])[1] - centroid(&s.p[top])[1];
        assert!(drop.abs() < 0.02, "intact support must carry crown weight: {drop}");
        assert!(s.joints.iter().any(|j| j.peak_stress > 0.));
        assert!(s.joints.iter().all(|j| !j.broken), "intact crown must not fracture under its resting weight");
        for c in s.cells.iter().filter(|c| c.anchored) {
            assert_eq!(&s.p[c.range.clone()], &rest[c.range.clone()]);
        }
    }

    #[test]
    fn crown_bending_load_survives_net_force_cancellation() {
        let mut s = trunk();
        let angle = 0.001f32;
        for c in s.cells.iter().filter(|c| !c.anchored) {
            for i in c.range.clone() {
                let p = s.p[i];
                s.p[i] = [p[0], 1. + (p[1] - 1.) * angle.cos() - p[2] * angle.sin(),
                    (p[1] - 1.) * angle.sin() + p[2] * angle.cos()];
            }
        }
        let dt = 1. / 120.;
        let mut q = s.p.clone();
        s.solve_interfaces(&mut q, dt);
        let j = s.joints.iter().find(|j| s.felling_interface(j)).unwrap();
        let net_stress = length(j.lambda.iter().copied().fold([0.; 3], add)) / (dt * dt * j.area);
        assert!(j.peak_stress > net_stress * 10., "bending edge stress must not cancel");
    }

    #[test]
    fn partial_cut_reduces_hinge_stiffness_without_erasing_support() {
        let mut intact = trunk();
        for c in intact.cells.iter().filter(|c| !c.anchored) {
            for i in c.range.clone() { intact.p[i] = add(intact.p[i], [0.002, 0.002, 0.]); }
        }
        let mut weak = intact.clone();
        let cut = weak.joints.iter().position(|j| weak.felling_interface(j)).unwrap();
        weak.joints[cut].damage = 0.9;
        let mut stiff_target = intact.p.clone();
        let mut weak_target = weak.p.clone();
        intact.solve_interfaces(&mut stiff_target, 1. / 120.);
        weak.solve_interfaces(&mut weak_target, 1. / 120.);
        let gap = |p: &[V]| weak.joints[cut].pairs.iter()
            .map(|&[a, b]| length(sub(p[b], p[a]))).sum::<f32>();
        assert!(gap(&weak_target) > gap(&stiff_target));
        assert!(!weak.joints[cut].broken);
        assert_eq!(weak.p.len(), intact.p.len());
    }

    #[test]
    fn severed_crown_falls_under_gravity_and_preserves_stump_and_mass() {
        let mut s = trunk();
        let initial = s.p.clone();
        let mass: f64 = s.cells.iter().map(|c| c.body.mass).sum();
        // This test isolates the consequence of a completed cut. It does not
        // claim that axe contact supplied the required fracture work.
        let cuts: Vec<_> = s.joints.iter().map(|j| s.felling_interface(j)).collect();
        for (j, cut) in s.joints.iter_mut().zip(cuts) {
            if cut { j.broken = true; j.damage = 1.; }
        }
        s.rebuild_wood_pieces();
        for _ in 0..120 { free_step(&mut s); }
        let crown = s.crown.as_ref().unwrap().0;
        let top = s.cells[crown].range.clone();
        let drop = centroid(&initial[top.clone()])[1] - centroid(&s.p[top])[1];
        assert!(drop > 0.2, "severed asymmetric crown must fall under its weight: {drop}");
        assert_eq!(mass, s.cells.iter().map(|c| c.body.mass).sum::<f64>());
        assert!(s.p.iter().flatten().all(|v| v.is_finite()));
        for c in s.cells.iter().filter(|c| c.anchored) {
            assert_eq!(&s.p[c.range.clone()], &initial[c.range.clone()]);
        }
    }
}

#[cfg(test)]
impl Solid {
    pub(super) fn crafting_fixture(fallen:bool)->Self{
        let shape=Poly::box_from([-0.15,0.,-0.15],[0.15,3.,0.15]);
        let cells=solid_geometry::partition(&shape,&[[0.,0.5,0.],[0.,1.5,0.],[0.,2.5,0.]],[1.;3],0).unwrap();
        let mut s=Self::from_specimen(solid_geometry::Specimen{cells,kind:SolidKind::Wood,grain:[0.,1.,0.],bark:[0.3,0.2,0.1],source:"crafting cut consequence fixture",volume:0.27},"game_tree").unwrap();
        s.prepare_felling(0.9,1.1);s.attach_crown(vec![Poly::box_from([-0.15,3.,-0.15],[0.15,4.,0.15])],vec![]);
        if fallen{
            for j in &mut s.joints{j.broken=true;}
            for (k,c) in s.cells.iter().enumerate(){if !c.anchored{
                for p in &mut s.p[c.range.clone()]{*p=[p[0],-p[2]+0.2,p[1]-3.5];}
                let _=k;
            }}
        }s
    }
}
#[cfg(test)]mod crafting_tree_checks{
 use super::*;
 #[test]fn crafting_requires_cut_and_horizontal_crown(){
  assert!(Solid::crafting_fixture(false).fallen_crown().is_none());
  let mut cut=Solid::crafting_fixture(false);for j in &mut cut.joints{j.broken=true;}
  assert!(cut.fallen_crown().is_none(),"severed but upright wood is not yet felled");
  assert!(Solid::crafting_fixture(true).fallen_crown().is_some());
 }
}
