//! Pure ownership/admission rules for one GPU ledger generation. No GPU API.
use super::meter_ledger_plan::{Layout, Reject, CAP_BYTES, UNMAPPED};

pub(super) const SLOT_BYTES: u64 = 784; // header16 + sixteen48-byte terms
pub(super) const JOB_BYTES: u64 = 16;
pub(super) const DUMMY_BYTES: u64 = SLOT_BYTES + 4;
/// Same order as Client::set_batch's two four-float slices per vertex.
pub(super) fn vertex_content_changed(previous:&[f32],triangles:&[[f32;24]])->bool {
    if previous.len()!=triangles.len()*24{return true;}
    let mut i=0;
    for triangle in triangles { for vertex in 0..3 {
        for v in triangle[vertex*4..vertex*4+4].iter().chain(&triangle[12+vertex*4..16+vertex*4]) {
            if previous[i].to_bits()!=v.to_bits(){return true;}i+=1;
        }
    }}false
}
/// Only the separately drawn camera prefix RGB is absent from optical capture
/// bindings. Every coverage/depth/tag bit and the complete authored fullscreen
/// tail must stay identical. The caller still uploads and draws all new colors.
pub(super) fn camera_rgb_change_only(previous:&[f32],triangles:&[[f32;24]])->bool {
    if previous.len()!=triangles.len()*24 || !super::camera_split::retained_batch(triangles)
        || previous.iter().any(|v|!v.is_finite())
        || triangles.iter().flatten().any(|v|!v.is_finite()) {return false;}
    let camera_count=triangles.len()-2;let mut changed=false;let mut i=0;
    for (t,triangle) in triangles.iter().enumerate(){for vertex in 0..3 {
        for (lane,v) in triangle[vertex*4..vertex*4+4].iter().chain(&triangle[12+vertex*4..16+vertex*4]).enumerate(){
            if previous[i].to_bits()!=v.to_bits(){
                if t>=camera_count || !(4..=6).contains(&lane){return false;}
                changed=true;
            }i+=1;
        }
    }}changed
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub binding_bytes: u64, pub buffer_bytes: u64,
    pub storage_per_stage: u32, pub workgroups: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Budget {
    pub slots: u64, pub jobs: u64, pub map: u64, pub total: u64,
    pub samples: u32, pub groups: u32,
}
impl Budget {
    pub(super) fn new(width: u32, height: u32, pixels: u32, limits: Limits) -> Result<Self, Reject> {
        if width == 0 || height == 0 || pixels == 0 { return Err(Reject::Viewport); }
        let viewport = u64::from(width).checked_mul(u64::from(height)).ok_or(Reject::Arithmetic)?;
        if u64::from(pixels) > viewport || viewport >= u64::from(UNMAPPED) { return Err(Reject::Viewport); }
        let samples = pixels.checked_mul(2).ok_or(Reject::Arithmetic)?;
        // No last sample may alias the full-map unmapped sentinel.
        if samples == UNMAPPED { return Err(Reject::Arithmetic); }
        let slots = u64::from(samples).checked_mul(SLOT_BYTES).ok_or(Reject::Arithmetic)?;
        let jobs = u64::from(samples).checked_mul(JOB_BYTES).ok_or(Reject::Arithmetic)?;
        let map = viewport.checked_mul(4).ok_or(Reject::Arithmetic)?;
        let total = slots.checked_add(jobs).and_then(|n|n.checked_add(map))
            .and_then(|n|n.checked_add(DUMMY_BYTES)).ok_or(Reject::Arithmetic)?;
        let groups = samples.div_ceil(64);
        if total > CAP_BYTES || limits.storage_per_stage < 6 || groups > limits.workgroups
            || [slots, jobs, map, SLOT_BYTES].iter().any(|n|*n > limits.binding_bytes || *n > limits.buffer_bytes)
        { return Err(Reject::Capacity); }
        Ok(Self { slots, jobs, map, total, samples, groups })
    }
}

/// Map values are BASE SLOT indices, not compact pixel indices. Job order is
/// identical to Layout's disjoint region order, then y/x, then sample0/sample1.
pub(super) fn uploads(layout: &Layout, budget: Budget) -> Result<(Vec<u32>, Vec<[u32;4]>), Reject> {
    if budget.samples != layout.pixels.checked_mul(2).ok_or(Reject::Arithmetic)?
        || budget.map != u64::from(layout.width)*u64::from(layout.height)*4
    { return Err(Reject::Arithmetic); }
    let mut map = layout.build_pixel_map()?;
    for value in &mut map { if *value != UNMAPPED { *value = value.checked_mul(2).ok_or(Reject::Arithmetic)?; } }
    let mut jobs = Vec::new();
    jobs.try_reserve_exact(budget.samples as usize).map_err(|_|Reject::Allocation)?;
    for region in &layout.regions {
        let r=region.rect;
        for y in r.y..r.y+r.height { for x in r.x..r.x+r.width {
            jobs.push([x,y,0,0]); jobs.push([x,y,1,0]);
        }}
    }
    if jobs.len()!=budget.samples as usize { return Err(Reject::Arithmetic); }
    Ok((map,jobs))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Encoded, Submitted(u64), Complete }
struct Generation<K> { key: K, phase: Phase, stale: bool }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Decision { pub release: bool, pub build: bool, pub cached: bool }
pub(super) struct State<K> {
    stable: Option<(K,u8)>, generation: Option<Generation<K>>, blocked: Option<K>, selected: bool,
}
impl<K> Default for State<K> {
    fn default()->Self { Self { stable:None,generation:None,blocked:None,selected:false } }
}
impl<K: Clone+Eq> State<K> {
    /// May run while the previous submission is in flight. It only deselects;
    /// the allocation remains owned until observe sees its completion serial.
    pub(super) fn invalidate(&mut self) {
        self.stable=None; self.blocked=None; self.selected=false;
        if let Some(g)=&mut self.generation { g.stale=true; }
    }
    pub(super) fn selected(&self)->bool { self.selected }
    pub(super) fn deselect(&mut self) { self.selected=false; }
    pub(super) fn observe(&mut self, key:Option<K>, completed:u64)->Decision {
        self.selected=false;
        let mut d=Decision::default();
        if let Some(g)=&mut self.generation {
            if matches!(g.phase,Phase::Submitted(serial) if serial<=completed) { g.phase=Phase::Complete; }
            if key.as_ref()!=Some(&g.key) { g.stale=true; }
            if g.stale && g.phase==Phase::Complete { self.generation=None; d.release=true; }
        }
        let Some(key)=key else { self.stable=None;self.blocked=None;return d; };
        if self.blocked.as_ref().is_some_and(|k|k!=&key) { self.blocked=None; }
        let count=match &self.stable { Some((k,n)) if k==&key => n.saturating_add(1).min(2),_=>1 };
        self.stable=Some((key.clone(),count));
        if let Some(g)=&self.generation {
            self.selected=!g.stale&&g.key==key&&g.phase==Phase::Complete;
            d.cached=self.selected; return d;
        }
        if count>=2&&self.blocked.as_ref()!=Some(&key) {
            self.generation=Some(Generation{key,phase:Phase::Encoded,stale:false}); d.build=true;
        }
        d
    }
    pub(super) fn submitted(&mut self,serial:u64)->Result<(), &'static str> {
        match &mut self.generation {
            Some(g) if g.phase==Phase::Encoded&&serial>0 => {g.phase=Phase::Submitted(serial);Ok(())},
            _=>Err("meter ledger submitted without one encoded generation"),
        }
    }
    /// Allocation/planning failure occurred before any commands referencing the
    /// generation were encoded. Block retries of this exact key until changed.
    pub(super) fn failed_before_encode(&mut self) {
        if self.generation.as_ref().is_some_and(|g|g.phase==Phase::Encoded) {
            self.blocked=self.generation.take().map(|g|g.key); self.selected=false;
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn limits()->Limits { Limits{binding_bytes:128<<20,buffer_bytes:256<<20,storage_per_stage:8,workgroups:65535} }
    #[test] fn exact_gpu_budget_includes_jobs_map_dummy_and_header16() {
        let b=Budget::new(1920,1200,34272,limits()).unwrap();
        assert_eq!((b.slots,b.jobs,b.map,b.total),(53_738_496,1_096_704,9_216_000,64_051_988));
        assert_eq!((b.samples,b.groups),(68_544,1071));
        assert!(Budget::new(1920,1200,40000,limits()).is_err());
        for l in [Limits{binding_bytes:32<<20,..limits()},Limits{buffer_bytes:32<<20,..limits()},
            Limits{storage_per_stage:5,..limits()},Limits{workgroups:1070,..limits()}] {
            assert!(Budget::new(1920,1200,34272,l).is_err());
        }
        assert!(Budget::new(u32::MAX,u32::MAX,u32::MAX,limits()).is_err());
    }
    #[test] fn overlapping_union_maps_pixel_to_its_two_slots() {
        use super::super::meter_regions::Rect;
        let l=Layout::new(10,8,&[Rect{x:1,y:2,width:4,height:3},Rect{x:3,y:1,width:3,height:4}]).unwrap();
        let b=Budget::new(l.width,l.height,l.pixels,limits()).unwrap();let(map,jobs)=uploads(&l,b).unwrap();
        let mut mapped=0;
        for y in 0..8 { for x in 0..10 {let slot=map[(y*10+x)as usize];
            if slot!=UNMAPPED {mapped+=1;assert_eq!(slot%2,0);assert_eq!(jobs[slot as usize],[x,y,0,0]);assert_eq!(jobs[slot as usize+1],[x,y,1,0]);}
        }}
        assert_eq!(mapped,l.pixels);assert_eq!(jobs.len(),2*mapped as usize);
    }
    #[test] fn two_stable_frames_and_completion_and_key_check_are_all_required() {
        let mut s=State::default();assert!(!s.observe(Some(7),0).build);
        assert!(s.observe(Some(7),0).build);assert!(!s.selected());s.submitted(3).unwrap();
        assert!(!s.observe(Some(7),2).cached);assert!(s.observe(Some(7),3).cached);
        assert!(!s.observe(Some(8),3).cached);assert!(!s.selected());
        assert!(s.observe(Some(8),3).build);
    }
    #[test] fn moving_scene_does_not_allocate_a_generation() {
        let mut s=State::default();for key in 0..1000 {assert!(!s.observe(Some(key),0).build);}
        assert!(s.observe(Some(999),0).build);
    }
    #[test] fn invalidation_cannot_publish_stale_capture_or_release_inflight() {
        let mut s=State::default();s.observe(Some(1),0);s.observe(Some(1),0);s.submitted(12).unwrap();
        s.invalidate();let d=s.observe(Some(1),11);assert!(!d.release&&!d.cached&&!d.build);
        let d=s.observe(Some(1),12);assert!(d.release&&d.build&&!d.cached);
        s.submitted(13).unwrap();assert!(s.observe(Some(1),13).cached);
        s.invalidate();assert!(!s.selected());assert!(s.observe(None,13).release);
    }
    #[test] fn interrupted_eligibility_requires_two_new_stable_draws() {
        let mut s=State::default();s.observe(Some(1),0);s.observe(None,0);
        assert!(!s.observe(Some(1),0).build);assert!(s.observe(Some(1),0).build);
    }
    #[test] fn failed_allocation_does_not_retry_same_key_each_frame() {
        let mut s=State::default();s.observe(Some(1),0);s.observe(Some(1),0);s.failed_before_encode();
        for _ in 0..100 {assert!(!s.observe(Some(1),0).build);}
        assert!(!s.observe(Some(2),0).build);assert!(s.observe(Some(2),0).build);
    }
    #[test] fn identical_upload_does_not_invalidate_but_any_converted_lane_does() {
        let triangles=[[std::f32::consts::PI;24],[0.;24]];
        let mut previous=vec![];
        for t in &triangles {for v in 0..3 {previous.extend_from_slice(&t[v*4..v*4+4]);previous.extend_from_slice(&t[12+v*4..16+v*4]);}}
        assert!(!vertex_content_changed(&previous,&triangles));
        for lane in 0..24 {let mut changed=triangles;changed[1][lane]=-0.;assert!(vertex_content_changed(&previous,&changed));}
        assert!(vertex_content_changed(&previous,&triangles[..1]));
    }

}

#[cfg(test)]mod camera_rgb_tests{
 use super::*;
 fn fixture()->Vec<[f32;24]>{
  let mut camera=[0.;24];for i in 0..3{camera[i*4..i*4+4].copy_from_slice(&[i as f32,0.,0.8,1.]);camera[12+i*4..16+i*4].copy_from_slice(&[0.1,0.2,0.3,90.]);}
  let mut ts=vec![camera,camera,[0.;24],[0.;24]];
  for(t,cs)in ts[2..].iter_mut().zip([[[-1.,-1.],[1.,-1.],[1.,1.]],[[-1.,-1.],[1.,1.],[-1.,1.]]]){for(i,p)in cs.iter().enumerate(){t[i*4..i*4+4].copy_from_slice(&[p[0],p[1],0.5,1.]);}}ts
 }
 fn uploaded(ts:&[[f32;24]])->Vec<f32>{let mut p=vec![];for t in ts{for v in 0..3{p.extend_from_slice(&t[v*4..v*4+4]);p.extend_from_slice(&t[12+v*4..16+v*4]);}}p}
 #[test]fn only_camera_rgb_lanes_are_independent(){
  let ts=fixture();let p=uploaded(&ts);assert!(!camera_rgb_change_only(&p,&ts));
  for t in 0..ts.len(){for lane in 0..24{let mut b=ts.clone();b[t][lane]+=0.001;assert!(vertex_content_changed(&p,&b));assert_eq!(camera_rgb_change_only(&p,&b),t<2&&lane>=12&&lane%4!=3,"{t}/{lane}");}}
 }
 #[test]fn nonfinite_colors_changed_order_count_and_signed_zero_geometry_reject(){
  let ts=fixture();let p=uploaded(&ts);
  for v in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY]{let mut b=ts.clone();b[0][12]=v;assert!(!camera_rgb_change_only(&p,&b));let mut old=p.clone();old[4]=v;assert!(!camera_rgb_change_only(&old,&ts));}
  let mut b=ts.clone();b[0][1]=-0.;b[0][12]+=0.1;assert!(!camera_rgb_change_only(&p,&b));
  let mut b=ts.clone();b.swap(0,2);assert!(!camera_rgb_change_only(&p,&b));
  assert!(!camera_rgb_change_only(&p,&ts[1..]));assert!(!camera_rgb_change_only(&p[1..],&ts));
  for tag in [89.,89.75,91.]{let mut b=ts.clone();b[0][15]=tag;assert!(!camera_rgb_change_only(&p,&b));}
 }
}
