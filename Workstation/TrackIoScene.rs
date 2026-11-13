//! Page-zero I/O uses the existing mini gems and the authored crosswalk model.
//! Geometry is immutable; a frame contains bounded poses, labels and palette IDs.
use super::{decorative_controls, panel_layout::Viewport, track_io};
use serde_json::Value;
use std::{io::Read,path::Path};

pub(crate) const BASE:usize=super::user_style::BASE+super::user_style::ROWS;
pub(crate) const ROWS:usize=524_288;
pub(crate) const HEADER:usize=decorative_controls::BASE+decorative_controls::ROWS-1024;
pub(crate) const POSE_BASE:usize=HEADER+4;
pub(crate) const FIRST_INSTANCE:usize=65;
pub(crate) const STRIDE:usize=5;
pub(crate) const END:usize=POSE_BASE+track_io::MAX_CONTROLS*STRIDE;
const _: () = assert!(END<=super::engraving::BASE);
const _: () = assert!(END<=super::transport_pose::META_BASE);
const DIRECTORY:&str="Art/AudioWorkstation/CrosswalkArming";
pub(crate) struct Kit {
    used:usize,root:usize,end:usize,materials:[usize;3],
    min:[f64;3],max:[f64;3],
    mini:([usize;3],[[f64;3];2]),
    wide:([usize;3],[[f64;3];2]),
}
impl Kit {
    pub(crate) fn load(root:&Path)->Result<Self,String>{
        let bytes=std::fs::read(root.join(DIRECTORY).join("runtime.json")).map_err(|e|e.to_string())?;
        let value:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        let integer=|key:&str|value[key].as_u64().and_then(|n|usize::try_from(n).ok()).ok_or_else(||format!("invalid crosswalk {key}"));
        let kit=Self {used:integer("rows")?,root:integer("root")?,end:integer("end")?,
            materials:serde_json::from_value(value["material_bases"].clone()).map_err(|e|e.to_string())?,
            min:serde_json::from_value(value["min"].clone()).map_err(|e|e.to_string())?,
            max:serde_json::from_value(value["max"].clone()).map_err(|e|e.to_string())?,
            wide:super::timeline_scene::Kit::load(root)?.bottom_controls()[0],
            mini:super::timeline_scene::Kit::load(root)?.mini_control()};
        let materials=integer("material_count")?;
        if integer("schema")?!=1||integer("base")?!=BASE||integer("capacity")?!=ROWS
            ||kit.used>ROWS||materials==0||materials>=256||integer("triangles")?!=48022
            ||kit.materials!=[BASE,BASE+materials*40,BASE+materials*80]
            ||kit.root!=BASE+materials*120||kit.end<=kit.root||(kit.end-kit.root)%3!=0
            ||kit.end+48022*6!=BASE+kit.used
            ||kit.min.iter().chain(&kit.max).any(|n|!n.is_finite())||(0..3).any(|i|kit.min[i]>=kit.max[i]) {
            return Err("invalid authored crosswalk package".into());
        }
        Ok(kit)
    }
    pub(crate) fn read_geometry(&self,root:&Path)->Result<Vec<[f32;4]>,String>{
        let mut bytes=Vec::new();
        std::fs::File::open(root.join(DIRECTORY).join("runtime.bin")).map_err(|e|e.to_string())?
            .take((self.used*16+1) as u64).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        if bytes.len()!=self.used*16 {return Err("crosswalk geometry length mismatch".into());}
        let rows:Vec<[f32;4]>=bytes.chunks_exact(16).map(|b|std::array::from_fn(|i|f32::from_le_bytes(b[i*4..i*4+4].try_into().unwrap()))).collect();
        if rows.iter().flatten().any(|v|!v.is_finite()){return Err("nonfinite crosswalk geometry".into());}
        let address=|f:f32|f>=BASE as f32&&f<=(BASE+self.used) as f32&&f.fract()==0.;
        for at in (self.root..self.end).step_by(3){let a=rows[at-BASE];let b=rows[at+1-BASE];let n=rows[at+2-BASE][0];
            if !address(a[3])||!address(b[3])||a[3] as usize<=at||a[3] as usize>self.end
                ||(a[3] as usize-self.root)%3!=0||!(0. ..=4.).contains(&n)||n.fract()!=0.
                ||(b[3] as usize)<self.end||(b[3] as usize-self.end)%6!=0
                ||b[3] as usize+n as usize*6>BASE+self.used||(0..3).any(|i|a[i]>b[i]) {return Err("invalid crosswalk BVH node".into());}
        }
        Ok(rows)
    }
    pub(crate) fn append(&self,rows:&mut [[f32;4]],view:Viewport,value:&Value)->Result<(),String>{
        if rows.len()<END {return Err("track I/O requires complete frame storage".into());}
        let mut packet=vec![[0.;4];END-HEADER];
        if value.is_null()||value["page"]!=0 && value["page"]!=1 {rows[HEADER..END].copy_from_slice(&packet);return Ok(());}
        let panel:[f64;4]=serde_json::from_value(value["panel"].clone()).map_err(|e|e.to_string())?;
        if panel.iter().any(|n|!n.is_finite()||n.abs()>1e6){return Err("invalid track I/O panel".into());}
        let controls=value["controls"].as_array().filter(|v|v.len()<=track_io::MAX_CONTROLS).ok_or("invalid track I/O controls")?;
        // An empty page has no clip consumers. Keep its optical packet stable
        // while the panel animation advances, instead of forcing a useless rebake.
        if controls.is_empty() { rows[HEADER..END].copy_from_slice(&packet); return Ok(()); }
        let mpp=1.3/view.height;
        let world=|x:f64,y:f64|[(x-view.width*0.5)*mpp,(view.height*0.5-y)*mpp];
        let low=world(panel[0],panel[1]+panel[3]);let high=world(panel[0]+panel[2],panel[1]);
        packet[3]=[low[0] as f32,low[1] as f32,high[0] as f32,high[1] as f32];
        let mut lo=[f64::INFINITY;3];let mut hi=[f64::NEG_INFINITY;3];
        for (index,control) in controls.iter().enumerate(){
            let rect:[f64;4]=serde_json::from_value(control["rect"].clone()).map_err(|e|e.to_string())?;
            if rect.iter().any(|n|!n.is_finite()||n.abs()>1e6)||rect[2]<=0.||rect[3]<=0.{return Err("invalid track I/O control rect".into());}
            let crosswalk=control["crosswalk"].as_bool().unwrap_or(control["key"]=="arm");
            let palette=control["crosswalk_palette"].as_u64().filter(|n|*n<3).ok_or("invalid crosswalk palette")? as usize;
            let (mut tree,min,max)=if crosswalk {([self.root,self.end,self.materials[palette]],self.min,self.max)}else if (control["key"]=="loop"||control["key"]=="learn"||control["key"]=="theatre"||control["key"]=="drum_rack"||control["key"]=="audio_rays"||control["key"]=="headset_view"||control["key"]=="import_audio"||control["key"]=="import_plugins"||control["key"]=="lane_layout"){(self.wide.0,self.wide.1[0],self.wide.1[1])}else{(self.mini.0,self.mini.1[0],self.mini.1[1])};
            // Latched mute selects baked colorless ruby artwork, independent of pointer pressure.
            if !crosswalk && control["key"]=="mute" && control["active"]==true {tree[2]=12704;}
            // Channel colors select persistent offline artwork; no runtime optics.
            if !crosswalk && matches!(control["key"].as_str(),Some("bus_input"|"output_l"|"output_r")) {
                tree[2]=match control["text"].as_str().unwrap_or("").strip_prefix("IN ").unwrap_or(control["text"].as_str().unwrap_or("")) {
                    "L"=>12744,"FF"=>12784,"S"=>12824,"OFF" if control["key"]!="bus_input"=>12704,_=>tree[2],
                };
            }
            // Crosswalk housing is 160 source units (80 mm); its visor remains
            // authored geometry. Match the source housing's square hit region.
            let scale=if crosswalk {rect[2].min(rect[3])*mpp/0.080}
                else{(rect[2]*mpp/(max[0]-min[0])).min(rect[3]*mpp/(max[1]-min[1]))};
            let pressed=control["pressed"]==true||control["active"]==true;
            let center=world(rect[0]+rect[2]*0.5,rect[1]+rect[3]*0.5+if pressed{0.75}else{0.});
            let font=9f64.min((rect[2]-4.)/(0.55*control["text"].as_str().unwrap_or("").chars().count().max(1) as f64));
            let at=4+index*STRIDE;
            packet[at]=[center[0] as f32,center[1] as f32,(-min[2]*scale+0.0005) as f32,scale as f32];
            packet[at+1]=[tree[0] as f32,tree[1] as f32,if crosswalk{0.}else{(font*mpp/scale) as f32},tree[2] as f32];
            for (i,c) in control["text"].as_str().unwrap_or("").chars().take(12).enumerate(){packet[at+2+i/4][i%4]=if c.is_ascii()&&!c.is_ascii_control(){c as u8 as f32}else{63.};}
            for axis in 0..3 {lo[axis]=lo[axis].min(packet[at][axis] as f64+min[axis]*scale);hi[axis]=hi[axis].max(packet[at][axis] as f64+max[axis]*scale);}
        }
        if !controls.is_empty(){for axis in 0..3 {packet[1][axis]=((lo[axis]+hi[axis])*0.5) as f32;packet[2][axis]=((hi[axis]-lo[axis])*0.5+1e-6) as f32;}}
        packet[0]=[POSE_BASE as f32,controls.len() as f32,0.,0.];
        rows[HEADER..END].copy_from_slice(&packet);Ok(())
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn routing_gems_select_channel_art_without_coloring_other_controls(){
        let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut rows=vec![[0.;4];END];
        for key in ["bus_input","output_l","output_r","bus_channel","mute"] {
            for (channel,material) in [("L",12744),("FF",12784),("S",12824),("R",kit.mini.0[2]),("OFF",kit.mini.0[2])] {
                let text=if key=="bus_input"{format!("IN {channel}")}else{channel.into()};
                kit.append(&mut rows,Viewport{width:1600.,height:1000.,canvas_scale:1.},&serde_json::json!({"page":0,"panel":[1200.,18.,400.,950.],"controls":[{"key":key,"text":text,"rect":[1240.,242.,36.,36.],"crosswalk":false,"crosswalk_palette":0}]})).unwrap();
                let expected=if channel=="OFF" && matches!(key,"output_l"|"output_r"){12704}else if matches!(key,"bus_input"|"output_l"|"output_r"){material}else{kit.mini.0[2]};
                assert_eq!(rows[POSE_BASE+1][3],expected as f32,"{key} {channel}");
            }
        }
    }
    #[test] fn empty_page_does_not_publish_moving_clip_bounds(){
        let kit=Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut rows=vec![[99.;4];super::super::engraving::BASE+super::super::engraving::ROWS];
        for x in [1200.,800.] {
            kit.append(&mut rows,Viewport{width:1600.,height:1000.,canvas_scale:1.},&serde_json::json!({"page":0,"panel":[x,18.,400.,950.],"controls":[]})).unwrap();
            assert!(rows[HEADER..END].iter().all(|r|*r==[0.;4]));
        }
    }
    #[test] fn authored_crosswalk_packet_keeps_geometry_and_three_led_palettes(){
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));let kit=Kit::load(root).unwrap();let data=kit.read_geometry(root).unwrap();
        assert_eq!(kit.end+48022*6,BASE+data.len());
        let mut emissions=[0.;3];let n=(kit.materials[1]-kit.materials[0])/40;
        for mode in 0..3{for mat in 0..n{emissions[mode]+=data[kit.materials[mode]-BASE+mat*40+5][0];}}
        assert_eq!(emissions[0],0.);assert!(emissions[1]>0.&&emissions[2]>0.);
        let mut rows=vec![[99.;4];super::super::engraving::BASE+super::super::engraving::ROWS];
        kit.append(&mut rows,Viewport{width:1600.,height:1000.,canvas_scale:1.},&serde_json::json!({"page":0,"panel":[1200.,18.,400.,950.],"controls":[{"key":"arm","rect":[1240.,242.,36.,36.],"crosswalk_palette":1}]})).unwrap();
        assert_eq!(rows[HEADER][1],1.);assert_eq!(rows[POSE_BASE+1][3],kit.materials[1] as f32);
        assert_eq!(rows[HEADER-1],[99.;4]);assert_eq!(rows[END],[99.;4]);
        kit.append(&mut rows,Viewport{width:1600.,height:1000.,canvas_scale:1.},&Value::Null).unwrap();
        assert!(rows[HEADER..END].iter().all(|r|*r==[0.;4]));
    }
}
