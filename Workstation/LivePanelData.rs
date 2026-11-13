//! Data shaded directly by the retained compositor. Exempt no geometry, asset,
//! glyph-control identity, or optical input. Both packets must validate before
//! an animated display value can bypass a full gem appearance bake.
use super::{bottom_panel_scene as bottom,fader_components as fader};
pub(crate) fn live_field(rows:&[[f32;4]],at:usize,lane:usize)->bool {
 if lane>=4{return false;}
 if rows.len()>=fader::BASE+fader::ROWS && rows[fader::BASE][3]==1. {
  let n=rows[fader::BASE][0];
  if n.is_finite()&&n>=0.&&n<=70.&&n.fract()==0.&&at>=fader::BASE+fader::HEADER {
   let relative=at-fader::BASE-fader::HEADER;
   if relative/6<n as usize&&relative%6==3&&lane>=2 {
    return rows[at][lane].is_finite()&&(0. ..=1.).contains(&rows[at][lane]);
   }
  }
 }
 if rows.len()<bottom::END||rows[255][0]!=bottom::BASE as f32||rows[255][1]!=1.{return false;}
 if (bottom::BASE+3..bottom::BASE+8).contains(&at)||((bottom::WAVE..bottom::END).contains(&at)&&!(bottom::FADERS..bottom::FADERS+3).contains(&at)){return rows[at][lane].is_finite();}
 let preview=super::fader_components::EDITOR_BASE;
 if rows[bottom::BASE+5][0]==preview as f32&&(preview..preview+super::fader_components::EDITOR_ROWS).contains(&at){return rows[at][lane].is_finite();}
 false
}
#[cfg(test)]mod tests{
 use super::*;
 fn frame()->Vec<[f32;4]>{let mut r=vec![[0.;4];bottom::END.max(256)];r[255]=[bottom::BASE as f32,1.,0.,0.];r[bottom::BASE+5][0]=fader::EDITOR_BASE as f32;r[fader::BASE]=[1.,0.,0.,1.];r}
 #[test]fn level_and_display_fields_are_live_but_poses_and_labels_are_not(){let r=frame();assert!(live_field(&r,fader::BASE+11,2));assert!(!live_field(&r,fader::BASE+11,0));assert!(!live_field(&r,fader::BASE+10,2));assert!(live_field(&r,bottom::WAVE,0));assert!(live_field(&r,fader::EDITOR_BASE,0));for at in [255,bottom::BASE,bottom::BASE+1,bottom::BASE+2,bottom::POSES,bottom::LABELS]{assert!(!live_field(&r,at,0));}}
 #[test]fn invalid_or_unpublished_live_fields_fail_closed(){let mut r=frame();r[fader::BASE+11][2]=f32::NAN;assert!(!live_field(&r,fader::BASE+11,2));r[255][1]=0.;assert!(!live_field(&r,bottom::WAVE,0));assert!(!live_field(&[],0,0));}
 #[test]fn dynamic_levels_and_scope_data_do_not_invalidate_retained_optics(){let a=frame();let mut b=a.clone();b[fader::BASE+11][2]=0.7;b[bottom::WAVE][0]=0.3;b[fader::EDITOR_BASE][1]=0.4;assert!(!super::super::transport_pose::optics_changed(&a,&b,true));b[fader::BASE+10][0]+=0.01;assert!(super::super::transport_pose::optics_changed(&a,&b,true));}

}
