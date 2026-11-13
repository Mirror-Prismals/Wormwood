//! CPU-only planning. This does not establish optical replay equivalence.
//! Integrate beside Render's meter_regions and workstation modules. Build off
//! the audio callback. Publish only after GPU completion and a matching key.
use super::meter_regions;
use super::super::workstation;
use meter_regions::Rect;

pub(crate) const CAP_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const SAMPLES: u64 = 2;
pub(crate) const TERMS: u64 = 16;
pub(crate) const TERM_BYTES: u64 = 48;
pub(crate) const HEADER_BYTES: u64 = 8;
pub(crate) const UNMAPPED: u32 = u32::MAX;
const BASE: usize = 43_152;
const FIRST: usize = BASE + 8;
const STRIDE: usize = 6;
const MAX_METERS: usize = 70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reject {
    Packet, Dependencies, MeterLayout, CameraOrRegions, Viewport,
    Rectangle, Arithmetic, Capacity, Allocation, PlaybackEligibility,
}

/// Bit identity, not a lossy hash. Dependency words must cover shader/format,
/// every externally bound resource's identity AND content epoch, atlas metadata
/// and generation, plus vertex/camera data not carried in the packet. Empty
/// dependency tokens are rejected; completeness remains the caller's contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    width: u32,
    height: u32,
    packet_bits: Vec<[u32; 4]>,
    dependencies: Vec<u64>,
}

impl Key {
    /// Only a shader with per-slot all-query independence/alias certificates may
    /// use this key. Every uncertified slot MUST execute the canonical fallback.
    pub(crate) fn with_playhead_certificate(rows:&[[f32;4]],width:u32,height:u32,dependencies:&[u64],enabled:bool)->Result<Self,Reject>{
        let mut key=Self::new(rows,width,height,dependencies)?;
        key.dependencies.push(u64::from(enabled));
        if !enabled{return Ok(key);}
        // Require the exact normal non-custom schema, not guessed equivalence
        // for animated user materials/backgrounds. Dynamic aliases are rejected
        // separately per captured slot, even with these normal flags.
        if rows[245][0]!=0.||rows[245][2]!=0.
            ||rows[252][0].abs()>1_000_000.||rows[45751][3].abs()>1_000_000.
        {return Err(Reject::PlaybackEligibility);}
        key.packet_bits[245][3]=0; // only unreachable custom clock consumers
        key.packet_bits[252][0]=0; // playhead line X, per-query certificate required
        key.packet_bits[45751][3]=0; // handle X, independently certified
        Ok(key)
    }

    pub(crate) fn new(rows: &[[f32; 4]], width: u32, height: u32, dependencies: &[u64]) -> Result<Self, Reject> {
        workstation::validate_frame(rows).map_err(|_| Reject::Packet)?;
        if dependencies.is_empty() { return Err(Reject::Dependencies); }
        if rows.len() < BASE + 512 || rows[BASE][3] != 1. { return Err(Reject::MeterLayout); }
        let count = rows[BASE][0];
        if !(1. ..=MAX_METERS as f32).contains(&count) || count.fract() != 0. {
            return Err(Reject::MeterLayout);
        }
        let count = count as usize;
        for meter in 0..count {
            if rows[FIRST + meter * STRIDE + 3][2..].iter().any(|v| !(0. ..=1.).contains(v)) {
                return Err(Reject::MeterLayout);
            }
        }
        meter_regions::regions(rows, width, height).ok_or(Reject::CameraOrRegions)?;
        let mut packet_bits: Vec<_> = rows.iter().map(|r| r.map(f32::to_bits)).collect();
        for meter in 0..count {
            // ONLY the actual two level lanes. No clock, playhead, glyph,
            // inactive record, bottom waveform, pose, or other exemption.
            packet_bits[FIRST + meter * STRIDE + 3][2] = 0;
            packet_bits[FIRST + meter * STRIDE + 3][3] = 0;
        }
        Ok(Self { width, height, packet_bits, dependencies: dependencies.to_vec() })
    }
}

/// Disjoint rectangles in increasing y-band then x order. Each rectangle owns
/// row-major compact pixels beginning at first_pixel. Screen coordinates and
/// sample indices are unchanged; this is storage packing, not resampling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub rect: Rect,
    pub first_pixel: u32,
    pub pixels: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cost {
    pub term_bytes: u64,
    pub header_bytes: u64,
    pub map_bytes: u64,
    // 32-byte GPU region descriptor, 4-byte overflow flag, and one matching
    // readback allocation padded to an 8-byte map alignment.
    pub region_bytes: u64,
    pub total_bytes: u64,
}

fn cost(viewport_pixels: u64, compact_pixels: u64, regions: u64) -> Result<Cost, Reject> {
    let samples = compact_pixels.checked_mul(SAMPLES).ok_or(Reject::Arithmetic)?;
    let term_bytes = samples.checked_mul(TERMS).and_then(|n| n.checked_mul(TERM_BYTES)).ok_or(Reject::Arithmetic)?;
    let header_bytes = samples.checked_mul(HEADER_BYTES).ok_or(Reject::Arithmetic)?;
    let map_bytes = viewport_pixels.checked_mul(4).ok_or(Reject::Arithmetic)?;
    let readback_bytes = regions.checked_mul(4).and_then(|n| n.checked_add(7)).ok_or(Reject::Arithmetic)? & !7;
    let region_bytes = regions.checked_mul(36).and_then(|n| n.checked_add(readback_bytes)).ok_or(Reject::Arithmetic)?;
    let total_bytes = term_bytes.checked_add(header_bytes).and_then(|n| n.checked_add(map_bytes))
        .and_then(|n| n.checked_add(region_bytes)).ok_or(Reject::Arithmetic)?;
    if total_bytes > CAP_BYTES { return Err(Reject::Capacity); }
    Ok(Cost { term_bytes, header_bytes, map_bytes, region_bytes, total_bytes })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub width: u32,
    pub height: u32,
    pub regions: Vec<Region>,
    pub pixels: u32,
    pub cost: Cost,
}

impl Layout {
    pub(crate) fn new(width: u32, height: u32, input: &[Rect]) -> Result<Self, Reject> {
        if !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) { return Err(Reject::Viewport); }
        if input.len() > MAX_METERS { return Err(Reject::Rectangle); }
        let viewport_pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(Reject::Arithmetic)?;
        // Reject an oversized map before union construction or map allocation.
        cost(viewport_pixels, 0, 0)?;
        let mut edges = Vec::with_capacity(input.len() * 2);
        for r in input {
            let right = r.x.checked_add(r.width).ok_or(Reject::Arithmetic)?;
            let bottom = r.y.checked_add(r.height).ok_or(Reject::Arithmetic)?;
            if r.width == 0 || r.height == 0 || right > width || bottom > height { return Err(Reject::Rectangle); }
            edges.extend([r.y, bottom]);
        }
        edges.sort_unstable(); edges.dedup();
        let mut regions = Vec::new();
        let mut pixels = 0u32;
        for edge in edges.windows(2) {
            let (y, bottom) = (edge[0], edge[1]);
            let mut intervals: Vec<_> = input.iter().filter(|r| r.y <= y && r.y + r.height >= bottom)
                .map(|r| (r.x, r.x + r.width)).collect();
            intervals.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::new();
            for (left, right) in intervals {
                if let Some(last) = merged.last_mut() {
                    if left <= last.1 { last.1 = last.1.max(right); continue; }
                }
                merged.push((left, right));
            }
            for (x, right) in merged {
                let rect = Rect { x, y, width: right - x, height: bottom - y };
                let count = rect.width.checked_mul(rect.height).ok_or(Reject::Arithmetic)?;
                let next = pixels.checked_add(count).ok_or(Reject::Arithmetic)?;
                if next == UNMAPPED { return Err(Reject::Arithmetic); }
                regions.push(Region { rect, first_pixel: pixels, pixels: count });
                pixels = next;
            }
        }
        let cost = cost(viewport_pixels, u64::from(pixels), regions.len() as u64)?;
        Ok(Self { width, height, regions, pixels, cost })
    }

    /// Allocate only when a new generation will actually build, not on every
    /// key comparison. Entries hold compact PIXEL indices, not sample indices.
    pub(crate) fn build_pixel_map(&self) -> Result<Vec<u32>, Reject> {
        let len = usize::try_from(self.cost.map_bytes / 4).map_err(|_| Reject::Arithmetic)?;
        let mut map = Vec::new();
        map.try_reserve_exact(len).map_err(|_| Reject::Allocation)?;
        map.resize(len, UNMAPPED);
        for region in &self.regions {
            let r = &region.rect;
            for dy in 0..r.height {
                let screen = (u64::from(r.y + dy) * u64::from(self.width) + u64::from(r.x)) as usize;
                let compact = region.first_pixel + dy * r.width;
                for dx in 0..r.width { map[screen + dx as usize] = compact + dx; }
            }
        }
        Ok(map)
    }

    pub(crate) fn screen_pixel(&self, compact: u32) -> Option<[u32; 2]> {
        let region = self.regions.iter().find(|r| compact >= r.first_pixel && compact - r.first_pixel < r.pixels)?;
        let local = compact - region.first_pixel;
        Some([region.rect.x + local % region.rect.width, region.rect.y + local / region.rect.width])
    }

    pub(crate) fn sample_index(&self, compact: u32, sample: u32) -> Option<u32> {
        if compact >= self.pixels || sample >= SAMPLES as u32 { return None; }
        compact.checked_mul(SAMPLES as u32)?.checked_add(sample)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan { pub key: Key, pub layout: Layout }

impl Plan {
    pub(crate) fn new(rows: &[[f32; 4]], width: u32, height: u32, dependencies: &[u64]) -> Result<Self, Reject> {
        let key = Key::new(rows, width, height, dependencies)?;
        let regions = meter_regions::regions(rows, width, height).ok_or(Reject::CameraOrRegions)?;
        let layout = Layout::new(width, height, &regions)?;
        Ok(Self { key, layout })
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn fixture(n: usize) -> Vec<[f32; 4]> {
        let mut rows = vec![[0.; 4]; workstation::FRAME_ROWS];
        rows[0][2]=2.; rows[1]=[0.,0.,3.,0.]; rows[2]=[1.,0.,0.,1.3];
        rows[3]=[0.,1.,0.,0.65]; rows[4]=[0.,0.,-1.,1.]; rows[8]=[96.,64.,16.,64.];
        rows[BASE]=[n as f32,12544.,0.,1.];
        for m in 0..MAX_METERS {
            rows[FIRST+m*STRIDE+2]=[0.,0.,0.05,0.3];
            rows[FIRST+m*STRIDE+3]=[1.,0.,0.2,0.8];
            rows[FIRST+m*STRIDE+4]=[-1.,-0.6,1.,0.6];
        }
        rows
    }
    fn key(rows: &[[f32; 4]]) -> Key { Key::new(rows,96,64,&[7,8,9]).unwrap() }
    fn r(x:u32,y:u32,width:u32,height:u32)->Rect { Rect{x,y,width,height} }

    #[test] fn levels_are_the_only_excluded_lanes_in_the_entire_packet() {
        let rows=fixture(2); let original=key(&rows); let mut altered=rows.clone();
        for m in 0..2 { for lane in 2..4 { altered[FIRST+m*STRIDE+3][lane]=1.-altered[FIRST+m*STRIDE+3][lane]; } }
        assert_eq!(key(&altered),original);
        let excluded: Vec<_>=(0..rows.len()).flat_map(|at|(0..4).map(move |lane|(at,lane)))
            .filter(|&(at,lane)| original.packet_bits[at][lane] != rows[at][lane].to_bits()).collect();
        assert_eq!(excluded,vec![(FIRST+3,2),(FIRST+3,3),(FIRST+STRIDE+3,2),(FIRST+STRIDE+3,3)]);
        // Unused meter records are never exempted.
        altered=rows.clone(); altered[FIRST+2*STRIDE+3][2]=0.4; assert_ne!(key(&altered),original);
    }
    #[test] fn playhead_clock_glyph_bottom_pose_count_and_resource_epoch_invalidate() {
        let rows=fixture(1); let original=key(&rows);
        for (at,lane) in [(252,0),(245,3),(45312,1),(40992,1),(43664,0),(FIRST+3,0),(FIRST+2,0),(2,3)] {
            let mut b=rows.clone(); b[at][lane]+=0.001; assert_ne!(key(&b),original,"{at}/{lane}");
        }
        assert_ne!(key(&fixture(2)),original);
        assert_ne!(Key::new(&rows,96,64,&[7,8,10]).unwrap(),original);
        let mut b=rows.clone(); b[100][0]=-0.; assert_ne!(key(&b),original);
        b=rows.clone(); b[8][0]=97.; assert_ne!(Key::new(&b,97,64,&[7,8,9]).unwrap(),original);
    }
    #[test] fn invalid_packets_levels_layout_camera_and_missing_dependencies_reject() {
        let rows=fixture(1);
        assert!(Key::new(&rows[..rows.len()-1],96,64,&[1]).is_err());
        assert_eq!(Key::new(&rows,96,64,&[]),Err(Reject::Dependencies));
        for (at,lane,value) in [(BASE,0,0.),(BASE,0,70.5),(BASE,0,71.),(BASE,3,0.6),
            (FIRST+3,2,-0.1),(FIRST+3,3,1.1),(100,0,f32::NAN),(FIRST+3,2,f32::INFINITY),
            (1,0,0.1),(4,2,1.),(FIRST+2,2,0.),(FIRST+4,0,2.)] {
            let mut b=rows.clone(); b[at][lane]=value; assert!(Key::new(&b,96,64,&[1]).is_err(),"{at}/{lane}");
        }
        assert!(Key::new(&rows,95,64,&[1]).is_err());
    }
    #[test] fn overlap_duplicate_touching_rectangles_form_an_exact_disjoint_union() {
        let input=vec![r(1,1,4,4),r(3,2,5,2),r(1,1,4,4),r(8,2,1,2),r(0,6,2,1)];
        let layout=Layout::new(12,9,&input).unwrap(); let map=layout.build_pixel_map().unwrap();
        let mut covered=0; let mut seen=vec![false;layout.pixels as usize];
        for y in 0..9 { for x in 0..12 {
            let expected=input.iter().any(|r|x>=r.x&&x<r.x+r.width&&y>=r.y&&y<r.y+r.height);
            let compact=map[(y*12+x)as usize]; assert_eq!(compact!=UNMAPPED,expected);
            if expected {
                covered+=1; assert!(!seen[compact as usize]); seen[compact as usize]=true;
                assert_eq!(layout.screen_pixel(compact),Some([x,y]));
                for sample in 0..2 {assert_eq!(layout.sample_index(compact,sample),Some(compact*2+sample));}
                assert_eq!(layout.regions.iter().filter(|p|x>=p.rect.x&&x<p.rect.x+p.rect.width&&y>=p.rect.y&&y<p.rect.y+p.rect.height).count(),1);
            }
        }}
        assert_eq!(covered,layout.pixels); assert!(seen.iter().all(|x|*x));
        let mut reversed=input; reversed.reverse(); assert_eq!(layout,Layout::new(12,9,&reversed).unwrap());
        assert_eq!(layout.screen_pixel(layout.pixels),None);
        assert_eq!(layout.sample_index(0,2),None); assert_eq!(layout.sample_index(UNMAPPED,0),None);
    }
    #[test] fn bounds_arithmetic_and_memory_cap_reject_before_large_allocation() {
        for rect in [r(8,0,3,1),r(0,8,1,3),r(0,0,0,1)] {assert_eq!(Layout::new(10,10,&[rect]),Err(Reject::Rectangle));}
        assert_eq!(Layout::new(10,10,&[r(u32::MAX,0,2,1)]),Err(Reject::Arithmetic));
        assert_eq!(Layout::new(0,10,&[]),Err(Reject::Viewport));
        assert_eq!(Layout::new(10,10,&vec![r(0,0,1,1);71]),Err(Reject::Rectangle));
        assert_eq!(Layout::new(16384,16384,&[]),Err(Reject::Capacity));
        assert_eq!(Layout::new(1920,1200,&[r(0,0,1920,1200)]),Err(Reject::Capacity));
        assert_eq!(cost(1,u64::MAX,0),Err(Reject::Arithmetic));
        assert_eq!(cost(u64::MAX,0,0),Err(Reject::Arithmetic));
        assert_eq!(cost(1,0,u64::MAX),Err(Reject::Arithmetic));
        let c=cost(1920*1200,29376,1).unwrap();
        assert_eq!(c.term_bytes,45_121_536); assert_eq!(c.header_bytes,470_016);
        assert_eq!(c.map_bytes,9_216_000); assert_eq!(c.total_bytes,54_807_596);
        assert!(c.total_bytes<CAP_BYTES);
        let max_pixels=(CAP_BYTES-c.map_bytes-c.region_bytes)/(SAMPLES*(TERMS*TERM_BYTES+HEADER_BYTES));
        assert!(cost(1920*1200,max_pixels,1).is_ok());
        assert_eq!(cost(1920*1200,max_pixels+1,1),Err(Reject::Capacity));
        assert_eq!(Layout::new(10,10,&[]).unwrap().build_pixel_map().unwrap(),vec![UNMAPPED;100]);
    }
    #[test] fn integrated_planner_uses_original_conservative_scissors() {
        let rows=fixture(3); let p=Plan::new(&rows,96,64,&[1]).unwrap();
        let expected=meter_regions::regions(&rows,96,64).unwrap();
        assert_eq!(p.layout,Layout::new(96,64,&expected).unwrap());
        let map=p.layout.build_pixel_map().unwrap();
        assert!(p.layout.pixels>0); assert_eq!(map.len(),96*64);
    }
    #[test] fn deterministic_small_rectangle_fuzz_matches_brute_force_union() {
        let mut seed=0x12345678u32;
        let mut next=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed};
        for case in 0..200 {
            let mut input=Vec::new();
            for _ in 0..case%20 {let x=next()%31;let y=next()%23;input.push(r(x,y,1+next()%(31-x),1+next()%(23-y)));}
            let layout=Layout::new(31,23,&input).unwrap(); let map=layout.build_pixel_map().unwrap();
            let mut count=0;
            for y in 0..23 {for x in 0..31 {
                let expected=input.iter().any(|r|x>=r.x&&x<r.x+r.width&&y>=r.y&&y<r.y+r.height);
                let pixel=map[(y*31+x)as usize]; assert_eq!(pixel!=UNMAPPED,expected,"case{case} at{x},{y}");
                if expected {count+=1;assert_eq!(layout.screen_pixel(pixel),Some([x,y]));}
            }}
            assert_eq!(count,layout.pixels);
        }
    }
    #[test] fn playhead_certificate_exempts_only_the_three_audited_lanes() {
        let rows=fixture(1);let k=Key::with_playhead_certificate(&rows,96,64,&[1],true).unwrap();let mut b=rows.clone();
        b[245][3]=19.;b[252][0]=0.25;b[45751][3]=-0.125;
        assert_eq!(Key::with_playhead_certificate(&b,96,64,&[1],true).unwrap(),k);
        assert_ne!(Key::with_playhead_certificate(&b,96,64,&[1],false).unwrap(),Key::with_playhead_certificate(&rows,96,64,&[1],false).unwrap());
        for(at,lane)in[(252,1),(252,3),(253,0),(45751,0),(45751,1),(45751,2),(45752,1),(240,3),(45750,0)]{
            let mut b=rows.clone();b[at][lane]=0.25;assert_ne!(Key::with_playhead_certificate(&b,96,64,&[1],true).unwrap(),k);
        }
    }
    #[test] fn playhead_certificate_rejects_custom_paths_unbounded_x_and_nonfinite() {
        for(at,lane,v)in[(245,0,1.),(245,2,1.),(252,0,1e7),(45751,3,-1e7),(245,3,f32::NAN)]{
            let mut b=fixture(1);b[at][lane]=v;assert!(Key::with_playhead_certificate(&b,96,64,&[1],true).is_err());
        }
    }

}
