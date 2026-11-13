//! Control-thread ownership of bounded intrinsic glass transport.
//! This plan requires RubyResponseShader's bake to exclude all engraving:
//! panel, control, timeline and plugin-navigation radiance AND attenuation.
//! Studio lighting, labels and pixel footprints are resolved live at playback;
//! reintroducing them into the bake also requires restoring their key inputs.
//! Geometry is immutable for a native session. Translation updates descriptors
//! without rebaking; shape, material, scale and path-budget edits still rebake.
//! Opt-in screen-pixel control transport retains every text segment and keys
//! exact camera, viewport, pose, glyph and engraving-light inputs.
use std::collections::{BTreeMap, HashMap};
#[path = "RubyCacheCauses.rs"]
mod cache_causes;

pub(crate) const CACHE_BYTES: usize = 120 * 1024 * 1024;
pub(crate) const PLAN_ROWS: usize = 5346;
const SLOTS: usize = 20;
const MAP_BASE: usize = 161;
const PANEL_ROWS: usize = 128 * 128 * 60;
const CONTROL_ROWS: usize = 64 * 64 * 60;
const CONTROL_BASE: usize = 4 * PANEL_ROWS;
const CACHE_ROWS: usize = CACHE_BYTES / 16;
const TIMELINE_FIRST: usize = 700_000;
const TIMELINE_END: usize = 704_096;
const TIMELINE_MAP: usize = 16_644;

fn map_target(target: usize) -> Option<usize> {
    if target < TIMELINE_MAP { Some(target) }
    else if (TIMELINE_FIRST..TIMELINE_END).contains(&target) {
        Some(TIMELINE_MAP + target - TIMELINE_FIRST)
    } else { None }
}

fn candidate_priority(candidate: &Candidate) -> u8 {
    if candidate.descriptor[4][3] == 0. { 0 }
    else if (TIMELINE_FIRST..TIMELINE_END).contains(&candidate.target) { 1 }
    else { 2 }
}

// Only mode1 allocations share free space. Legacy 64-square ruby entries keep
// their original fixed blocks. Earlier candidates own higher priority spans.
fn pixel_span(occupied: &mut Vec<(usize, usize)>, preferred: Option<(usize, usize)>,
              required: usize) -> Option<(usize, usize)> {
    if required == 0 || required > CACHE_ROWS - CONTROL_BASE { return None; }
    let valid = |base: usize, capacity: usize| base >= CONTROL_BASE && capacity >= required
        && base.checked_add(capacity).is_some_and(|end| end <= CACHE_ROWS
            && occupied.iter().all(|&(start, count)| end <= start || base >= start + count));
    if let Some((base, capacity)) = preferred.filter(|&(base, capacity)| valid(base, capacity)) {
        occupied.push((base, capacity));
        return Some((base, capacity));
    }
    occupied.sort_unstable_by_key(|span| span.0);
    let mut base = CONTROL_BASE;
    for &(start, count) in occupied.iter() {
        if base + required <= start { break; }
        base = base.max(start + count);
    }
    if base + required > CACHE_ROWS { return None; }
    occupied.push((base, required));
    Some((base, required))
}

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
    // Opt-in until matched native motion and appearance validation completes.
    pixel_transport: bool,
    geometry: HashMap<(usize, usize), Vec<Component>>,
    slots: [Option<Entry>; SLOTS],
    cache_causes: Option<cache_causes::CacheCauses>,
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
        Self { initial: initial.to_vec(), pixel_transport: std::env::var("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref()==Ok("1"), geometry: HashMap::new(), slots: std::array::from_fn(|_| None),
            cache_causes: (std::env::var("WORMWOOD_WORKSTATION_CACHE_CAUSES").as_deref()==Ok("1"))
                .then(cache_causes::CacheCauses::new) }
    }

    pub(crate) fn cache_causes(&self) -> Option<&serde_json::Value> {
        self.cache_causes.as_ref().map(cache_causes::CacheCauses::report)
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
        if (4..TIMELINE_MAP).contains(&target) {
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

    // Exact screen-pixel transport is admitted only when the original camera
    // expressions are invertible and the complete two-sample records fit.
    fn pixel_candidate(&self, frame: &[[f32; 4]], mut candidate: Candidate) -> Option<Candidate> {
        let camera = self.row(frame, 1);
        let right = self.row(frame, 2);
        let up = self.row(frame, 3);
        let forward = self.row(frame, 4);
        let quality = self.row(frame, 8);
        if right[..3] != [1., 0., 0.] || up[..3] != [0., 1., 0.]
            || forward[..3] != [0., 0., -1.] || camera[3] != 0.
            || right[3] <= 0. || up[3] <= 0.
            || quality[2] < 1. || quality[2] > 16. || quality[3] < 1. || quality[3] > 64. {
            return None;
        }
        let width = index(quality[0]).filter(|&n| n > 0 && n <= 16384)?;
        let height = index(quality[1]).filter(|&n| n > 0 && n <= 16384)?;
        let pose = candidate.descriptor[1];
        let bounds = candidate.descriptor[2];
        let project_x = |value: f32| ((f64::from(pose[0]) + f64::from(value)*f64::from(pose[3])
            - f64::from(camera[0]))/f64::from(right[3]) + 1.) * width as f64 * 0.5;
        let project_y = |value: f32| (1. - (f64::from(pose[1]) + f64::from(value)*f64::from(pose[3])
            - f64::from(camera[1]))/f64::from(up[3])) * height as f64 * 0.5;
        let x0 = (project_x(bounds[0]).floor() - 2.).max(0.).min(width as f64) as usize;
        let x1 = (project_x(bounds[2]).ceil() + 2.).max(0.).min(width as f64) as usize;
        let y0 = (project_y(bounds[3]).floor() - 2.).max(0.).min(height as f64) as usize;
        let y1 = (project_y(bounds[1]).ceil() + 2.).max(0.).min(height as f64) as usize;
        // Both compute passes flatten a bounded 128-by-128 invocation grid.
        // Actual storage is allocated later from free legacy-control blocks.
        if x1 <= x0 || y1 <= y0 || (x1-x0)*(y1-y0) > 128 * 128 { return None; }
        candidate.descriptor[4] = [(x1-x0) as f32, (y1-y0) as f32, 108., 1.];
        candidate.descriptor[5] = [x0 as f32, y0 as f32, 0., 0.];
        for row in [pose, camera, right, up, forward,
            [quality[0], quality[1], 0., 0.], candidate.descriptor[4], candidate.descriptor[5]] {
            append(&mut candidate.key, row);
        }
        Some(candidate)
    }

    fn engraving_key(&self, frame: &[[f32;4]], material: usize, candidate: &mut Candidate) -> Option<()> {
        let descriptor = index(self.row(frame,material+2)[3])?;
        if descriptor==0 { return Some(()); }
        if !self.pixel_transport { return None; }
        let packet = self.row(frame,descriptor+2);
        let count = index(packet[1]).filter(|&n| (1..=11).contains(&n))?;
        let mip_start = index(packet[2])?;
        let mut texels = 0usize;
        for at in mip_start..mip_start.checked_add(count)? {
            let mip = self.row(frame,at);
            let width = index(mip[0]).filter(|&n| n>0)?;
            let height = index(mip[1]).filter(|&n| n>0)?;
            let offset = index(mip[2])?;
            texels=texels.max(offset.checked_add(width.checked_mul(height)?)?);
            append(&mut candidate.key,mip);
        }
        let raster = index(self.row(frame,244)[0])?.checked_add(index(packet[0])?)?;
        let end = raster.checked_add(texels.div_ceil(2))?;
        // Immutable session data, like the triangle mesh. A raster in the
        // mutable frame is declined rather than silently dropping its key.
        if raster < frame.len() || end > self.initial.len() { return None; }
        append(&mut candidate.key,[descriptor as f32,raster as f32,end as f32,1.]);
        for at in descriptor..descriptor.checked_add(3)? { append(&mut candidate.key,self.row(frame,at)); }
        for at in [5,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,213,214,215,216,223,244] {
            let mut row = self.row(frame,at);
            // Engraving reads ambient RGB; floor Z belongs to terminal resolve.
            // Retain the original float32 RGB bits without recomputing light.
            if at == 5 { row[3] = 0.; }
            append(&mut candidate.key,row);
        }
        Some(())
    }

    fn timeline_engraving_key(&self, frame: &[[f32;4]], instance: usize,
                              candidate: &mut Candidate) -> Option<()> {
        let header = self.row(frame,248);
        // Counts establish current activity; they are not this handle's shape
        // or engraving inputs. Keep this proof next to their normalization.
        let count = index(header[0]).filter(|n| *n <= 256)?;
        let handles = index(header[3]).filter(|n| *n <= count)?;
        if instance >= handles { return None; }
        let instances = index(header[1])?;
        let letters = index(header[2])?.checked_add(instance.checked_mul(2)?)?;
        let at = instances.checked_add(instance.checked_mul(3)?)?;
        if at.checked_add(3)? > frame.len() || letters.checked_add(2)? > frame.len() { return None; }
        // Unlike the control text descriptors, timeline lettering reads its
        // eight dynamic ASCII codes directly and uses the shared glyph atlas.
        let atlas = self.row(frame,224);
        let density = index(atlas[0])?;
        if atlas[1] != 128. || atlas[2] <= 0. || atlas[3] != 21_846. { return None; }
        let stride = index(atlas[3])?;
        let mut ended = false;
        for code in self.row(frame,letters).into_iter().chain(self.row(frame,letters+1)) {
            if ended { continue; }
            if code == 0. { ended = true; continue; }
            let cp = index(code).filter(|cp| (32..=126).contains(cp))?;
            let begin = density.checked_add((cp-32).checked_mul(stride)? / 2)?;
            let end = begin.checked_add(stride / 2)?;
            // Geometry and glyph texels are immutable for this Plan instance.
            // Decline mutable atlas spans instead of omitting their contents.
            if begin < frame.len() || end > self.initial.len() { return None; }
            append(&mut candidate.key,[cp as f32,begin as f32,end as f32,1.]);
        }
        // Own-instance queries cannot observe other active handles or clips.
        // Keep both pointers, the full clip, and every own-instance/text row.
        let intrinsic_header = [0.,header[1],header[2],0.];
        for row in [intrinsic_header,self.row(frame,249),self.row(frame,at),self.row(frame,at+1),
                    self.row(frame,at+2),self.row(frame,letters),self.row(frame,letters+1)] {
            append(&mut candidate.key,row);
        }
        for at in [5,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,213,214,215,216,223,224] {
            let mut row = self.row(frame,at);
            // Timeline engraving uses only ambient RGB from this shared row.
            // A floor-height edit still invalidates RubyResolveReuse's key.
            if at == 5 { row[3] = 0.; }
            append(&mut candidate.key,row);
        }
        Some(())
    }

    fn timeline_candidate(&mut self, frame: &[[f32;4]], common: &[u32],
                          instance: usize) -> Option<Candidate> {
        let header = self.row(frame,248);
        let count = index(header[0]).filter(|n| *n <= 256)?;
        let handles = index(header[3]).filter(|n| *n <= count)?;
        if !self.pixel_transport || instance >= handles { return None; }
        let at = index(header[1])?.checked_add(instance.checked_mul(3)?)?;
        if at.checked_add(3)? > frame.len() { return None; }
        let pose = self.row(frame,at);let tree = self.row(frame,at+1);let size = self.row(frame,at+2);
        if pose[3] <= 0. || tree[3] != 0. || size.iter().any(|v| *v <= 0.) { return None; }
        let root = index(tree[0])?;let end = index(tree[1])?;let material = index(tree[2])?;
        // The authored non-tape handle has one material and immutable geometry.
        // A future mixed-material/tape asset must retain canonical tracing.
        if root != material.checked_add(40)? || root < frame.len() || end > self.initial.len() { return None; }
        let components = self.components(frame,root,end);
        if components.len() != 1 || components[0].material != 0
            || !ruby(self.row(frame,material),self.row(frame,material+1)) { return None; }
        let component = &components[0];
        let clip = self.row(frame,249);
        // Enclose both the authored vertices and the exact broad-phase box.
        // Strict float32 margins keep clipping inactive for the entire object.
        let lo = [component.lo[0].min(-size[0]*0.5),component.lo[1].min(-size[1]*0.5),component.lo[2].min(-size[2])];
        let hi = [component.hi[0].max(size[0]*0.5),component.hi[1].max(size[1]*0.5),component.hi[2].max(1e-9)];
        for axis in 0..2 {
            let lower = f64::from(pose[axis]) + f64::from(lo[axis])*f64::from(pose[3]);
            let upper = f64::from(pose[axis]) + f64::from(hi[axis])*f64::from(pose[3]);
            let magnitude = f64::from(pose[axis]).abs()
                + f64::from(lo[axis].abs().max(hi[axis].abs()))*f64::from(pose[3]);
            let margin = 32. * f64::from(f32::EPSILON)*magnitude.max(1e-6);
            if lower - margin <= f64::from(clip[axis])
                || upper + margin >= f64::from(clip[axis+2]) { return None; }
        }
        if f64::from(self.row(frame,1)[2]) <= f64::from(pose[2])+f64::from(hi[2])*f64::from(pose[3]) { return None; }
        let bounds = [lo[0],lo[1],hi[0],hi[1]];
        let target = TIMELINE_FIRST + instance * 16;
        let candidate = self.make_candidate(frame,common,target,material,pose,bounds,64.);
        let mut candidate = self.pixel_candidate(frame,candidate)?;
        self.timeline_engraving_key(frame,instance,&mut candidate)?;
        Some(candidate)
    }

    fn pose_at(&self, frame: &[[f32; 4]], instance: usize) -> usize {
        if instance>=61 { self.row(frame,232)[0] as usize+32+2*(instance-61) }
        else if instance>=29 { self.row(frame,247)[0] as usize+2*(instance-29) }
        else if instance>=13 { self.row(frame,232)[0] as usize+2*(instance-13) }
        else if instance>=10 { 226+2*(instance-10) }
        else { 192+2*instance }
    }

    fn place(&mut self, slot: usize, candidate: Candidate) {
        let base = if slot < 4 { slot*PANEL_ROWS } else { CONTROL_BASE+(slot-4)*CONTROL_ROWS };
        self.place_at(slot,candidate,base,0);
    }

    fn place_at(&mut self, slot: usize, mut candidate: Candidate, base: usize, capacity: usize) {
        candidate.descriptor[0][2] = base as f32;
        candidate.descriptor[6][0] = capacity as f32;
        let same = self.slots[slot].as_ref().is_some_and(|entry|
            entry.target == candidate.target && entry.key == candidate.key
                && translated_planes_match(&entry.panel, &candidate.panel)
                && entry.descriptor[0][2] == candidate.descriptor[0][2]
                && entry.descriptor[6][0] == candidate.descriptor[6][0]);
        if let Some(causes) = &mut self.cache_causes {
            causes.place(slot, self.slots[slot].as_ref(), &candidate, same);
        }
        if same {
            // Preserve a pending bake. Moving or resizing a pixel allocation
            // is never considered reusable, even when its optical key matches.
            self.slots[slot].as_mut().unwrap().descriptor = candidate.descriptor;
            return;
        }
        self.slots[slot] = Some(Entry { target: candidate.target, key: candidate.key,
            descriptor: candidate.descriptor, panel: candidate.panel, dirty: true });
    }

    pub(crate) fn update(&mut self, frame: &[[f32; 4]]) -> Update {
        if let Some(causes) = &mut self.cache_causes { causes.begin(&self.slots); }
        let mut rows = vec![[-1.; 4]; PLAN_ROWS];
        rows[..MAP_BASE].fill([0.; 4]);
        let disabled = || Update { rows: vec![[0.; 4]; PLAN_ROWS], entry_count: 0, dirty: false, ready_count: 0 };
        if frame.len() < 256 || !frame.iter().flatten().all(|v| v.is_finite())
            || self.row(frame,0)[2] != 2. || self.row(frame,1)[3] != 0.
            || self.row(frame,4)[..3] != [0.,0.,-1.] {
            if let Some(causes) = &mut self.cache_causes { causes.finish(true); }
            return disabled();
        }
        let common = self.common_key(frame);
        for panel in 0..4 {
            let base = 32+40*panel; let material = self.row(frame,base);
            if self.row(frame,245)[0] > 0.5 || panel as f32 >= self.row(frame,0)[1]
                || material[0] < 0.5 || !ruby(material,self.row(frame,base+1)) {
                if let Some(causes) = &mut self.cache_causes { causes.removed(panel, "inactive_disabled_or_non_ruby_panel"); }
                self.slots[panel] = None; continue;
            }
            let center = self.row(frame,base+2); let extent = self.row(frame,base+3);
            if extent[0] <= 0. || extent[1] <= 0.
                || self.row(frame,1)[2] <= center[2]+extent[2] {
                if let Some(causes) = &mut self.cache_causes { causes.removed(panel, "invalid_extent_or_camera_behind_panel"); }
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
                let optical = self.row(frame,material);
                let is_ruby = ruby(optical,self.row(frame,material+1));
                let pixel = self.pixel_transport && !is_ruby && (4..=9).contains(&instance)
                    && optical[0] >= 0.5 && optical[3] > 0.99
                    && optical[1] > 1. && optical[2] >= 0. && optical[2] <= 0.1;
                if !is_ruby && !pixel { continue; }
                if self.row(frame,1)[2] <= pose[2]+component.hi[2]*pose[3] { continue; }
                let bounds = [component.lo[0],component.lo[1],component.hi[0],component.hi[1]];
                if bounds[0]>=bounds[2] || bounds[1]>=bounds[3] { continue; }
                let camera = self.row(frame,1); let right = self.row(frame,2); let up = self.row(frame,3);
                if pose[0]+bounds[2]*pose[3] < camera[0]-right[3] || pose[0]+bounds[0]*pose[3] > camera[0]+right[3]
                    || pose[1]+bounds[3]*pose[3] < camera[1]-up[3] || pose[1]+bounds[1]*pose[3] > camera[1]+up[3] { continue; }
                let candidate = self.make_candidate(frame,&common,4+instance*256+component.material,
                    material,pose,bounds,64.);
                if pixel {
                    if let Some(mut candidate) = self.pixel_candidate(frame,candidate) {
                        if self.engraving_key(frame,material,&mut candidate).is_some() { controls.push(candidate); }
                    }
                } else { controls.push(candidate); }
            }
        }
        if self.pixel_transport {
            let header = self.row(frame,248);
            if let (Some(count),Some(handles)) = (index(header[0]),index(header[3])) {
                if handles <= count && count <= 256 {
                    for instance in 0..handles {
                        if let Some(candidate) = self.timeline_candidate(frame,&common,instance) { controls.push(candidate); }
                    }
                }
            }
        }
        // Preserve legacy ruby first, then exact timeline ruby, then optional
        // non-ruby pixel glass. Capacity failure retains canonical rendering.
        controls.sort_by(|a,b| candidate_priority(a).cmp(&candidate_priority(b))
            .then_with(|| b.area.total_cmp(&a.area)).then_with(||a.target.cmp(&b.target)));
        let eligible_controls = controls.len();
        controls.truncate(16);
        let wanted: Vec<_> = controls.iter().map(|c| c.target).collect();
        for (slot, entry) in self.slots.iter_mut().enumerate().skip(4) {
            if entry.as_ref().is_some_and(|entry| !wanted.contains(&entry.target)) {
                if let Some(causes) = &mut self.cache_causes { causes.removed(slot, "not_in_selected_candidates"); }
                *entry = None;
            }
        }
        let mut assigned = [false;SLOTS];
        let mut pixels = Vec::new();
        let mut occupied = Vec::new();
        for candidate in controls {
            let slot = (4..SLOTS).find(|&slot| self.slots[slot].as_ref().is_some_and(|entry|entry.target==candidate.target))
                .or_else(|| (4..SLOTS).find(|&slot| !assigned[slot] && self.slots[slot].is_none())).unwrap();
            assigned[slot] = true;
            if candidate.descriptor[4][3] == 1. { pixels.push((slot,candidate)); }
            else {
                self.place(slot,candidate);
                occupied.push((CONTROL_BASE+(slot-4)*CONTROL_ROWS,CONTROL_ROWS));
            }
        }
        for (slot,candidate) in pixels {
            let shape = candidate.descriptor[4];
            let required = shape[0] as usize * shape[1] as usize * 108;
            let preferred = self.slots[slot].as_ref().filter(|entry| entry.target == candidate.target
                && entry.descriptor[4][3] == 1.).and_then(|entry|
                    Some((index(entry.descriptor[0][2])?,index(entry.descriptor[6][0])?)));
            if let Some((base,capacity)) = pixel_span(&mut occupied,preferred,required) {
                self.place_at(slot,candidate,base,capacity);
            } else {
                if let Some(causes) = &mut self.cache_causes {
                    causes.allocation_failed(slot, self.slots[slot].as_ref(), &candidate);
                }
                self.slots[slot] = None;
            }
        }
        let admitted_controls = self.slots[4..].iter().flatten().count();
        let mut count = 0; let mut ready_count = 0; let mut dirty = false;
        for (slot,entry) in self.slots.iter().enumerate() {
            let Some(entry) = entry else { continue; };
            count = slot+1; ready_count += 1; dirty |= entry.dirty;
            let at = 1+slot*8;
            rows[at..at+8].copy_from_slice(&entry.descriptor);
            rows[at][3] = if entry.dirty { 2. } else { 1. };
            if let Some(target) = map_target(entry.target) {
                rows[MAP_BASE+target/4][target%4] = slot as f32;
            }
        }
        rows[0] = [count as f32,1.,eligible_controls as f32,admitted_controls as f32];
        if let Some(causes) = &mut self.cache_causes { causes.finish(false); }
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
    fn pixel_fixture() -> Vec<[f32;4]> {
        let mut f=control_fixture();
        f[12][0]=5.;f[200]=f[192];f[201]=f[193];
        f[192][3]=0.;f[200][3]=0.2;
        f[601]=[11.,14.,20.,0.];
        f
    }
    #[test]
    fn pixel_cache_keys_exact_projection_pose_but_keeps_lighting_live() {
        let mut f=pixel_fixture();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        let first=plan.update(&f);assert_eq!(first.ready_count,2);
        assert_eq!(first.rows[1+4*8+4][3],1.);plan.submitted();
        f[16][0]+=0.1;assert!(!plan.update(&f).dirty);
        for (at,c) in [(200,0),(200,1),(200,2),(200,3),(1,0),(1,1),(1,2),(2,3),(3,3),(8,0),(8,1)] {
            f[at][c]+=if at==8 {1.}else{0.001};
            assert!(plan.update(&f).dirty,"pixel key omitted row {at} component {c}");
            plan.submitted();assert!(!plan.update(&f).dirty);
        }
    }
    #[test]
    fn pixel_cache_declines_engraving_large_rectangles_and_unsupported_camera() {
        let base=pixel_fixture();
        for (at,c,value) in [(602,3,700.),(200,3,10.),(2,1,0.01),(3,0,0.01),(8,0,640.5),(8,2,17.),(8,3,65.)] {
            let mut f=base.clone();f[at][c]=value;let mut plan=Plan::new(&f);plan.pixel_transport=true;
            let update=plan.update(&f);
            assert_eq!(update.ready_count,1,"new mode did not fail closed for {at}:{c}");
        }
    }
    #[test]
    fn pixel_transport_keys_full_engraving_and_declines_mutable_raster() {
        let mut f=pixel_fixture();f.resize(2048,[0.;4]);
        f[602][3]=700.;f[700]=[-0.1,-0.1,0.2,0.2];f[701]=[-0.01,-0.02,-0.03,0.02];
        f[702]=[4.,1.,704.,0.];f[704]=[2.,2.,0.,0.];f[244]=[1200.,10.,0.,0.];
        let mut plan=Plan::new(&f);plan.pixel_transport=true;
        assert_eq!(plan.update(&f[..1024]).ready_count,2);plan.submitted();
        for (at,channel) in [(5,0),(16,0),(20,1),(25,2),(30,0),(213,0),(214,3),(215,2),(216,1),(223,3),(244,1),(700,0),(701,2),(704,2)] {
            f[at][channel]+=1.;assert!(plan.update(&f[..1024]).dirty,"glyph key omitted {at}:{channel}");
            plan.submitted();assert!(!plan.update(&f[..1024]).dirty);
        }
        // Supplying the raster as mutable data cannot reuse its initial copy.
        assert_eq!(plan.update(&f).ready_count,1);
    }
    #[test]
    fn pixel_engraving_ignores_floor_height_but_keeps_exact_ambient_rgb() {
        let mut control = pixel_fixture();
        control.resize(2048, [0.; 4]);
        control[602][3] = 700.;
        control[700] = [-0.1, -0.1, 0.2, 0.2];
        control[701] = [-0.01, -0.02, -0.03, 0.02];
        control[702] = [4., 1., 704., 0.];
        control[704] = [2., 2., 0., 0.];
        control[244] = [1200., 10., 0., 0.];
        for (name, mut frame) in [("control", control), ("timeline", timeline_fixture())] {
            frame[5] = [0.02, 0.03, 0.04, -0.2];
            let mut plan = Plan::new(&frame);
            plan.pixel_transport = true;
            let first = plan.update(&frame[..1024]);
            assert_eq!(first.ready_count, 2, "{name} fixture admission");
            assert!(first.dirty);
            plan.submitted();
            for floor in [-0.3, -0.1, -0., 0., 0.125] {
                frame[5][3] = floor;
                let update = plan.update(&frame[..1024]);
                assert_eq!(update.ready_count, 2, "{name} floor-only admission");
                assert!(!update.dirty, "{name} floor-only change rebaked engraving");
            }
            for channel in 0..3 {
                frame[5][channel] = f32::from_bits(frame[5][channel].to_bits() ^ 1);
                let update = plan.update(&frame[..1024]);
                assert_eq!(update.ready_count, 2);
                assert!(update.dirty, "{name} lost ambient bit dependency {channel}");
                plan.submitted();
                assert!(!plan.update(&frame[..1024]).dirty);
            }
        }
    }

    #[test]
    fn workstation_ruby_plan_waits_for_submission_and_invalidates_shape_optics_and_budgets() {
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
    fn workstation_ruby_plan_keeps_lighting_engraving_viewport_and_camera_translation_live() {
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
    fn workstation_ruby_plan_reuses_translation_and_updates_pose_without_clearing_pending_bake() {
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
        // Several edits each smaller than the comparison tolerance must still
        // invalidate once their total change exceeds the original bake bound.
        let mut invalidated = false;
        for _ in 0..64 {
            f[38][3] += 2. * f32::EPSILON;
            if plan.update(&f).dirty { invalidated = true; break; }
        }
        assert!(invalidated, "plane comparison drifted with each accepted frame");
    }
    #[test]
    fn workstation_ruby_plan_reuses_control_translation_and_glyph_edits_but_not_scale() {
        let mut f=control_fixture();let mut plan=Plan::new(&f);
        assert_eq!(plan.update(&f).ready_count,2);plan.submitted();
        f[192]=[0.25,-0.125,0.2,1.];f[602][3]=700.;f[700]=[3.,4.,5.,6.];f[193][2]=4.;
        let ready=plan.update(&f);assert!(!ready.dirty);
        assert_eq!(ready.rows[2+4*8],f[192]);
        f[192][3]=1.1;assert!(plan.update(&f).dirty);
    }
    #[test]
    fn workstation_ruby_plan_rejects_camera_inside_component() {
        let mut f=control_fixture();let mut plan=Plan::new(&f);plan.update(&f);plan.submitted();
        f[1][2]=0.05;assert_eq!(plan.update(&f).ready_count,0);
    }
    fn timeline_fixture() -> Vec<[f32;4]> {
        let mut f = fixture();f.resize(30_000,[0.;4]);
        f[2][3] = 640. * (1.3 / 480.) * 0.5;f[3][3] = 0.65;
        f[248] = [1.,800.,820.,1.];f[249] = [-0.8,-0.6,0.8,0.6];
        f[800] = [0.,0.,0.05,24. * (1.3 / 480.) / 0.022];
        f[801] = [1140.,1143.,1100.,0.];f[802] = [0.022,0.022,0.01265,9. * (1.3 / 480.)];
        f[820] = [32.,0.,0.,0.];f[224] = [2048.,128.,1.2,21846.];
        f[1100] = f[32];f[1101] = f[33];f[1103] = [0.001,0.001,0.001,0.];
        f[1140] = [-0.011,-0.011,-0.01265,1143.];
        f[1141] = [0.011,0.011,0.,1143.];f[1142] = [1.,0.,0.,0.];
        f[1143] = [-0.011,-0.011,0.,0.];f[1144] = [0.022,0.,0.,1.];
        f[1145] = [0.022,0.022,0.,0.];
        // A second immutable copy permits testing a valid tree/material change.
        for offset in 0..40 { f[1210+offset] = f[1100+offset]; }
        for offset in 0..9 { f[1250+offset] = f[1140+offset]; }
        f[1250][3] = 1253.;f[1251][3] = 1253.;
        f
    }

    #[test]
    fn timeline_pixel_map_is_reserved_and_cannot_alias_ordinary_targets() {
        assert_eq!(map_target(16643),Some(16643));
        assert_eq!(map_target(16644),None);
        assert_eq!(map_target(699999),None);
        assert_eq!(map_target(700000),Some(16644));
        assert_eq!(map_target(704095),Some(20739));
        assert_eq!(map_target(704096),None);
        assert_eq!(MAP_BASE + map_target(704095).unwrap()/4,PLAN_ROWS-1);
        let f=timeline_fixture();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        let update=plan.update(&f[..1024]);
        assert_eq!(update.ready_count,2);
        let slot=update.rows[MAP_BASE+16644/4][16644%4] as usize;
        assert!((4..20).contains(&slot));
        let descriptor=&update.rows[1+slot*8..1+slot*8+8];
        assert_eq!(descriptor[0][0],700000.);assert_eq!(descriptor[4][3],1.);
        assert!(descriptor[6][0] >= descriptor[4][0]*descriptor[4][1]*108.);
        assert!(descriptor[0][2]+descriptor[6][0] <= CACHE_ROWS as f32);
    }

    #[test]
    fn timeline_pixel_keys_every_live_text_and_geometry_dependency() {
        let mut f=timeline_fixture();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        plan.update(&f[..1024]);plan.submitted();
        for (at,channel,delta) in [(800,0,0.001),(800,1,0.001),(800,2,0.001),(800,3,0.001),
            (802,0,0.00001),(802,1,0.00001),(802,2,0.00001),(802,3,0.001),
            (249,0,-0.01),(820,0,1.),(224,2,0.01),
            (5,0,0.1),(16,0,0.1),(17,0,0.1),(18,0,0.1),(19,0,0.1),(20,0,0.1),
            (21,0,0.1),(22,0,0.1),(23,0,0.1),(24,0,0.1),(25,0,0.1),(26,0,0.1),
            (27,0,0.1),(28,0,0.1),(29,0,0.1),(30,0,0.1),(213,0,0.1),(214,0,0.1),
            (215,0,0.1),(216,0,0.1),(223,0,0.1),(1,0,0.001),(1,2,0.001),
            (2,3,0.001),(3,3,0.001),(8,0,1.),(8,1,1.)] {
            f[at][channel]+=delta;
            let update=plan.update(&f[..1024]);
            assert_eq!(update.ready_count,2,"dependency {at}:{channel} rejected fixture");
            assert!(update.dirty,"timeline omitted {at}:{channel}");
            plan.submitted();assert!(!plan.update(&f[..1024]).dirty);
        }
        f[801]=[1250.,1253.,1210.,0.];
        assert!(plan.update(&f[..1024]).dirty);
    }

    #[test]
    fn timeline_count_changes_preserve_active_handles_and_admit_new_ones() {
        let mut f=timeline_fixture();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        let first=plan.update(&f[..1024]);assert_eq!(first.ready_count,2);plan.submitted();
        let mapping=|update:&Update,instance:usize| {
            let target=map_target(TIMELINE_FIRST+instance*16).unwrap();
            let slot=update.rows[MAP_BASE+target/4][target%4];
            (slot>=0.).then_some(slot as usize)
        };
        let first_slot=mapping(&first,0).unwrap();
        // Extra currently invalid instances must not dirty the existing one.
        for (count,handles) in [(2.,1.),(12.,1.),(12.,3.),(5.,2.),(1.,1.)] {
            f[248][0]=count;f[248][3]=handles;
            let update=plan.update(&f[..1024]);
            assert_eq!(update.ready_count,2);assert!(!update.dirty);
            assert_eq!(mapping(&update,0),Some(first_slot));
            assert_eq!(update.rows[1+first_slot*8][3],1.);
        }
        // A valid new instance still needs its own bake and allocation.
        for offset in 0..3 { f[803+offset]=f[800+offset]; }
        f[803][0]+=0.1;f[822]=f[820];f[823]=f[821];
        f[248][0]=2.;f[248][3]=2.;
        let added=plan.update(&f[..1024]);assert_eq!(added.ready_count,3);assert!(added.dirty);
        assert_eq!(mapping(&added,0),Some(first_slot));
        assert_eq!(added.rows[1+first_slot*8][3],1.);
        let new_slot=mapping(&added,1).unwrap();assert_ne!(new_slot,first_slot);
        assert_eq!(added.rows[1+new_slot*8][3],2.);plan.submitted();
        f[248][3]=1.;
        let removed=plan.update(&f[..1024]);assert_eq!(removed.ready_count,2);
        assert!(!removed.dirty);assert_eq!(mapping(&removed,1),None);
        assert_eq!(mapping(&removed,0),Some(first_slot));
        f[248][3]=2.;
        let restored=plan.update(&f[..1024]);assert_eq!(restored.ready_count,3);
        let restored_slot=mapping(&restored,1).unwrap();
        assert_eq!(restored.rows[1+restored_slot*8][3],2.);
        assert!(restored.dirty);
    }

    #[test]
    fn timeline_count_normalization_preserves_rejection_and_pointer_dependencies() {
        let original=timeline_fixture();
        for (count,handles) in [(0.,0.),(1.,0.),(1.,2.),(257.,1.),(-1.,1.),(1.,0.5)] {
            let mut f=original.clone();let mut plan=Plan::new(&f);plan.pixel_transport=true;
            plan.update(&f[..1024]);plan.submitted();
            f[248][0]=count;f[248][3]=handles;
            let rejected=plan.update(&f[..1024]);assert_eq!(rejected.ready_count,1);
            assert_eq!(rejected.rows[MAP_BASE+TIMELINE_MAP/4][TIMELINE_MAP%4],-1.);
            f[248]=original[248];
            let restored=plan.update(&f[..1024]);
            assert_eq!(restored.ready_count,2);assert!(restored.dirty);
        }
        let mut f=original.clone();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        plan.update(&f[..1024]);plan.submitted();
        for offset in 0..3 { f[840+offset]=f[800+offset]; }
        f[248][1]=840.;
        let moved=plan.update(&f[..1024]);assert_eq!(moved.ready_count,2);assert!(moved.dirty);
        plan.submitted();
        f[860]=f[820];f[861]=f[821];f[248][2]=860.;
        let labels=plan.update(&f[..1024]);assert_eq!(labels.ready_count,2);assert!(labels.dirty);
    }

    #[test]
    fn timeline_pixel_rejects_tape_clipped_mixed_or_mutable_inputs() {
        let original=timeline_fixture();
        for (at,channel,value) in [(801,3,1.),(248,3,0.),(800,3,0.),(249,2,0.),
            (224,1,64.),(224,3,100.),(820,0,127.)] {
            let mut f=original.clone();f[at][channel]=value;
            let mut plan=Plan::new(&f);plan.pixel_transport=true;
            assert_eq!(plan.update(&f[..1024]).ready_count,1,"accepted {at}:{channel}");
        }
        let mut f=original.clone();f[1142][0]=2.;
        f[1149]=f[1143];f[1149][3]=1.;f[1150]=f[1144];f[1151]=f[1145];
        let mut plan=Plan::new(&f);plan.pixel_transport=true;
        assert_eq!(plan.update(&f[..1024]).ready_count,1);
        let mut plan=Plan::new(&original);plan.pixel_transport=true;
        assert_eq!(plan.update(&original).ready_count,1);
        let mut plan=Plan::new(&original);plan.pixel_transport=false;
        assert_eq!(plan.update(&original[..1024]).ready_count,1);
    }

    #[test]
    fn timeline_authored_pixel_sizes_fit_variable_spans_without_more_gpu_storage() {
        let mut f=fixture();f[2][3]=320.;f[3][3]=240.;
        let sizes=[(28. * 0.08 / 0.03,28.),(24.,24.),(24.,24.),(44.,44. * 0.03 / 0.08)];
        let mut occupied=Vec::new();
        for ratio in [1.,2.] {
            f[8][0]=640.*ratio;f[8][1]=480.*ratio;
            let plan=Plan::new(&f);
            for (n,&(width,height)) in sizes.iter().enumerate() {
                let candidate=plan.make_candidate(&f,&[],700000+n*16,32,[0.,0.,0.,1.],
                    [-width*0.5,-height*0.5,width*0.5,height*0.5],64.);
                let candidate=plan.pixel_candidate(&f,candidate).unwrap();
                let pixels=candidate.descriptor[4][0] as usize*candidate.descriptor[4][1] as usize;
                assert!(pixels<=16384);
                if n==0 || ratio==2. { assert!(pixels>2275); }
                if ratio==2. { assert!(pixel_span(&mut occupied,None,pixels*108).is_some()); }
            }
        }
        assert_eq!(CACHE_BYTES,120*1024*1024);
        assert_eq!(CONTROL_BASE+16*CONTROL_ROWS,CACHE_ROWS);
        assert!(occupied.iter().map(|span| span.1).sum::<usize>() < 16*CONTROL_ROWS);
    }

    #[test]
    fn pixel_pool_preserves_legacy_blocks_and_reallocation_requires_bake() {
        let legacy:Vec<_>=(0..8).map(|i|(CONTROL_BASE+i*CONTROL_ROWS,CONTROL_ROWS)).collect();
        let mut occupied=legacy.clone();
        let span=pixel_span(&mut occupied,None,2640*108).unwrap();
        assert_eq!(span.0,CONTROL_BASE+8*CONTROL_ROWS);
        let mut occupied=legacy.clone();
        assert_eq!(pixel_span(&mut occupied,Some(span),2000*108),Some(span));
        assert!(pixel_span(&mut occupied,None,CACHE_ROWS-CONTROL_BASE).is_none());
        let f=timeline_fixture();let mut plan=Plan::new(&f);plan.pixel_transport=true;
        let common=plan.common_key(&f[..1024]);
        let candidate=plan.timeline_candidate(&f[..1024],&common,0).unwrap();
        plan.place_at(4,candidate,span.0,span.1);plan.submitted();
        let candidate=plan.timeline_candidate(&f[..1024],&common,0).unwrap();
        plan.place_at(4,candidate,span.0,span.1);
        assert!(!plan.slots[4].as_ref().unwrap().dirty);
        let candidate=plan.timeline_candidate(&f[..1024],&common,0).unwrap();
        plan.place_at(4,candidate,span.0+span.1,span.1);
        assert!(plan.slots[4].as_ref().unwrap().dirty);
    }

    #[test]
    fn pixel_pool_relocates_when_new_legacy_storage_occupies_an_old_span() {
        let old=(CONTROL_BASE,2640*108);
        let mut occupied=vec![(CONTROL_BASE,CONTROL_ROWS)];
        let moved=pixel_span(&mut occupied,Some(old),2640*108).unwrap();
        assert_eq!(moved.0,CONTROL_BASE+CONTROL_ROWS);
        assert!(occupied.windows(2).all(|pair| pair[0].0+pair[0].1 <= pair[1].0));
        let mut full:Vec<_>=(0..16).map(|i|(CONTROL_BASE+i*CONTROL_ROWS,CONTROL_ROWS)).collect();
        assert_eq!(pixel_span(&mut full,None,108),None);
        let f=fixture();let plan=Plan::new(&f);
        let legacy=plan.make_candidate(&f,&[],4,32,[0.,0.,0.,1.],[-0.1,-0.1,0.1,0.1],64.);
        let mut timeline=plan.make_candidate(&f,&[],700000,32,[0.,0.,0.,1.],[-0.1,-0.1,0.1,0.1],64.);
        timeline.descriptor[4][3]=1.;
        let mut optional=plan.make_candidate(&f,&[],1028,32,[0.,0.,0.,1.],[-0.1,-0.1,0.1,0.1],64.);
        optional.descriptor[4][3]=1.;
        assert!(candidate_priority(&legacy)<candidate_priority(&timeline));
        assert!(candidate_priority(&timeline)<candidate_priority(&optional));
    }


    // Diagnostic toggle is set directly: no process-global environment races.
    fn cache_cause_plans(initial: &[[f32;4]], pixel: bool) -> (Plan, Plan) {
        let mut plain=Plan::new(initial); let mut observed=Plan::new(initial);
        plain.pixel_transport=pixel; observed.pixel_transport=pixel;
        plain.cache_causes=None;
        observed.cache_causes=Some(cache_causes::CacheCauses::new());
        (plain,observed)
    }

    fn cache_cause_pair(plain: &mut Plan, observed: &mut Plan, frame: &[[f32;4]]) -> Update {
        let a=plain.update(frame); let b=observed.update(frame);
        assert_eq!((a.entry_count,a.ready_count,a.dirty),(b.entry_count,b.ready_count,b.dirty));
        assert_eq!(a.rows.len(),b.rows.len());
        for (index,(left,right)) in a.rows.iter().zip(&b.rows).enumerate() {
            assert_eq!(left.map(f32::to_bits),right.map(f32::to_bits),"diagnostic changed row {index}");
        }
        assert!(plain.cache_causes().is_none());
        b
    }

    fn cache_cause_record(plan: &Plan, target: usize) -> &serde_json::Value {
        plan.cache_causes().unwrap()["records"].as_array().unwrap().iter()
            .find(|record|record["target"].as_u64()==Some(target as u64)).unwrap()
    }

    #[test]
    fn cache_cause_replay_preserves_outputs_and_distinguishes_pending_from_new_dirty() {
        let mut frame=control_fixture();
        let (mut plain,mut observed)=cache_cause_plans(&frame,false);
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        let record=cache_cause_record(&observed,0);
        assert_eq!(record["decision"],"new");
        assert_eq!(record["newly_dirty"],true);
        assert_eq!(record["pending_inherited"],false);
        assert_eq!(record["old_key_words"],0);
        assert_eq!(record["new_key_words"],172);

        // An unchanged pending bake remains pending; observing it cannot acknowledge it.
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        let record=cache_cause_record(&observed,0);
        assert_eq!(record["decision"],"retained");
        assert_eq!(record["newly_dirty"],false);
        assert_eq!(record["pending_inherited"],true);
        assert_eq!(record["changed_key_word_count"],0);

        plain.submitted(); observed.submitted();
        assert!(!cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        assert_eq!(cache_cause_record(&observed,0)["pending_inherited"],false);
        assert_eq!(cache_cause_record(&observed,0)["dirty_after_placement"],false);
        translate_panel(&mut frame,[0.125,-0.25,0.125]);
        frame[192][0]+=0.125;
        assert!(!cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        for target in [0,4] {
            let record=cache_cause_record(&observed,target);
            assert_eq!(record["decision"],"retained");
            assert_eq!(record["changed_key_word_count"],0);
            assert_eq!(record["translated_plane_mismatch"],false);
        }

        // The panel key normalizes plane distance, so this is a plane-only cause.
        frame[38][3]+=0.01;
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        let record=cache_cause_record(&observed,0);
        assert_eq!(record["changed_key_word_count"],0);
        assert_eq!(record["translated_plane_mismatch"],true);
        assert_eq!(record["decision"],"replaced");
        plain.submitted(); observed.submitted();

        let old=frame[32][2].to_bits(); frame[32][2]=0.019;
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        let record=cache_cause_record(&observed,0);
        assert_eq!(record["decision"],"replaced");
        assert_eq!(record["newly_dirty"],true);
        assert_eq!(record["pending_inherited"],false);
        assert_eq!(record["changed_key_word_count"],1);
        assert_eq!(record["first_changed_key_words"],serde_json::json!([[14,old,frame[32][2].to_bits()]]));

        // Disabled output must not silently clear the still-pending entry.
        frame[1][3]=1.;
        assert_eq!(cache_cause_pair(&mut plain,&mut observed,&frame).ready_count,0);
        let report=observed.cache_causes().unwrap();
        assert_eq!(report["status"],"plan_disabled_no_cache_decisions");
        assert!(report["records"].as_array().unwrap().is_empty());
        frame[1][3]=0.;
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        assert_eq!(cache_cause_record(&observed,0)["pending_inherited"],true);
        plain.submitted(); observed.submitted();
        assert!(!cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
    }

    #[test]
    fn cache_cause_budget_exhaustion_keeps_cache_work_and_drops_observation_payloads() {
        let mut frame=fixture();
        let (mut plain,mut observed)=cache_cause_plans(&frame,false);
        for update in 1..=256 {
            let result=cache_cause_pair(&mut plain,&mut observed,&frame);
            assert_eq!(result.dirty,update==1);
            let report=observed.cache_causes().unwrap();
            assert_eq!(report["update_index"],update);
            assert_eq!(report["update_limit"],256);
            assert_eq!(report["status"],"observed");
            assert_eq!(report["record_count"],1);
            plain.submitted(); observed.submitted();
        }
        frame[32][2]=0.019;
        assert!(cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        let report=observed.cache_causes().unwrap();
        assert_eq!(report["status"],"sample_budget_exhausted");
        assert_eq!(report["sampled_updates"],256);
        assert_eq!(report["dropped_updates"],1);
        assert!(report["records"].as_array().unwrap().is_empty());
        assert!(report.get("key_schema").is_none());
        plain.submitted(); observed.submitted();
        assert!(!cache_cause_pair(&mut plain,&mut observed,&frame).dirty);
        assert_eq!(observed.cache_causes().unwrap()["dropped_updates"],2);
    }

    #[test]
    fn cache_cause_layouts_track_actual_variable_control_and_timeline_keys() {
        let mut frame=pixel_fixture(); frame.resize(2048,[0.;4]);
        frame[602][3]=700.; frame[700]=[-0.1,-0.1,0.2,0.2];
        frame[701]=[-0.01,-0.02,-0.03,0.02]; frame[702]=[4.,1.,704.,0.];
        frame[244]=[1200.,10.,0.,0.];
        for mip in 0..11 { frame[704+mip]=[2.,2.,(4*mip) as f32,0.]; }
        let (mut plain,mut observed)=cache_cause_plans(&frame,true);
        for mips in 1..=11 {
            frame[702][1]=mips as f32;
            assert_eq!(cache_cause_pair(&mut plain,&mut observed,&frame[..1024]).ready_count,2);
            let record=cache_cause_record(&observed,1028);
            assert_eq!(record["new_key_words"],312+4*mips);
            assert_eq!(record["new_key_layout"]["recognized"],true);
            assert_eq!(record["new_key_layout"]["mip_rows"],mips);
            assert_eq!(record["new_key_layout"]["light_rows_start"],224+4*mips);
            plain.submitted(); observed.submitted();
            assert!(!cache_cause_pair(&mut plain,&mut observed,&frame[..1024]).dirty);
        }

        let mut frame=timeline_fixture();
        let (mut plain,mut observed)=cache_cause_plans(&frame,true);
        for glyphs in 0..=8 {
            frame[820]=[0.;4]; frame[821]=[0.;4];
            for glyph in 0..glyphs { frame[820+glyph/4][glyph%4]=32.; }
            assert_eq!(cache_cause_pair(&mut plain,&mut observed,&frame[..1024]).ready_count,2);
            let record=cache_cause_record(&observed,700_000);
            assert_eq!(record["new_key_words"],320+4*glyphs);
            assert_eq!(record["new_key_layout"]["recognized"],true);
            assert_eq!(record["new_key_layout"]["glyph_reference_rows"],glyphs);
            assert_eq!(record["new_key_layout"]["own_rows_start"],204+4*glyphs);
            assert_eq!(record["new_key_layout"]["light_rows_start"],232+4*glyphs);
            if glyphs>0 {
                assert_eq!(record["old_key_layout"]["glyph_reference_rows"],glyphs-1);
                assert!(record["changed_key_word_count"].as_u64().unwrap()>0);
            }
            plain.submitted(); observed.submitted();
            assert!(!cache_cause_pair(&mut plain,&mut observed,&frame[..1024]).dirty);
        }
    }
}
