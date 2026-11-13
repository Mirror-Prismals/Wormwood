//! Conservative live-meter scissors for the existing two-sample retained camera.
//! Each region must run the WHOLE original fs_retained, never just one meter.
//! None means use the unchanged original draw. No rows/resources may change
//! between the cheap base draw and the full-shader scissored draws.
const BASE: usize = 43_152;
const FIRST: usize = BASE + 8;
const STRIDE: usize = 6;
const MAX_METERS: usize = 70;
const LIMIT: f64 = 1e6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn regions(rows: &[[f32; 4]], width: u32, height: u32) -> Option<Vec<Rect>> {
    // Match TransportPose::retained_camera plus conservative arithmetic limits.
    // Do not assume a perspective/rotated camera uses this XY projection.
    if rows.len() < FIRST
        || !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height)
        || rows[0][2] != 2. || rows[1][3] != 0.
        || rows[1][0] != 0. || rows[1][1] != 0.
        || rows[2][..3] != [1., 0., 0.] || rows[3][..3] != [0., 1., 0.]
        || rows[4][..3] != [0., 0., -1.]
        || rows[8][0] != width as f32 || rows[8][1] != height as f32
        || rows[1..=4].iter().flatten().any(|v| !v.is_finite() || f64::from(v.abs()) > LIMIT)
        || !(1e-6..=1e6).contains(&rows[2][3]) || !(1e-6..=1e6).contains(&rows[3][3])
    { return None; }
    let header = rows[BASE];
    if !header[0].is_finite() || header[0] < 0. || header[0] > MAX_METERS as f32
        || header[0].fract() != 0. || !header[3].is_finite() || f64::from(header[3].abs()) > LIMIT
    { return None; }
    let count = header[0] as usize;
    if rows.len() < FIRST + count * STRIDE { return None; }
    // This is the exact outer shader condition, including its strict threshold.
    if header[3] <= 0.5 || count == 0 { return Some(Vec::new()); }
    let camera = rows[1];
    let half = [rows[2][3], rows[3][3]];
    let mut result = Vec::new();
    for meter in 0..count {
        let at = FIRST + meter * STRIDE;
        let bounds = rows[at + 2];
        let clip = rows[at + 4];
        if bounds.iter().chain(&clip).any(|v| !v.is_finite() || f64::from(v.abs()) > LIMIT)
            || bounds[2] <= 0. || bounds[3] <= 0.
            || clip[0] >= clip[2] || clip[1] >= clip[3]
        { return None; }
        // Compute the aperture threshold with the same float32 operations as
        // abs(origin.xy-box.xy) <= box.zw*0.5 + vec2(0.000001).
        let aperture = [bounds[2] * 0.5 + 0.000001, bounds[3] * 0.5 + 0.000001];
        let mut lo = [0.; 2];
        let mut hi = [0.; 2];
        for axis in 0..2 {
            // Enclose roundoff in retainedRay, abs(origin-box), and projection.
            // Intersect AFTER expansion: near-touching clip/aperture edges can
            // pass the original float32 gate despite a tiny exact-real gap.
            let magnitude = 1. + f64::from(camera[axis].abs()) + f64::from(half[axis])
                + f64::from(bounds[axis].abs()) + f64::from(aperture[axis])
                + f64::from(clip[axis].abs().max(clip[axis + 2].abs()));
            let margin = 64. * f64::from(f32::EPSILON) * magnitude;
            lo[axis] = (f64::from(bounds[axis]) - f64::from(aperture[axis]) - margin)
                .max(f64::from(clip[axis]) - margin);
            hi[axis] = (f64::from(bounds[axis]) + f64::from(aperture[axis]) + margin)
                .min(f64::from(clip[axis + 2]) + margin);
        }
        if lo[0] > hi[0] || lo[1] > hi[1] { continue; }
        let x = |v: f64| (1. + (v - f64::from(camera[0])) / f64::from(half[0])) * 0.5 * f64::from(width);
        let y = |v: f64| (1. - (v - f64::from(camera[1])) / f64::from(half[1])) * 0.5 * f64::from(height);
        // Same full viewport/sample positions. The 2px enclosure includes both
        // original strata (x offsets .375/.875, y offsets .875/.125) and an
        // extra conservative pixel on either side. Scissor upper edge excludes.
        let x0 = (x(lo[0]).floor() - 2.).clamp(0., f64::from(width)) as u32;
        let x1 = (x(hi[0]).ceil() + 2.).clamp(0., f64::from(width)) as u32;
        let y0 = (y(hi[1]).floor() - 2.).clamp(0., f64::from(height)) as u32;
        let y1 = (y(lo[1]).ceil() + 2.).clamp(0., f64::from(height)) as u32;
        if x1 > x0 && y1 > y0 {
            let rect = Rect { x:x0, y:y0, width:x1-x0, height:y1-y0 };
            // Repeating a full original shader is equivalent, but identical
            // rectangles have no benefit. No per-meter-order rewrite occurs.
            if !result.contains(&rect) { result.push(rect); }
        }
    }
    Some(result)
}

#[cfg(test)] mod tests {
    use super::*;
    fn fixture(w:u32,h:u32,n:usize)->Vec<[f32;4]> {
        let mut rows=vec![[0.;4];FIRST+n*STRIDE];
        rows[0][2]=2.;rows[1]=[0.,0.,3.,0.];rows[2]=[1.,0.,0.,1.3];
        rows[3]=[0.,1.,0.,0.65];rows[4]=[0.,0.,-1.,1.];rows[8]=[w as f32,h as f32,16.,64.];
        rows[BASE]=[n as f32,12544.,0.,1.];
        for m in 0..n {rows[FIRST+m*STRIDE+2]=[0.,0.,0.05,0.3];rows[FIRST+m*STRIDE+4]=[-1.,-0.6,1.,0.6];}
        rows
    }
    // Operation-by-operation transcription of retainedRay, not the inverse
    // projection used by the planner. This deliberately preserves f32 rounds.
    fn origin(rows:&[[f32;4]],x:u32,y:u32,s:u32)->[f32;2] {
        let r=rows[8];let sample=s as f32;
        let xi=[(0.375+0.5*sample).fract(),(0.125+0.75*sample).fract()];
        let screen=[(x as f32+0.5)*2./r[0]-1.,1.-(y as f32+0.5)*2./r[1]];
        let p=[screen[0]+(xi[0]-0.5)*2./r[0],screen[1]+(xi[1]-0.5)*2./r[1]];
        [rows[1][0]+p[0]*rows[2][3],rows[1][1]+p[1]*rows[3][3]]
    }
    fn original_gate(rows:&[[f32;4]],o:[f32;2],m:usize)->bool {
        if !(rows[BASE][3]>0.5) || m as f32>=rows[BASE][0] {return false;}
        let b=rows[FIRST+m*STRIDE+2];let c=rows[FIRST+m*STRIDE+4];
        !( (o[0]-b[0]).abs()>b[2]*0.5+0.000001 || (o[1]-b[1]).abs()>b[3]*0.5+0.000001
            || o[0]<c[0] || o[1]<c[1] || o[0]>c[2] || o[1]>c[3] )
    }
    fn contains(r:Rect,x:u32,y:u32)->bool{x>=r.x&&x<r.x+r.width&&y>=r.y&&y<r.y+r.height}
    fn check(rows:&[[f32;4]],w:u32,h:u32)->usize {
        let regions=regions(rows,w,h).expect("valid fixture");let mut admitted=0;
        for y in 0..h {for x in 0..w {for s in 0..2 {
            let o=origin(rows,x,y,s);
            if (0..rows[BASE][0] as usize).any(|m|original_gate(rows,o,m)) {
                admitted+=1;assert!(regions.iter().any(|r|contains(*r,x,y)),"uncovered pixel {x},{y}, sample{s}, origin{o:?}, regions{regions:?}");
            }
        }}}
        for r in regions {assert!(r.width>0&&r.height>0&&r.x+r.width<=w&&r.y+r.height<=h);}
        admitted
    }
    #[test] fn fractional_apertures_clips_and_all_original_sample_positions_are_enclosed() {
        let (w,h)=(97,71);let mut hits=0;
        for i in 0..96 {
            let mut r=fixture(w,h,3);let f=i as f32/96.;
            for m in 0..3 {let at=FIRST+m*STRIDE;
                r[at+2]=[-1.1+2.2*f+(m as f32-1.)*0.03125,-0.6+1.2*(1.-f),0.001+(i%13)as f32*0.0125,0.001+(i%17)as f32*0.017];
                r[at+4]=[-0.97,-0.51,0.98,0.53];
            }
            hits+=check(&r,w,h);
        }
        assert!(hits>100);
    }
    #[test] fn gates_touching_pixel_strata_and_clip_edges_are_not_lost() {
        let (w,h)=(83,59);let mut hits=0;
        for s in 0..2 {for (x,y) in [(0,0),(1,1),(41,29),(82,58)] {for delta in [-0.000001,0.,0.000001] {
            let mut r=fixture(w,h,1);let o=origin(&r,x,y,s);let at=FIRST;
            r[at+2]=[o[0]+delta,o[1]-delta,0.0000001,0.0000001];
            r[at+4]=[o[0]-0.000001,o[1]-0.000001,o[0]+0.000001,o[1]+0.000001];
            hits+=check(&r,w,h);
        }}}
        assert!(hits>=8);
    }
    #[test] fn large_finite_coordinates_and_tiny_view_spans_remain_conservative() {
        for half in [0.000001,0.001,1.,100000.,1000000.] {
            let (w,h)=(41,37);let mut r=fixture(w,h,2);r[2][3]=half;r[3][3]=half;
            let o=origin(&r,20,18,1);r[FIRST+2]=[o[0],o[1],half*0.000001,half*0.000001];
            r[FIRST+4]=[-half,-half,half,half];
            r[FIRST+STRIDE+2]=[999999.,-999999.,1.,1.];r[FIRST+STRIDE+4]=[-1000000.,-1000000.,1000000.,1000000.];
            assert!(check(&r,w,h)>0);
        }
    }
    #[test] fn max_count_overlap_disabled_and_outside_regions_are_supported() {
        let mut r=fixture(31,23,70);assert!(check(&r,31,23)>0);
        assert_eq!(regions(&r,31,23).unwrap().len(),1); // exact duplicate scissors
        r[BASE][3]=0.5;assert!(regions(&r,31,23).unwrap().is_empty());
        r[BASE][3]=1.;r[BASE][0]=0.;assert!(regions(&r,31,23).unwrap().is_empty());
        r[BASE][0]=70.;for m in 0..70{r[FIRST+m*STRIDE+2]=[3.,3.,0.1,0.1];}
        assert!(regions(&r,31,23).unwrap().is_empty());
    }
    #[test] fn changed_camera_dimensions_or_invalid_packet_falls_back() {
        let r=fixture(64,48,1);
        for (row,col,value) in [(0,2,1.),(1,0,0.01),(1,1,-0.01),(1,3,1.),(2,1,0.1),(3,0,0.1),(4,0,0.1),(2,3,0.),(3,3,f32::INFINITY),(BASE,0,70.1),(BASE,0,71.),(BASE,0,-1.),(BASE,3,f32::NAN),(FIRST+2,0,f32::NAN),(FIRST+2,2,0.),(FIRST+2,3,-1.),(FIRST+4,2,-2.),(FIRST+4,0,f32::INFINITY)] {
            let mut bad=r.clone();bad[row][col]=value;assert!(regions(&bad,64,48).is_none(),"row{row} col{col} value{value}");
        }
        assert!(regions(&r,63,48).is_none());assert!(regions(&r,64,47).is_none());assert!(regions(&r,0,48).is_none());
        assert!(regions(&r[..FIRST],64,48).is_none());assert!(regions(&r[..100],64,48).is_none());
    }
}
