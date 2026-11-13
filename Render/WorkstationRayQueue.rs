//! Opt-in storage-layout experiment. Ray arithmetic, order, budgets and ties
//! stay canonical; native pixels and timings decide whether this is useful.
pub(super) fn selected(source: &str) -> Result<String, String> {
    match std::env::var("WORMWOOD_WORKSTATION_RAY_QUEUE").as_deref() {
        Ok("soa") => transform(source),
        _ => Ok(source.to_owned()),
    }
}
fn transform(source: &str) -> Result<String, String> {
    let declaration = "var pending:array<GemRay,16>;var count:i32=0;";
    let n = source.matches(declaration).count();
    if n == 0 { return Err("ray queue canonical declaration missing".into()); }
    let mut out = source.replace(declaration,
        "var pending_origin:array<vec3<f32>,16>;var pending_medium:array<i32,16>;var pending_direction:array<vec3<f32>,16>;var pending_depth:array<i32,16>;var pending_weight:array<vec3<f32>,16>;var pending_priority:array<f32,16>;var count:i32=0;");
    let get = |index: &str| format!("GemRay(pending_origin[{index}],pending_medium[{index}],pending_direction[{index}],pending_depth[{index}],pending_weight[{index}],pending_priority[{index}])");
    let put = |index: &str, value: &str| ["origin", "medium", "direction", "depth", "weight", "priority"].iter()
        .map(|field| format!("pending_{field}[{index}]={value}.{field};")).collect::<String>();
    for (from, to, count) in [
        ("pending[candidate].priority".to_owned(), "pending_priority[candidate]".to_owned(), n),
        ("current=pending[strongest];".to_owned(), format!("current={};", get("strongest")), n),
        ("pending[strongest]=swapRay;".to_owned(), put("strongest", "swapRay"), n),
        ("pending[count]=transmission;".to_owned(), put("count", "transmission"), n*2),
        ("pending[count]=reflection;".to_owned(), put("count", "reflection"), n*2),
        ("current=pending[count];".to_owned(), format!("current={};", get("count")), n),
    ] {
        if out.matches(&from).count() != count { return Err(format!("ray queue marker changed: {from}")); }
        out=out.replace(&from,&to);
    }
    if out.contains("pending[") { return Err("unhandled canonical ray queue access".into()); }
    Ok(out)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn full_production_retained_shader_validates_with_all_canonical_ray_queues() {
        let original=include_str!("../Procedures/Shaders/workstation.wgsl");
        for compact in [false,true] {
            let mut source=super::super::source::source(original,true,false,compact).unwrap();
            super::super::source::append_meter_base(&mut source).unwrap();
            let new=transform(&source).unwrap();
            assert!(new.matches("var pending_origin:").count()>=3);
            assert_eq!(source.matches("event<64").count(),new.matches("event<64").count());
            assert_eq!(source.matches("count<16").count(),new.matches("count<16").count());
            assert_eq!(source.matches("weight>strongestWeight").count(),new.matches("weight>strongestWeight").count());
            let m=naga::front::wgsl::parse_str(&new).unwrap();
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();
        }
    }
    #[test] fn unexpected_queue_access_or_missing_store_fails_closed() {
        let source=include_str!("../Procedures/Shaders/workstation.wgsl");
        assert!(transform(&source.replace("pending[count]=reflection;","pending[count-1]=reflection;")).is_err());
        assert!(transform("fn empty() {}").is_err());
    }
    #[test] fn data_layout_preserves_every_ray_bit_including_priority_ties() {
        #[derive(Clone,Copy,Debug,PartialEq)] struct Ray([u32;12]);
        let mut aos=[Ray([0;12]);16];let mut soa=[[0u32;16];12];let mut seed=0x9237_ab11u32;
        for step in 0..10000 {
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let at=(seed%16)as usize;
            let mut ray=Ray([0;12]);for bits in &mut ray.0{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);*bits=seed;}
            ray.0[11]=[0.,-0.,0.2,0.2,1.,f32::NAN,f32::INFINITY][step%7].to_bits();
            aos[at]=ray;for field in 0..12{soa[field][at]=ray.0[field];}
            for i in 0..16{assert_eq!(aos[i],Ray(std::array::from_fn(|field|soa[field][i])));}
            let strongest=|read:&dyn Fn(usize)->f32|{let mut index=None;let mut weight=0f32;for i in 0..16{let value=read(i);if value>weight{weight=value;index=Some(i);}}index};
            assert_eq!(strongest(&|i|f32::from_bits(aos[i].0[11])),strongest(&|i|f32::from_bits(soa[11][i])));
        }
    }
}
