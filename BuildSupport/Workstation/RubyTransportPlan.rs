//! Diagnostic plan for unengraved, intrinsic-only ruby transport.
//! Never pair this with the production RubyResponseShader: that shader still
//! bakes illumination-dependent engraving. This alternative removes only input
//! dependencies absent from a bake with all engraving disabled. Studio lighting,
//! labels and pixel footprints must instead be resolved live during playback.
//! Geometry is immutable for a native session. Translation updates descriptors
//! without rebaking; shape, material, scale and path-budget edits still rebake.
use std::collections::{BTreeMap, HashMap};

pub(crate) const CACHE_BYTES: usize = 120 * 1024 * 1024;
pub(crate) const PLAN_ROWS: usize = 5346;
const SLOTS: usize = 20;
const MAP_BASE: usize = 161;
const PANEL_ROWS: usize = 128 * 128 * 60;
const CONTROL_ROWS: usize = 64 * 64 * 60;

pub(crate) struct Update {
    pub rows: Vec<[f32; 4]>,
    pub entry_count: u32,
    pub dirty: bool,
    pub ready_count: usize,
}

#[derive(Clone)]
struct Component { material: usize, lo: [f32; 3], hi: [f32; 3] }
// Keep the plane/pose anchor from the actual bake, not the most recent frame;
// otherwise tolerance-sized edits could accumulate without an invalidation.
struct PanelPlanes { pose: [f32; 4], planes: [[f32; 4]; 26] }
struct Entry { target: usize, key: Vec<u32>, descriptor: [[f32; 4]; 8], panel: Option<PanelPlanes>, dirty: bool }
struct Candidate { target: usize, key: Vec<u32>, descriptor: [[f32; 4]; 8], panel: Option<PanelPlanes>, area: f32 }

pub(crate) struct Plan {
    initial: Vec<[f32; 4]>,
    geometry: HashMap<(usize, usize), Vec<Component>>,
    slots: [Option<Entry>; SLOTS],
}

fn index(value: f32) -> Option<usize> {
    (value.is_finite() && value >= 0. && value < 16_777_216. && value.fract() == 0.)
        .then_some(value as usize)
}

fn ruby(material: [f32; 4], sigma: [f32; 4]) -> bool {
    // Optical identity, not a filename or a particular control instance.
    material[3] > 0.99 && (material[1] - 1.766).abs() < 0.001
        && material[2] >= 0. && material[2] <= 0.1
        && sigma[0] > 0. && sigma[1] > sigma[0] * 4. && sigma[2] > sigma[0] * 4.
}

fn append(key: &mut Vec<u32>, row: [f32; 4]) { key.extend(row.map(f32::to_bits)); }

fn translated_planes_match(a: &Option<PanelPlanes>, b: &Option<PanelPlanes>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return a.is_none() && b.is_none(); };
    a.planes.iter().zip(&b.planes).all(|(pa, pb)| {
        if pa[..3] != pb[..3] { return false; }
        // Authored planes are calculated in f64 then packed into world-space
        // f32 rows. Recovering a local distance therefore contains rounding
        // proportional to the world coordinates, not just the object size.
        // Compare against the original bake with an explicit float32 error
        // bound instead of quantizing into bins which jitter at boundaries.
        let mut local_a = f64::from(pa[3]);
        let mut local_b = f64::from(pb[3]);
        let mut magnitude = f64::from(pa[3]).abs() + f64::from(pb[3]).abs();
        for axis in 0..3 {
            let normal = f64::from(pa[axis]);
            local_a -= normal * f64::from(a.pose[axis]);
            local_b -= normal * f64::from(b.pose[axis]);
            magnitude += normal.abs() * (f64::from(a.pose[axis]).abs() + f64::from(b.pose[axis]).abs());
        }
        let tolerance = 4. * f64::from(f32::EPSILON) * magnitude.max(1e-9);
        (local_a - local_b).abs() <= tolerance
    })
}

impl Plan {
    pub(crate) fn new(initial: &[[f32; 4]]) -> Self {
        Self { initial: initial.to_vec(), geometry: HashMap::new(), slots: std::array::from_fn(|_| None) }
    }

    fn row(&self, frame: &[[f32; 4]], at: usize) -> [f32; 4] {
        frame.get(at).or_else(|| self.initial.get(at)).copied().unwrap_or([0.; 4])
    }

    fn components(&mut self, frame: &[[f32; 4]], root: usize, end: usize) -> Vec<Component> {
        if let Some(found) = self.geometry.get(&(root, end)) { return found.clone(); }
        if root >= end || end > self.initial.len().max(frame.len()) || (end-root)%3 != 0 || end-root > 200_000 {
            return Vec::new();
        }
        let mut components: BTreeMap<usize, Component> = BTreeMap::new();
        for node in (root..end).step_by(3) {
            let count = self.row(frame, node+2)[0] as usize;
            if count == 0 || count > 4 { continue; }
            let Some(first) = index(self.row(frame, node+1)[3]) else { continue; };
            for triangle in 0..count {
                let at = first+triangle*6;
                let a = self.row(frame, at);
                let Some(material) = index(a[3]).filter(|&n| n < 256) else { continue; };
                let b = self.row(frame, at+1); let c = self.row(frame, at+2);
                let points: [[f32; 3]; 3] = [std::array::from_fn(|i| a[i]),
                    std::array::from_fn(|i| b[i]+if b[3]>=0.5 { a[i] } else { 0. }),
                    std::array::from_fn(|i| c[i]+if b[3]>=0.5 { a[i] } else { 0. })];
                let entry = components.entry(material).or_insert(Component {
                    material, lo: [f32::INFINITY; 3], hi: [f32::NEG_INFINITY; 3],
                });
                for point in points {
                    for i in 0..3 { entry.lo[i] = entry.lo[i].min(point[i]); entry.hi[i] = entry.hi[i].max(point[i]); }
                }
            }
        }
        let found: Vec<_> = components.into_values().filter(|c| (0..3).all(|i|
            c.lo[i].is_finite() && c.hi[i].is_finite() && c.lo[i] <= c.hi[i])).collect();
        self.geometry.insert((root, end), found.clone());
        found
    }

    fn common_key(&self, frame: &[[f32; 4]]) -> Vec<u32> {
        let mut key = Vec::new();
        // The compute shader fixes two sample seeds and an axial primary ray.
        // Pixel size, camera translation, lights, floor and labels have no role
        // in unengraved intrinsic transport. Its finite ray budgets do.
        let quality = self.row(frame, 8);
        append(&mut key, [quality[2], quality[3], 0., 0.]);
        key
    }

    fn make_candidate(&self, frame: &[[f32; 4]], common: &[u32], target: usize,
                      material: usize, pose: [f32; 4], bounds: [f32; 4], edge: f32) -> Candidate {
        let mut key = common.to_vec();
        // Keep scale: the canonical epsilon is in world units, so scale reuse
        // is not mathematically identical even though Beer attenuation divides
        // by scale. Translation does not change that epsilon.
        append(&mut key, [0., 0., 0., pose[3]]); append(&mut key, bounds);
        let panel = if target < 4 {
            Some(PanelPlanes { pose, planes: std::array::from_fn(|i| self.row(frame, material+6+i)) })
        } else { None };
        for offset in 0..40 {
            let mut row = self.row(frame, material+offset);
            if target < 4 {
                if offset == 2 { row[..3].fill(0.); }
                if (6..32).contains(&offset) { row[3] = 0.; }
            } else if offset == 2 {
                // Control engraving descriptor; xyz still selects intrinsic
                // material-only geometry or the authored convex planes.
                row[3] = 0.;
            }
            append(&mut key, row);
        }
        if target >= 4 {
            let instance = (target-4)/256;
            let pose_at = self.pose_at(frame, instance);
            let tree = self.row(frame, pose_at+1);
            append(&mut key, [tree[0], tree[1], 0., 0.]);
        }
        let mut descriptor = [[0.; 4]; 8];
        descriptor[0] = [target as f32, edge, 0., 0.];
        descriptor[1] = pose;
        descriptor[2] = bounds;
        descriptor[3] = [self.row(frame, 1)[2], 0., 0., 0.];
        Candidate { target, key, descriptor, panel, area: (bounds[2]-bounds[0])*(bounds[3]-bounds[1])*pose[3]*pose[3] }
    }

    fn pose_at(&self, frame: &[[f32; 4]], instance: usize) -> usize {
        if instance>=61 { self.row(frame,232)[0] as usize+32+2*(instance-61) }
        else if instance>=29 { self.row(frame,247)[0] as usize+2*(instance-29) }
        else if instance>=13 { self.row(frame,232)[0] as usize+2*(instance-13) }
        else if instance>=10 { 226+2*(instance-10) }
        else { 192+2*instance }
    }

    fn place(&mut self, slot: usize, mut candidate: Candidate) {
        let same = self.slots[slot].as_ref().is_some_and(|entry|
            entry.target == candidate.target && entry.key == candidate.key
                && translated_planes_match(&entry.panel, &candidate.panel));
        let base = if slot < 4 { slot*PANEL_ROWS } else { 4*PANEL_ROWS+(slot-4)*CONTROL_ROWS };
        candidate.descriptor[0][2] = base as f32;
        if same {
            // Leaf origins are object-local, so playback needs the latest pose
            // even when the intrinsic response remains valid. Preserve dirty
            // until submission; updating a pose must not mark an unbaked slot
            // ready or lose its original shape-comparison anchor.
            self.slots[slot].as_mut().unwrap().descriptor = candidate.descriptor;
            return;
        }
        self.slots[slot] = Some(Entry { target: candidate.target, key: candidate.key,
            descriptor: candidate.descriptor, panel: candidate.panel, dirty: true });
    }

    pub(crate) fn update(&mut self, frame: &[[f32; 4]]) -> Update {
        let mut rows = vec![[-1.; 4]; PLAN_ROWS];
        rows[..MAP_BASE].fill([0.; 4]);
        let disabled = || Update { rows: vec![[0.; 4]; PLAN_ROWS], entry_count: 0, dirty: false, ready_count: 0 };
        if frame.len() < 256 || !frame.iter().flatten().all(|v| v.is_finite())
            || self.row(frame,0)[2] != 2. || self.row(frame,1)[3] != 0.
            || self.row(frame,4)[..3] != [0.,0.,-1.] {
            return disabled();
        }
        let common = self.common_key(frame);
        for panel in 0..4 {
            let base = 32+40*panel; let material = self.row(frame,base);
            if self.row(frame,245)[0] > 0.5 || panel as f32 >= self.row(frame,0)[1]
                || material[0] < 0.5 || !ruby(material,self.row(frame,base+1)) {
                self.slots[panel] = None; continue;
            }
            let center = self.row(frame,base+2); let extent = self.row(frame,base+3);
            if extent[0] <= 0. || extent[1] <= 0.
                || self.row(frame,1)[2] <= center[2]+extent[2] {
                self.slots[panel] = None; continue;
            }
            let bounds = [-extent[0],-extent[1],extent[0],extent[1]];
            self.place(panel,self.make_candidate(frame,&common,panel,base,
                [center[0],center[1],center[2],1.],bounds,128.));
        }
        let mut controls = Vec::new();
        for instance in 0..65 {
            let active = if instance>=61 { ((instance-61) as f32)<self.row(frame,232)[2] }
                else if instance>=29 { ((instance-29) as f32)<self.row(frame,247)[1] }
                else { (instance as f32)<self.row(frame,12)[0] };
            if !active { continue; }
            let at = self.pose_at(frame,instance); let pose = self.row(frame,at); let tree = self.row(frame,at+1);
            if pose[3] <= 0. { continue; }
            let (Some(root),Some(end)) = (index(tree[0]),index(tree[1])) else { continue; };
            let base = if tree[3]>0.5 { tree[3] as usize } else { self.row(frame,12)[1] as usize };
            for component in self.components(frame,root,end) {
                let material = base+component.material*40;
                if !ruby(self.row(frame,material),self.row(frame,material+1)) { continue; }
                if self.row(frame,1)[2] <= pose[2]+component.hi[2]*pose[3] { continue; }
                let bounds = [component.lo[0],component.lo[1],component.hi[0],component.hi[1]];
                if bounds[0]>=bounds[2] || bounds[1]>=bounds[3] { continue; }
                let camera = self.row(frame,1); let right = self.row(frame,2); let up = self.row(frame,3);
                if pose[0]+bounds[2]*pose[3] < camera[0]-right[3] || pose[0]+bounds[0]*pose[3] > camera[0]+right[3]
                    || pose[1]+bounds[3]*pose[3] < camera[1]-up[3] || pose[1]+bounds[1]*pose[3] > camera[1]+up[3] { continue; }
                controls.push(self.make_candidate(frame,&common,4+instance*256+component.material,
                    material,pose,bounds,64.));
            }
        }
        controls.sort_by(|a,b| b.area.total_cmp(&a.area).then_with(||a.target.cmp(&b.target)));
        let eligible_controls = controls.len();
        controls.truncate(16);
        let admitted_controls = controls.len();
        let wanted: Vec<_> = controls.iter().map(|c| c.target).collect();
        for slot in &mut self.slots[4..] {
            if slot.as_ref().is_some_and(|entry| !wanted.contains(&entry.target)) { *slot = None; }
        }
        for candidate in controls {
            let slot = (4..SLOTS).find(|&slot| self.slots[slot].as_ref().is_some_and(|entry|entry.target==candidate.target))
                .or_else(|| (4..SLOTS).find(|&slot|self.slots[slot].is_none())).unwrap();
            self.place(slot,candidate);
        }
        let mut count = 0; let mut ready_count = 0; let mut dirty = false;
        for (slot,entry) in self.slots.iter().enumerate() {
            let Some(entry) = entry else { continue; };
            count = slot+1; ready_count += 1; dirty |= entry.dirty;
            let at = 1+slot*8;
            rows[at..at+8].copy_from_slice(&entry.descriptor);
            rows[at][3] = if entry.dirty { 2. } else { 1. };
            rows[MAP_BASE+entry.target/4][entry.target%4] = slot as f32;
        }
        rows[0] = [count as f32,1.,eligible_controls as f32,admitted_controls as f32];
        Update { rows, entry_count: count as u32, dirty, ready_count }
    }

    pub(crate) fn submitted(&mut self) {
        for entry in self.slots.iter_mut().flatten() { entry.dirty = false; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<[f32; 4]> {
        let mut f=vec![[0.;4];1024]; f[0]=[0.,1.,2.,0.];f[1]=[0.,0.,3.,0.];
        f[2]=[1.,0.,0.,1.];f[3]=[0.,1.,0.,1.];f[4]=[0.,0.,-1.,1.];f[8]=[640.,480.,16.,64.];
        f[32]=[1.,1.766,0.018,1.];f[33]=[3.7,412.,412.,0.];f[35]=[1.,1.,0.5,0.];
        let planes = [[1.,0.,0.,1.],[-1.,0.,0.,1.],[0.,1.,0.,1.],
            [0.,-1.,0.,1.],[0.,0.,1.,0.5],[0.,0.,-1.,0.5]];
        for i in 0..26 { f[38+i] = planes[i%6]; }
        f
    }
    fn translate_panel(frame: &mut [[f32; 4]], delta: [f32; 3]) {
        for axis in 0..3 { frame[34][axis] += delta[axis]; }
        for plane in &mut frame[38..64] {
            plane[3] = (f64::from(plane[3]) + (0..3)
                .map(|axis| f64::from(plane[axis])*f64::from(delta[axis])).sum::<f64>()) as f32;
        }
    }
    fn control_fixture() -> Vec<[f32; 4]> {
        let mut f=fixture();
        f[12]=[1.,600.,0.,0.];f[192]=[0.,0.,0.,1.];f[193]=[512.,515.,0.,600.];
        f[512]=[-0.1,-0.1,0.1,515.];f[513]=[0.1,0.1,0.1,520.];f[514]=[1.,0.,0.,0.];
        f[520]=[-0.1,-0.1,0.1,0.];f[521]=[0.2,0.,0.,1.];f[522]=[0.,0.2,0.,0.];
        f[600]=f[32];f[601]=f[33];f[603]=[0.1,0.1,0.1,0.];f
    }
    #[test]
    fn transport_plan_waits_for_submission_and_invalidates_shape_optics_and_budgets() {
        let mut f=fixture();let mut plan=Plan::new(&f);
        assert!(plan.update(&f).dirty);assert!(plan.update(&f).dirty);
        plan.submitted();assert!(!plan.update(&f).dirty);
        for (at,component) in [(38,0),(38,3),(35,0),(32,2),(33,0),(64,0),(8,2),(8,3)] {
            f[at][component]+=0.01;assert!(plan.update(&f).dirty);plan.submitted();
            assert!(!plan.update(&f).dirty);
        }
        f[1][3]=1.;assert_eq!(plan.update(&f).ready_count,0);
    }
    #[test]
    fn transport_plan_keeps_lighting_engraving_viewport_and_camera_translation_live() {
        let mut f=fixture();let mut plan=Plan::new(&f);plan.update(&f);plan.submitted();
        for at in [5,6,9,16,20,24,30,212,213,214,215,216,218,219,222,223,224,240,244] {
            f[at][0]+=0.2;
            assert!(!plan.update(&f).dirty, "playback-only row {at} invalidated intrinsic transport");
        }
        f[0][3]=2.;f[1]=[0.4,0.2,4.,0.];f[2][3]=2.;f[3][3]=1.5;f[8][0]=1280.;f[8][1]=720.;
        let update=plan.update(&f);assert!(!update.dirty);assert_eq!(update.rows[MAP_BASE][0],0.);
        assert_eq!(update.rows[4][0],4.);
        assert_eq!(4*PANEL_ROWS+16*CONTROL_ROWS,CACHE_BYTES/16);
        assert!(MAP_BASE+(16644usize+4096).div_ceil(4)<=PLAN_ROWS);
    }
    #[test]
    fn transport_plan_reuses_translation_and_updates_pose_without_clearing_pending_bake() {
        let mut f=fixture();let mut plan=Plan::new(&f);plan.update(&f);
        translate_panel(&mut f,[0.125,-0.25,0.125]);
        let pending=plan.update(&f);assert!(pending.dirty);
        assert_eq!(pending.rows[2][..3],f[34][..3]);
        plan.submitted();
        translate_panel(&mut f,[0.125,0.25,-0.125]);
        let ready=plan.update(&f);assert!(!ready.dirty);assert_eq!(ready.rows[2][..3],f[34][..3]);
        assert_eq!(ready.rows[1][3],1.);
        // The shape-comparison anchor remains the baked shape, so a real
        // plane edit cannot be hidden by later descriptor translations.
        f[38][3]+=0.001;assert!(plan.update(&f).dirty);
    }
    #[test]
    fn transport_plan_reuses_control_translation_and_glyph_edits_but_not_scale() {
        let mut f=control_fixture();let mut plan=Plan::new(&f);
        assert_eq!(plan.update(&f).ready_count,2);plan.submitted();
        f[192]=[0.25,-0.125,0.2,1.];f[602][3]=700.;f[700]=[3.,4.,5.,6.];f[193][2]=4.;
        let ready=plan.update(&f);assert!(!ready.dirty);
        assert_eq!(ready.rows[2+4*8],f[192]);
        f[192][3]=1.1;assert!(plan.update(&f).dirty);
    }
    #[test]
    fn transport_plan_rejects_camera_inside_component() {
        let mut f=control_fixture();let mut plan=Plan::new(&f);plan.update(&f);plan.submitted();
        f[1][2]=0.05;assert_eq!(plan.update(&f).ready_count,0);
    }
}
