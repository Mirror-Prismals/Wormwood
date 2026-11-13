//! Physical transport-button travel, independent of pointer interpretation.
//! Actual XYZ is authoritative in every renderer. Descriptor.w records neutral Z
//! for these four instances only; all other descriptor fields keep their ABI.
const FIRST: usize = 192;
const COUNT: usize = 4;
// Authored transport depth is 23 mm. At the original half-pressure limit,
// travel is one quarter of that depth; the mesh itself is never flattened.
const DEPTH: f32 = 0.023;
const FULL_TRAVEL: f32 = DEPTH * 0.25;
// Existing dynamic allocation: engraving lines end at +368, lane furniture
// currently ends at +432, and Edison begins at +448. Reserve six rows in that
// unused gap; no frame length or immutable asset address changes. The compile
// assertion prevents future lane growth from overlapping this explicit reserve.
pub(crate) const META_ROWS: usize = 6;
pub(crate) const META_BASE: usize = super::engraving::BASE + 448 - META_ROWS;
const PRESSURE_ROW: usize = META_BASE + 4;
const PARAMETERS_ROW: usize = META_BASE + 5;
const META_TAG: f32 = 10531.;
const _: () = assert!(super::input_monitor::LANE_POSE_BASE
    + super::input_monitor::MAX_LANES * 2 <= META_BASE);

fn placed(neutral: [f32;4], pressure: f32, metres_per_logical_pixel: f32) -> [f32;4] {
    let mut pose = neutral;
    // Cardinal ButtonSystem.cpp:939-941 also moves GEM buttons left by
    // 10*pressure logical pixels; ordinary held/latch pressure is at most 0.5.
    pose[0] -= 10. * pressure * metres_per_logical_pixel;
    pose[2] -= FULL_TRAVEL * pose[3] * (pressure * 2.);
    pose
}

/// Call once, immediately after the decorative kit writes its neutral poses.
/// Keep the coarse top AABB conservative for the entire travel, and let the
/// final per-instance hierarchy describe the actual current geometry.
pub(crate) fn apply(rows: &mut [[f32; 4]], pressure: [f64; 4], metres_per_logical_pixel: f64) -> Result<(), String> {
    if rows.len() < META_BASE + META_ROWS
        || !metres_per_logical_pixel.is_finite() || metres_per_logical_pixel <= 0.
        || !(metres_per_logical_pixel as f32).is_finite() || metres_per_logical_pixel as f32 <= 0.
        || pressure.iter().any(|p| !p.is_finite() || !(0.0..=0.5).contains(p))
    {
        return Err("transport pose needs four finite pressures within 0..0.5".into());
    }
    let mpp = metres_per_logical_pixel as f32;
    let mut max_travel = 0f32;
    for i in 0..COUNT {
        let at = FIRST + i * 2;
        let pose = rows[at];
        if !pose.iter().all(|v| v.is_finite()) || pose[3] < 0. {
            return Err("invalid authored transport pose".into());
        }
        rows[META_BASE + i] = pose;
        rows[at + 1][3] = pose[2];
        let travel = FULL_TRAVEL * pose[3];
        rows[at] = placed(pose, pressure[i] as f32, mpp);
        max_travel = max_travel.max(travel);
    }
    rows[PRESSURE_ROW] = pressure.map(|v| v as f32);
    rows[PARAMETERS_ROW] = [mpp, META_TAG, 0., 0.];
    // Header 13/14 remains a conservative XYZ AABB for the full physical
    // left/inward travel, independent of pressure. W lanes remain untouched.
    rows[13][0] -= 2.5 * mpp;
    rows[14][0] += 2.5 * mpp;
    rows[13][2] -= max_travel * 0.5;
    rows[14][2] += max_travel * 0.5;
    Ok(())
}

/// The retained per-pixel depth transform is exact for this projection only.
/// Other cameras retain the original renderer and its actual moved geometry.
pub(crate) fn retained_camera(rows: &[[f32; 4]]) -> bool {
    rows.len() >= 256
        && rows[0][2] == 2. && rows[1][3] == 0.
        && rows[1][0] == 0. && rows[1][1] == 0. && rows[1][2].is_finite()
        && rows[2][..3] == [1.,0.,0.] && rows[3][..3] == [0.,1.,0.]
        && rows[4][..3] == [0.,0.,-1.]
        && [rows[2][3],rows[3][3],rows[8][0],rows[8][1]].iter().all(|v| v.is_finite() && *v > 0.)
}

fn valid_poses(rows: &[[f32; 4]]) -> bool {
    if rows.len() < META_BASE + META_ROWS || !retained_camera(rows) || rows[12][0] < 4. { return false; }
    let parameters = rows[PARAMETERS_ROW];
    if parameters[0] <= 0. || !parameters[0].is_finite() || parameters[1..] != [META_TAG,0.,0.] { return false; }
    (0..COUNT).all(|i| {
        let at = FIRST + i * 2;
        let neutral = rows[META_BASE+i]; let pressure = rows[PRESSURE_ROW][i];
        neutral.iter().all(|v| v.is_finite()) && neutral[3] > 0. && neutral[2] > 0.
            && pressure.is_finite() && (0.0..=0.5).contains(&pressure)
            && rows[at+1][3].to_bits() == neutral[2].to_bits()
            && rows[at].map(f32::to_bits) == placed(neutral,pressure,parameters[0]).map(f32::to_bits)
    })
}

/// Compare all optical inputs. Exempt only the established live playhead and
/// style clock, plus bit-validated button X/Z movement when buttons have their own layer.
/// A derived BVH can change only when its complete contents validate against
/// the actual live poses and immutable mesh-root bounds in each packet.
pub(crate) fn optics_changed(a: &[[f32; 4]], b: &[[f32; 4]], isolated_buttons: bool) -> bool {
    if a.len() != b.len() { return true; }
    let motion = isolated_buttons && valid_poses(a) && valid_poses(b);
    let moved = motion && (0..COUNT).any(|i| [0,2].into_iter().any(|j| a[FIRST+i*2][j].to_bits()!=b[FIRST+i*2][j].to_bits()));
    let derived = if moved {
        super::decorative_controls::derived_instance_motion_range(a,b)
    } else { None };
    let bars=super::timeline_bar_scene::BASE;
    let live_handle=[a,b].iter().all(|r| r.len()>=bars+super::timeline_bar_scene::ROWS
        && r[bars+8][2]==super::timeline_bar_scene::TAG
        && r[bars+8][1]==1. && r[bars+8][3]>0. && r[bars+7][3].is_finite());
    a.iter().zip(b).enumerate().any(|(i,(x,y))| {
        if i == 252 || (motion && i == PRESSURE_ROW) || derived.as_ref().is_some_and(|r| r.contains(&i)) { return false; }
        (0..4).any(|j| {
            if super::live_panel_data::live_field(a,i,j)&&super::live_panel_data::live_field(b,i,j){return false;}
            if i == 245 && j == 3 { return false; }
            if live_handle && i == bars+7 && j == 3 { return false; }
            if motion && (FIRST..FIRST+COUNT*2).contains(&i) && (i-FIRST)%2 == 0 && (j == 0 || j == 2) { return false; }
            x[j].to_bits()!=y[j].to_bits()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<[f32;4]> {
        let mut r=vec![[0.;4];META_BASE+META_ROWS];
        r[0][2]=2.;r[1]=[0.,0.,3.,0.];r[2]=[1.,0.,0.,1.];r[3]=[0.,1.,0.,1.];r[4]=[0.,0.,-1.,0.];r[8]=[1200.,780.,0.,0.];r[12][0]=4.;
        r[13]=[0.,0.4,0.012,0.];r[14]=[0.2,0.02,0.012,0.];
        for i in 0..4 {r[FIRST+i*2]=[(i as f32-1.5)*0.055,0.4,0.0235,1.];r[FIRST+i*2+1]=[1000.+i as f32*10.,1006.+i as f32*10.,0.,0.];}
        r
    }
    #[test]
    fn physical_press_moves_xz_and_preserves_neutral_pose_material_and_bounds_contract() {
        let original=fixture();let mut neutral=original.clone();apply(&mut neutral,[0.;4],1.3/780.).unwrap();
        let mut down=original.clone();apply(&mut down,[0.,0.25,0.5,0.],1.3/780.).unwrap();
        assert_eq!(neutral[13],down[13]);assert_eq!(neutral[14],down[14]);
        assert_eq!(neutral[13][1],original[13][1]);assert_eq!(neutral[14][1],original[14][1]);
        assert!(neutral[13][0]<original[13][0]);assert!(neutral[14][0]>original[14][0]);
        for i in 0..4 {let at=FIRST+i*2;assert_eq!(neutral[at],original[at]);assert_eq!(down[at][1],original[at][1]);assert_eq!(down[META_BASE+i],original[at]);assert_eq!(down[at][3],original[at][3]);assert_eq!(&down[at+1][..3],&original[at+1][..3]);assert_eq!(down[at+1][3],original[at][2]);}
        assert_eq!(down[196][2],original[196][2]-FULL_TRAVEL);
        assert!(!optics_changed(&neutral,&down,true));assert!(!optics_changed(&down,&neutral,true));
        assert!(optics_changed(&neutral,&down,false));
    }
    #[test]
    fn unrelated_edits_and_invalid_motion_are_not_hidden() {
        let mut a=fixture();apply(&mut a,[0.;4],1.3/780.).unwrap();
        for(row,col)in[(13,0),(14,2),(192,0),(192,3),(193,0),(193,3),(200,2),(254,0),(META_BASE,0),(META_BASE,1),(PARAMETERS_ROW,0)] {let mut b=a.clone();b[row][col]+=0.001;assert!(optics_changed(&a,&b,true),"{row}:{col}");}
        let mut bad=a.clone();bad[192][2]=a[193][3]+0.01;assert!(optics_changed(&a,&bad,true));
        bad=a.clone();bad[192][2]=a[193][3]-FULL_TRAVEL-0.01;assert!(optics_changed(&a,&bad,true));
        for pressure in [f64::NAN,-0.01,0.51] {assert!(apply(&mut fixture(),[pressure;4],1.3/780.).is_err());}
        bad=a.clone();bad[4][0]=0.1;assert!(!retained_camera(&bad));
        bad=a.clone();bad[PRESSURE_ROW][0]=0.25;assert!(optics_changed(&a,&bad,true));
        bad=a.clone();bad[192][0]=f32::from_bits(a[192][0].to_bits()+1);assert!(optics_changed(&a,&bad,true));
        assert!(apply(&mut fixture(),[0.;4],0.).is_err());
    }
    #[test]
    fn only_valid_live_timeline_handle_x_is_exempt() {
        let mut a=fixture();apply(&mut a,[0.;4],1.3/780.).unwrap();
        let base=super::super::timeline_bar_scene::BASE;
        a[base+8]=[32.,1.,super::super::timeline_bar_scene::TAG,3.];
        let mut b=a.clone();b[base+7][3]=0.25;assert!(!optics_changed(&a,&b,true));
        for(row,col)in[(base+7,0),(base+7,1),(base+7,2),(base+8,1),(base+8,2),(base+9,3),(META_BASE,0)]{
            let mut c=b.clone();c[row][col]+=0.1;assert!(optics_changed(&a,&c,true),"{row}:{col}");
        }
        a[base+8][2]=0.;b[base+8][2]=0.;assert!(optics_changed(&a,&b,true));
    }
    #[test]
    fn actual_pose_projects_to_cardinals_five_logical_pixel_full_press() {
        for (width,height,dpi) in [(1280.,720.,1.),(1280.,720.,2.),(780.,1200.,1.)] {
            let mut neutral=fixture();let mpp=1.3/height;
            neutral[2][3]=(width*mpp*0.5) as f32;neutral[3][3]=(height*mpp*0.5) as f32;
            neutral[8][0]=(width*dpi) as f32;neutral[8][1]=(height*dpi) as f32;
            let mut pressed=neutral.clone();apply(&mut neutral,[0.;4],mpp).unwrap();apply(&mut pressed,[0.5;4],mpp).unwrap();
            let pixel=|r:&[[f32;4]],i:usize|(r[FIRST+i*2][0]/r[2][3]+1.)*r[8][0]*0.5;
            for i in 0..4 {assert!(((pixel(&neutral,i)-pixel(&pressed,i))/dpi as f32-5.).abs()<0.0001);}
            assert!(!optics_changed(&neutral,&pressed,true));assert!(!optics_changed(&pressed,&neutral,true));
            // The existing captured/current pose remap uses actual centers.
            let capture=pixel(&pressed,0);let current=pixel(&neutral,0);
            let source=(current+0.5-current)+capture-0.5;
            assert_eq!(source,capture); // release samples the pressed capture's center
        }
    }
    #[test]
    fn captured_pressed_depth_uses_capture_delta_and_never_tints_rgb() {
        fn sample(value:[f32;4],captured:[f32;4],current:[f32;4],old_camera:f32,camera:f32)->[f32;4] {
            if current[2]==captured[2] && current[3]==captured[3] && camera==old_camera {return value;}
            let local=(old_camera-value[3]-captured[2])/captured[3];
            [value[0],value[1],value[2],camera-(current[2]+local*current[3])]
        }
        // Power-of-two coordinates make the reference geometry exact in f32.
        let rgb=[0.125,0.25,0.5,1.75];let capture=[0.,0.,0.25,1.];
        assert_eq!(sample(rgb,capture,capture,2.,2.).map(f32::to_bits),rgb.map(f32::to_bits));
        let down=[0.,0.,0.125,1.];let pressed=sample(rgb,capture,down,2.,2.);
        assert_eq!(pressed,[0.125,0.25,0.5,1.875]);
        assert_eq!(sample(pressed,down,down,2.,2.),pressed); // no double depression
        assert_eq!(sample(pressed,down,capture,2.,2.),rgb); // releasing a pressed capture
        assert_eq!(sample(rgb,capture,[0.,0.,0.25,2.],2.,2.),rgb); // same face, changed scale
    }
}
