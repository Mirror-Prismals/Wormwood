//! Cardinal hosted automation targets: instrument first, then hosted inserts.
//! Stock/preamp processors are excluded exactly as buildDeviceRefs does.
use super::{document::{Content,State,Track},track_io::CcLane};
use serde_json::{Value,json};

pub(super) fn devices<'a>(track:&Track,catalog:&'a Value)->Vec<&'a Value>{
    if track.cc_lane==CcLane::Workspace{return crate::theatre::lighting::catalog()["devices"].as_array().unwrap().iter().collect();}
    let CcLane::Track(id)=&track.cc_lane else{return Vec::new();};
    let mut result=catalog["devices"].as_array().map_or_else(Vec::new,|rows|rows.iter()
        .filter(|device|device["track"]==*id&&device["stock"]!=true&&device["chain"].as_str().unwrap_or("playback")=="playback").collect::<Vec<_>>());
    // Stable sort retains the real effect order, including after reordering.
    result.sort_by_key(|device|device["instrument"]!=true);result
}
pub(super) fn resolved(track:&Track,catalog:&Value)->Value{
    let devices=devices(track,catalog);
    let device_slot=track.cc_device_slot.min(devices.len().saturating_sub(1));
    let device=devices.get(device_slot).copied().unwrap_or(&Value::Null);
    let parameters=device["parameters"].as_array().cloned().unwrap_or_default();
    let parameter_slot=track.cc_parameter_slot.min(parameters.len().saturating_sub(1));
    let parameter=parameters.get(parameter_slot).unwrap_or(&Value::Null);
    json!({"cc_target_device":device["id"],"cc_target_device_label":device["label"].as_str().unwrap_or("NONE"),
        "cc_target_device_slot":device_slot,"cc_parameter_slot":parameter_slot,"cc_parameter_id":parameter["id"],
        "cc_target_parameter_label":parameter["label"].as_str().unwrap_or("NONE"),"cc_parameter_writable":parameter["writable"].as_bool().unwrap_or(true),"cc_parameters":parameters,
        "cc_workspace_available":true})
}
pub(super) fn apply(state:&State,destination:&str,seconds:f64)->Result<(),String>{
    if !state.tracks.values().any(|track|track.kind=="cc"&&track.cc_lane==CcLane::Track(destination.into())){return Ok(());}
    let catalog=state.plugins.automation_catalog_cached()?;
    for id in &state.track_order {
        let track=&state.tracks[id];
        if track.kind!="cc"||track.cc_lane!=CcLane::Track(destination.into()){continue;}
        let target=resolved(track,&catalog);
        if target["cc_parameter_writable"]!=true{continue;}
        let(Some(device),Some(parameter))=(target["cc_target_device"].as_u64(),target["cc_parameter_id"].as_u64())else{continue;};
        for clip_id in &state.clip_order {
            let clip=&state.clips[clip_id];if clip.track!=*id{continue;}
            if let Content::Cc{duration,points}=&clip.content {
                if seconds<clip.start||seconds>clip.start+duration{continue;}
                let value=super::automation::evaluate(points,(seconds-clip.start).clamp(0.,*duration));
                state.plugins.automate(device,u32::try_from(parameter).map_err(|_|"Plugin parameter id exceeds u32")?,value)?;
                break; // First active clip, as AutomationLaneSystem does.
            }
        }
    }
    Ok(())
}
