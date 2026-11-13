//! Explicit lighting only for finite combined benchmarks; normal cameras are unchanged.
use serde_json::{Value,json};
const ENV:&str="WORMWOOD_COMBINED_CAMERA_FRACTION";

pub(crate) fn from_environment(embedded_benchmark:bool)->Result<Option<f64>,String>{
    let value=match std::env::var(ENV){
        Ok(v)=>Some(v),Err(std::env::VarError::NotPresent)=>None,
        Err(std::env::VarError::NotUnicode(_))=>return Err(format!("{ENV} must be valid UTF-8")),
    };
    fraction(embedded_benchmark,value.as_deref())
}
fn fraction(embedded_benchmark:bool,value:Option<&str>)->Result<Option<f64>,String>{
    let Some(value)=value else{return Ok(None);};
    if !embedded_benchmark{return Err(format!("{ENV} requires a finite embedded pass-through benchmark"));}
    value.parse::<f64>().ok().filter(|v|v.is_finite()&&(0.0..=1.0).contains(v))
        .map(Some).ok_or_else(||format!("{ENV} must be finite within 0..1"))
}

/// Deterministic opt-in fixture: advance exterior daylight over a repeatable
/// 120-frame cycle while geometry remains under the normal camera controller.
pub(crate) fn frame_fraction(base:Option<f64>,frame:u64,animate:bool)->Option<f64>{
    base.map(|base|if animate{(base+(frame%120) as f64*0.00001).rem_euclid(1.)}else{base})
}
/// Called after the normal camera controller, before equality, projection and
/// cache lookup. Null is a strict no-op for ordinary interactive sessions.
pub(crate) fn apply(camera:&mut Value,value:&Value)->Result<(),String>{
    if value.is_null(){return Ok(());}
    let fraction=value.as_f64().filter(|v|v.is_finite()&&(0.0..=1.0).contains(v))
        .ok_or("benchmark camera fraction must be finite within 0..1")?;
    if !camera.is_object(){return Err("benchmark camera fraction requires a camera packet".into());}
    camera["day_night"]=json!({"fraction":fraction});
    Ok(())
}

/// Hash the exact prepared packet passed to set_workstation_view, including
/// unaltered world clip positions/materials and the optical fullscreen tail.
/// to_bits preserves signed zero; no float-to-JSON round trip participates.
pub(crate) fn evidence(camera:&Value,triangles:&[[f32;24]])->Value{
    let mut bytes=Vec::with_capacity(triangles.len()*24*4);
    for triangle in triangles{for v in triangle{bytes.extend_from_slice(&v.to_bits().to_le_bytes());}}
    json!({"camera":camera,"triangles":{
        "count":triangles.len(),"byte_length":bytes.len(),
        "camera_count":triangles.iter().filter(|t|[t[15],t[19],t[23]]==[90.;3]).count(),
        "sha256":super::super::control_engraving::sha256(&bytes),
        "encoding":"Ordered triangle [f32;24], each f32.to_bits as four little-endian bytes; includes complete prepared packet and fullscreen tail",
        "scope":"Exact prepared CPU packet passed to GraphicsConnectionSystem.SetView; GPU readback is not implied"
    }})
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn animated_fixture_is_repeatable_and_normal_mode_is_unchanged(){
        assert_eq!(frame_fraction(None,123,true),None);
        assert_eq!(frame_fraction(Some(0.265),33,false),Some(0.265));
        assert_eq!(frame_fraction(Some(0.265),0,true),Some(0.265));
        assert_eq!(frame_fraction(Some(0.265),120,true),Some(0.265));
        assert_ne!(frame_fraction(Some(0.265),1,true),Some(0.265));
        for f in 0..240{let a=frame_fraction(Some(0.9999),f,true).unwrap();assert!((0.0..1.0).contains(&a));assert_eq!(a,frame_fraction(Some(0.9999),f+120,true).unwrap());}
    }
    #[test]fn absent_override_preserves_normal_camera(){
        assert_eq!(fraction(false,None).unwrap(),None);assert_eq!(fraction(true,None).unwrap(),None);
        for mut c in [Value::Null,json!({"id":"foh-left","position":[1,2,3]})]{let before=c.clone();apply(&mut c,&Value::Null).unwrap();assert_eq!(c,before);}
    }
    #[test]fn only_finite_combined_scope_accepts_override(){
        for v in ["0","1","0.26666666666666666"]{
            assert!(fraction(false,Some(v)).is_err());assert!(fraction(true,Some(v)).unwrap().is_some());
        }
        for v in ["NaN","inf","-inf","1.000001","-0.1","","text"]{assert!(fraction(true,Some(v)).is_err());}
    }
    #[test]fn actual_camera_receives_fraction_without_losing_controls(){
        let mut c=json!({"id":"foh-left","yaw":1.4,"position":[1.,2.,3.],"forward":[0.,0.,1.]});let before=c.clone();
        apply(&mut c,&json!(0.265)).unwrap();assert_eq!(c["day_night"]["fraction"],json!(0.265));
        c.as_object_mut().unwrap().remove("day_night");assert_eq!(c,before);
        for v in [json!(1.1),json!(-0.1),json!("0.5"),json!(true)]{assert!(apply(&mut c,&v).is_err());}
        assert!(apply(&mut Value::Null,&json!(0.5)).is_err());
    }
    #[test]fn evidence_hashes_every_prepared_bit_and_packet_order(){
        let c=json!({"id":"foh-left","day_night":{"fraction":0.265}});let base=[[0f32;24];2];
        let a=evidence(&c,&base);assert_eq!(a["camera"],c);assert_eq!(a["triangles"]["byte_length"],192);
        for i in 0..48{let mut t=base;t[i/24][i%24]=f32::from_bits(1);assert_ne!(a["triangles"]["sha256"],evidence(&c,&t)["triangles"]["sha256"]);}
        let mut t=base;t[0][0]=-0.;assert_ne!(a["triangles"]["sha256"],evidence(&c,&t)["triangles"]["sha256"]);
        assert_ne!(evidence(&c,&t)["triangles"]["sha256"],evidence(&c,&[t[1],t[0]])["triangles"]["sha256"]);
    }
    #[test]fn sha256_reuse_matches_independent_empty_and_zero_packet_vectors(){
        assert_eq!(evidence(&Value::Null,&[])["triangles"]["sha256"],"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(evidence(&Value::Null,&[[0.;24]])["triangles"]["sha256"],"2ea9ab9198d1638007400cd2c3bef1cc745b864b76011a0e1bc52180ac6452d4");
    }
}
