//! Existing Cardinal audio-track LANES / EDIT LANES context-menu actions.
//! DawLaneInputSystem.cpp pages 0, 3 and1; native popup reuses the plugin menu
//! boundary. Menu selection never makes an alternate audio lane audible.
use crate::host::Host;
use serde_json::{Value,json};
const BACK:i64=1;
const LANES:i64=2;
const EDIT:i64=3;
const TOGGLE_EXTRA:i64=4;
const TOGGLE_RECORD:i64=5;
const ADD:i64=6;
const DUPLICATE:i64=7;
const RENAME:i64=8;
const DELETE:i64=9;
const PREVIOUS:i64=10;
const MORE:i64=11;
const SELECT:i64=1000;
fn rows(page:usize,offset:usize,state:&Value)->Vec<Value>{
    let mut out=Vec::new();
    let mut row=|value:i64,label:&str,enabled:bool|out.push(json!({"value":value,"label":label,"enabled":enabled}));
    let expanded=state["expanded"]==true;
    let recording=state["record_controls_visible"]==true;
    if page!=0{row(BACK,"< TRACK",true);}
    if page==1{
        row(TOGGLE_EXTRA,if expanded{"HIDE PLAYLIST LANES"}else{"SHOW PLAYLIST LANES"},true);
        row(ADD,"ADD LANE",true);row(DUPLICATE,"DUPLICATE LANE",true);row(RENAME,"RENAME LANE",true);
        row(DELETE,"DELETE LANE",state["selected"].as_u64().unwrap_or(0)>0);
        if let Some(lanes)=state["lanes"].as_array(){
            let first=offset.min(lanes.len().saturating_sub(1)/10*10);
            if first>0{row(PREVIOUS,"< PREVIOUS PLAYLISTS",true);}
            for (index,lane) in lanes.iter().enumerate().skip(first).take(10){
                let label=format!("{}{}",if lane["active"]==true{"* "}else{"  "},lane["name"].as_str().unwrap_or(""));
                row(SELECT+index as i64,&label,true);
            }
            if first+10<lanes.len(){row(MORE,"MORE PLAYLISTS >",true);}
        }
    }else{
        if page==0{row(LANES,"LANES...",true);}
        row(TOGGLE_EXTRA,if expanded{"HIDE EXTRA LANES"}else{"SHOW EXTRA LANES"},true);
        row(TOGGLE_RECORD,if recording{"HIDE RECORDING TARGETS"}else{"SHOW RECORDING TARGETS"},true);
        row(ADD,"ADD LANE",true);
        if page==3{row(EDIT,"EDIT LANES...",true);}
    }
    out
}
/// Menus stay open across source actions; cancellation closes the session.
/// Each mutation obtains a fresh revision after the modal OS popup returns.
pub(crate) fn open(host:&mut Host,track:&str)->Result<(),String>{
    let mut page=0;let mut offset=0;
    loop{
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        let state=snapshot["audio_playlists"].as_array().and_then(|rows|rows.iter().find(|row|row["track"]==track)).ok_or("audio playlist track no longer exists")?;
        let choice=host.invoke("PluginConnectionSystem.Menu",json!({"items":rows(page,offset,state)}))?["selection"].as_i64().unwrap_or(-1);
        if choice<0{return Ok(());}
        match choice{BACK=>{page=0;offset=0;continue;},LANES=>{page=3;offset=0;continue;},EDIT=>{page=1;offset=0;continue;},PREVIOUS=>{offset=offset.saturating_sub(10);continue;},MORE=>{offset+=10;continue;},_=>{}}
        let selected=state["selected"].as_u64().unwrap_or(0);
        let mut command=json!({"id":track,"index":selected});
        command["action"]=json!(match choice{TOGGLE_EXTRA=>"toggle_lanes",TOGGLE_RECORD=>"toggle_record_controls",ADD=>"new",DUPLICATE=>"duplicate",RENAME=>"rename",DELETE=>"delete",n if n>=SELECT=>"select",_=>return Err("unknown lane menu action".into())});
        if choice>=SELECT{command["index"]=json!(choice-SELECT);}
        if choice==RENAME{
            let name=state["lanes"][selected as usize]["name"].as_str().unwrap_or("");
            let reply=host.invoke("PluginConnectionSystem.Menu",json!({"text_entry":{"title":"RENAME LANE","value":name}}))?;
            if reply["cancelled"]==true{continue;}
            command["name"]=reply["name"].clone();
        }
        command["document_revision"]=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
        host.invoke("WorkstationDataSystem.AudioPlaylist",command)?;
    }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_audio_pages_have_exact_lane_actions_and_protect_main(){let s=json!({"selected":0,"lanes":[{"name":"Main","active":true}]});let main=rows(0,0,&s);assert_eq!(main[0]["label"],"LANES...");let edit=rows(1,0,&s);assert_eq!(edit[5]["label"],"DELETE LANE");assert_eq!(edit[5]["enabled"],false);assert_eq!(edit[6]["label"],"* Main");assert!(!edit.iter().any(|r|r["label"].as_str().unwrap().contains("CLIP")));}
 #[test]fn source_lane_list_pages_ten_at_a_time(){let s=json!({"selected":2,"lanes":(0..22).map(|i|json!({"name":format!("Lane {}",i+1),"active":i==0})).collect::<Vec<_>>()});let rows=rows(1,10,&s);assert!(rows.iter().any(|r|r["value"]==PREVIOUS));assert!(rows.iter().any(|r|r["value"]==MORE));assert_eq!(rows.iter().filter(|r|r["value"].as_i64().unwrap()>=SELECT).count(),10);}
}
