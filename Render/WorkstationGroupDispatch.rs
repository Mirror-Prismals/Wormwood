//! Opt-in bounded synchronous dispatch. The original cs_retained body is unchanged.
use super::*;

const MAX_GROUPS: usize = 6;
pub(super) fn requested() -> bool {
    std::env::var("WORMWOOD_WORKSTATION_CAPTURE_BOUNDED_GROUPS").as_deref() == Ok("1")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dispatch {
    pub group: usize,
    pub workgroups: [u32; 3],
}

/// None keeps the original full-grid dispatch. Rect origins are deliberately
/// not clipped: offscreen captured texels may be used by later retained poses.
pub(super) fn plan(rows: &[[f32; 4]], width: u32, height: u32, groups: u32, group_y: u32)
    -> Option<Vec<Dispatch>>
{
    if width == 0 || height == 0 || !matches!(group_y, 4 | 8)
        || width > 65535 * 8 || height > 65535 * group_y
        || groups == 0 || groups as usize > MAX_GROUPS { return None; }
    let mut out = Vec::with_capacity(groups as usize);
    for group in 0..groups as usize {
        let r = rows.get(2 + group * 4)?;
        if r.iter().any(|x| !x.is_finite()) || r[2] < 0.0 || r[3] < 0.0
            || r[2].fract() != 0.0 || r[3].fract() != 0.0
            || r[2] > width as f32 || r[3] > height as f32 { return None; }
        let [w, h] = [r[2] as u32, r[3] as u32];
        if w == 0 || h == 0 { continue; }
        out.push(Dispatch { group, workgroups: [w.div_ceil(8), h.div_ceil(group_y), 2] });
    }
    Some(out)
}

pub(super) struct Groups {
    api: &'static Api,
    handles: Vec<WGPUBindGroup>,
}
impl Drop for Groups {
    fn drop(&mut self) { unsafe {
        for &g in &self.handles { (self.api.wgpuBindGroupRelease)(g); }
    } }
}

pub(super) struct Bindings {
    api: &'static Api,
    offsets: Vec<WGPUBuffer>,
    groups: Groups,
}
impl Bindings {
    pub(super) unsafe fn new(api: &'static Api, device: WGPUDevice, queue: WGPUQueue,
        layout: WGPUBindGroupLayout, entries: &[WGPUBindGroupEntry]) -> Result<Self, String>
    {
        let mut out = Self { api, offsets: Vec::with_capacity(MAX_GROUPS),
            groups: Groups { api, handles: Vec::new() } };
        for g in 0..MAX_GROUPS {
            let buffer = (api.wgpuDeviceCreateBuffer)(device, &WGPUBufferDescriptor {
                label: view("Immutable retained group dispatch offset"), size: 16,
                usage: WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst, ..Default::default()
            });
            if buffer.is_null() { return Err("retained group offset allocation failed".into()); }
            out.offsets.push(buffer);
            let offset = [0u32, 0, (g * 2) as u32, 0];
            // Written once, before any dispatch. Never rewrite a shared offset
            // between encoded dispatches: queue writes are not snapshots.
            (api.wgpuQueueWriteBuffer)(queue, buffer, 0, offset.as_ptr().cast(), 16);
        }
        out.groups = out.prepare_groups(device, layout, entries)?;
        Ok(out)
    }

    /// Build the complete replacement first. The caller commits it only after
    /// all new atlas bindings validate, preserving the old target on failure.
    pub(super) unsafe fn prepare_groups(&self, device: WGPUDevice,
        layout: WGPUBindGroupLayout, entries: &[WGPUBindGroupEntry]) -> Result<Groups, String>
    {
        if entries.iter().filter(|e| e.binding == 7).count() != 1 {
            return Err("bounded capture requires the incremental offset binding".into());
        }
        let mut out = Groups { api: self.api, handles: Vec::with_capacity(MAX_GROUPS) };
        for &offset in &self.offsets {
            let mut entries = entries.to_vec();
            let entry = entries.iter_mut().find(|e| e.binding == 7).unwrap();
            *entry = WGPUBindGroupEntry { binding: 7, buffer: offset, size: 16, ..Default::default() };
            let group = (self.api.wgpuDeviceCreateBindGroup)(device, &WGPUBindGroupDescriptor {
                label: view("Bounded retained group capture"), layout,
                entryCount: entries.len(), entries: entries.as_ptr(), ..Default::default()
            });
            if group.is_null() { return Err("retained group binding failed".into()); }
            out.handles.push(group);
        }
        Ok(out)
    }
    pub(super) fn commit_groups(&mut self, groups: Groups) { self.groups = groups; }
    pub(super) fn group(&self, index: usize) -> WGPUBindGroup { self.groups.handles[index] }
}
impl Drop for Bindings {
    fn drop(&mut self) { unsafe {
        // Drop group references before the buffers they bind.
        self.groups.handles.drain(..).for_each(|g| (self.api.wgpuBindGroupRelease)(g));
        for &b in &self.offsets { (self.api.wgpuBufferRelease)(b); }
    } }
}

#[cfg(test)] mod tests {
    use super::*;
    fn rows(sizes: &[[u32; 2]]) -> [[f32; 4]; 64] {
        let mut r = [[0.; 4]; 64];
        for (g, size) in sizes.iter().enumerate() {
            r[2 + g * 4] = [-31.5 + g as f32 * 2000., 987.25, size[0] as f32, size[1] as f32];
        }
        r
    }
    #[test] fn accepted_pixel_sample_ids_and_texture_targets_are_identical() {
        for gy in [4, 8] { for (w, h) in [(1, 1), (9, 7), (33, 19)] {
            let r = rows(&[[w,h],[w/2,h],[w,h/2],[0,h],[w,0],[1,1]]);
            let jobs = plan(&r,w,h,6,gy).unwrap();
            for group in 0..6 {
                let mut actual = std::collections::BTreeSet::new();
                if let Some(job) = jobs.iter().find(|j|j.group==group) {
                    for z in 0..job.workgroups[2] { for y in 0..job.workgroups[1]*gy { for x in 0..job.workgroups[0]*8 {
                        let id = [x,y,z+2*group as u32];
                        if x < r[2+4*group][2] as u32 && y < r[2+4*group][3] as u32 {
                            assert!(actual.insert(id));
                        }
                    } } }
                }
                let mut expected = std::collections::BTreeSet::new();
                for z in 2*group as u32..2*group as u32+2 { for y in 0..h.div_ceil(gy)*gy { for x in 0..w.div_ceil(8)*8 {
                    if x < r[2+4*group][2] as u32 && y < r[2+4*group][3] as u32 { expected.insert([x,y,z]); }
                } } }
                assert_eq!(actual,expected);
                // cs_retained derives group/sample and both compact/noncompact
                // output coordinates solely from this same original id.
            }
        } }
    }
    #[test] fn recorded_motion_rectangles_remove_only_early_return_work() {
        let r = rows(&[[1920,1200],[565,189],[1428,190],[202,588],[201,591],[565,189]]);
        let p = plan(&r,1920,1200,6,8).unwrap();
        assert_eq!(p.iter().map(|j|j.workgroups.iter().product::<u32>() as u64*64).sum::<u64>(),6_086_656);
        assert_eq!(1920u64*1200*12,27_648_000);
        assert_eq!(p.iter().map(|j|j.group).collect::<Vec<_>>(),vec![0,1,2,3,4,5]);
    }
    #[test] fn invalid_bounds_fail_closed() {
        let valid = rows(&[[10,10];6]);
        for value in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,-1.,10.5,11.] {
            let mut r=valid;r[10][2]=value;assert!(plan(&r,10,10,6,8).is_none());
        }
        let mut r=valid;r[2][0]=f32::NAN;assert!(plan(&r,10,10,6,8).is_none());
        for (w,h,g,gy) in [(0,10,6,8),(10,0,6,8),(10,10,0,8),(10,10,7,8),(10,10,6,3),(524281,10,6,8)] {
            assert!(plan(&valid,w,h,g,gy).is_none());
        }
        assert!(plan(&valid[..22],10,10,6,8).is_none());
    }
    #[test] fn empty_and_offscreen_groups_are_not_screen_clipped() {
        assert!(plan(&rows(&[[0,0];6]),10,10,6,8).unwrap().is_empty());
        let mut r=rows(&[[10,10];6]);r[2]=[1.0e20,-1.0e20,10.,10.];
        assert_eq!(plan(&r,10,10,6,8).unwrap()[0].workgroups,[2,2,2]);
        assert_eq!(plan(&r,10,10,5,4).unwrap().len(),5);
    }
}
