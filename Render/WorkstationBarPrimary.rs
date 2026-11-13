//! Axial outside rays can reject a bar using one ORIGINAL vertical halfspace.
//! The rectangle selects a candidate plane, never authorizes a rejection.
fn span(s:&str,name:&str)->Result<(usize,usize),String>{
    let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("bar gate function changed: {name}"));}
    let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing bar body")?;let mut depth=0;
    for (i,b) in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok((start,i+1));}}}
    Err("unclosed bar body".into())
}
pub(super) fn source(original:&str)->Result<String,String>{
    if original.contains("BAR_PRIMARY_CERTIFIED_GATE"){return Err("bar gate already installed".into());}
    let (start,end)=span(original,"gemBarHit")?;let body=&original[start..end];
    for marker in ["for(var i:i32=0;i<3;i++)", "if(medium>=0&&medium!=panel){continue;}",
        "if(size.w<0.5||pose.w<=0.0){continue;}","for(var j:i32=0;j<26;j++)",
        "var plane:vec4<f32>=gemBarPlane(j,size.xy);var denominator:f32=dot(plane.xyz,ray);",
        "var distance:f32=plane.w-dot(plane.xyz,local);",
        "if(abs(denominator)<0.0000001){if(distance< -0.0000001){miss=true;}}"] {
        if body.matches(marker).count()!=1{return Err(format!("bar rejection contract changed: {marker}"));}
    }
    if body.matches(ANCHOR).count()!=1{return Err("bar local transform changed".into());}
    let changed=body.replacen(ANCHOR,&format!("{ANCHOR}{GATE}"),1);
    let mut result=original.to_owned();result.replace_range(start..end,&changed);Ok(result)
}
const ANCHOR:&str="        var local:vec3<f32>=(origin-pose.xyz)/pose.w;\n";
const GATE:&str=r#"        // BAR_PRIMARY_CERTIFIED_GATE: only an original plane can reject.
        // Signed-zero XY is axial too. NaN comparisons retain the original
        // path; infinities use the same original predicate. No estimated cull.
        if(medium<0&&ray.x==0.0&&ray.y==0.0){
            var side:i32=-1;
            if(local.x>size.x){side=2;}
            else if(local.x< -size.x){side=6;}
            else if(local.y>size.y){side=4;}
            else if(local.y< -size.y){side=8;}
            if(side>=0){
                let plane=gemBarPlane(side,size.xy);
                let denominator=dot(plane.xyz,ray);
                let distance=plane.w-dot(plane.xyz,local);
                if(abs(denominator)<0.0000001&&distance< -0.0000001){continue;}
            }
        }
"#;

#[cfg(test)] mod tests{
    use super::*;
    use std::cell::Cell;
    const ORIGINAL:&str=include_str!("../Procedures/Shaders/workstation.wgsl");
    fn validate(s:&str){let m=naga::front::wgsl::parse_str(s).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();}
    #[test]fn only_certified_prelude_changes_and_both_wgsl_sources_validate(){
        let changed=source(ORIGINAL).unwrap();validate(&changed);assert_eq!(changed.replace(GATE,""),ORIGINAL);
        let gated=crate::workstation::ruby_response_shader::source(&changed).unwrap();validate(&gated);
        let(a,b)=span(ORIGINAL,"gemBarPlane").unwrap();let(c,d)=span(&changed,"gemBarPlane").unwrap();assert_eq!(&ORIGINAL[a..b],&changed[c..d]);
        assert!(source(&changed).is_err());assert!(source(&ORIGINAL.replace("distance< -0.0000001){miss=true;}","distance< -0.001){miss=true;}")).is_err());
    }
    // Operation-order transcription used for a comparison proof, not as a new
    // rendering implementation. Native GPU image equivalence remains separate.
    fn dot(a:[f32;3],b:[f32;3])->f32{a[0]*b[0]+a[1]*b[1]+a[2]*b[2]}
    fn plane(index:i32,half:[f32;2],calls:&Cell<usize>)->[f32;4]{
        calls.set(calls.get()+1);let mut p=[0.,0.,1.,0.30];
        if index==1{p=[0.,0.,-1.,0.85];}
        if index>=2{let ring=(index-2)/8;let side=(index-2)%8;let mut angle=side as f32*0.7853981633974483;if ring==2{angle+=0.39269908169872414;}
            let x=angle.cos();let y=angle.sin();let extent=x.abs()*half[0]+y.abs()*half[1]-0.18*x.abs().min(y.abs());let slope=extent*0.86/(0.85-0.07);
            let mut z=0.;let mut distance=extent;if ring==1{z=0.31/0.30;}if ring==2{z=-slope;distance=extent+slope*0.07;}p=[x,y,z,distance];}
        let magnitude=dot([p[0],p[1],p[2]],[p[0],p[1],p[2]]).sqrt();[p[0]/magnitude,p[1]/magnitude,p[2]/magnitude,(p[3]-0.30*p[2])/magnitude]
    }
    fn certificate(local:[f32;3],ray:[f32;3],half:[f32;2],medium:i32,calls:&Cell<usize>)->bool{
        if medium>=0||ray[0]!=0.||ray[1]!=0.{return false;}
        let side=if local[0]>half[0]{2}else if local[0]< -half[0]{6}else if local[1]>half[1]{4}else if local[1]< -half[1]{8}else{return false;};
        let p=plane(side,half,calls);let denominator=dot([p[0],p[1],p[2]],ray);let distance=p[3]-dot([p[0],p[1],p[2]],local);
        denominator.abs()<0.0000001&&distance< -0.0000001
    }
    fn original(local:[f32;3],ray:[f32;3],half:[f32;2],scale:f32,medium:i32,limit:f32,calls:&Cell<usize>)->[u32;5]{
        let mut near=-1e20_f32;let mut far=1e20_f32;let mut entering=[0.;3];let mut leaving=[0.;3];let mut miss=false;
        for j in 0..26{let p=plane(j,half,calls);let n=[p[0],p[1],p[2]];let denominator=dot(n,ray);let distance=p[3]-dot(n,local);
            if denominator.abs()<0.0000001{if distance< -0.0000001{miss=true;}}
            else{let t=distance/denominator;if denominator<0.&&t>near{near=t;entering=n;}if denominator>0.&&t<far{far=t;leaving=n;}}
            if far<near{miss=true;}if miss{break;}}
        let (t,normal)=if medium>=0{(far,leaving)}else{(near,entering)};let world=t*scale;
        if !miss&&world>0.0000001&&world<limit{[world.to_bits(),normal[0].to_bits(),normal[1].to_bits(),normal[2].to_bits(),720000]}else{[limit.to_bits(),0,0,0,u32::MAX]}
    }
    fn check(local:[f32;3],ray:[f32;3],half:[f32;2],scale:f32,medium:i32,limit:f32){
        let calls=Cell::new(0);let before=original(local,ray,half,scale,medium,limit,&calls);
        let after=if certificate(local,ray,half,medium,&calls){[limit.to_bits(),0,0,0,u32::MAX]}else{original(local,ray,half,scale,medium,limit,&calls)};
        assert_eq!(before,after,"local={local:?} ray={ray:?} half={half:?} scale={scale} medium={medium} limit={limit}");
    }
    #[test]fn boundary_roundoff_far_limits_signed_zero_and_inside_rays_keep_hit_bits(){
        for half in [[1.,1.],[80.,1.],[1.,80.],[0.01,0.01],[100000.,1.]]{for axis in 0..2{for sign in [-1.,1.]{for delta in [-0.01,-1e-6,-1e-7,-1e-8,0.,1e-8,1e-7,1e-6,0.01]{
            let mut local=[0.,0.,3.];local[axis]=sign*(half[axis]+delta);
            for ray in [[0.,0.,-1.],[-0.,-0.,-1.],[0.,0.,1.],[1e-8,0.,-1.],[0.,-1e-8,-1.]]{for medium in [-1,720000]{for limit in [0.,0.0000001,0.1,1.,3.,1e20]{check(local,ray,half,0.05,medium,limit);}}}
        }}}}
        for v in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,-0.,0.,f32::MIN_POSITIVE]{for axis in 0..3{let mut p=[0.,0.,3.];p[axis]=v;check(p,[0.,0.,-1.],[1.,1.],1.,-1,1e20);let mut ray=[0.,0.,-1.];ray[axis]=v;check([2.,2.,3.],ray,[1.,1.],1.,-1,1e20);}}
    }
    #[test]fn random_axial_and_nonaxial_rays_match_original(){
        let mut seed=0x83a917a33b510e2b_u64;let mut unit=||{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);((seed>>40)as u32)as f32/16777216.};
        for i in 0..30_000{let half=[0.01+unit()*200.,0.01+unit()*200.];let local=[(unit()*4.-2.)*half[0],(unit()*4.-2.)*half[1],unit()*100.-2.];let ray=if i%3==0{[unit()*2.-1.,unit()*2.-1.,unit()*2.-1.]}else{[0.,0.,if i%2==0{-1.}else{1.}]};check(local,ray,half,0.00001+unit()*20.,if i%5==0{720000}else{-1},unit()*10000.);}
    }
    #[test]fn far_screen_misses_reduce_actual_plane_evaluations(){
        let old=Cell::new(0);let new=Cell::new(0);let mut certified=0;
        for y in [-100.,100.]{for x in -50..50{let local=[x as f32,y,600.];let half=[80.,1.];let ray=[0.,0.,-1.];let a=original(local,ray,half,0.005,-1,1e20,&old);
            let b=if certificate(local,ray,half,-1,&new){certified+=1;[1e20_f32.to_bits(),0,0,0,u32::MAX]}else{original(local,ray,half,0.005,-1,1e20,&new)};assert_eq!(a,b);}}
        println!("200 representative far-screen rays: original plane evaluations={}, candidate={}, certified misses={certified}",old.get(),new.get());assert_eq!(certified,200);assert!(new.get()<old.get());
    }
}
