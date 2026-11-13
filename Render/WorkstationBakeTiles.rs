//! Pure bounded scheduling for immutable GPU bake generations.
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Phase{Snapshot,Panel,General,Resolve,Capture,Done}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct Tile{pub phase:Phase,pub origin:[u32;3],pub workgroups:[u32;3]}
#[derive(Clone,Copy,Debug)]pub struct Entry{pub slot:u32,pub edge:u32,pub dirty:bool,pub ready:bool}
pub struct Plan{tiles:Vec<Tile>,completed:usize,pending:Option<(u64,usize)>,cancelled:bool}
impl Plan{
 pub fn new(entries:&[Entry],rects:&[[u32;2]],stripe:u32)->Result<Self,&'static str>{
  Self::with_bounded_capture(entries,rects,stripe,0)
 }
 pub fn with_bounded_capture(entries:&[Entry],rects:&[[u32;2]],stripe:u32,bounded_groups:u32)->Result<Self,&'static str>{
  if stripe==0||stripe%8!=0||stripe>128||entries.len()>20||rects.len()>6{return Err("invalid tile bounds");}
  let mut tiles=vec![Tile{phase:Phase::Snapshot,origin:[0;3],workgroups:[0;3]}];
  for phase in [Phase::Panel,Phase::General,Phase::Resolve]{for e in entries{
   if e.slot>=20||![64,128].contains(&e.edge){return Err("invalid response entry");}
   if !e.ready||(phase!=Phase::Resolve&&!e.dirty){continue;}
   for y in (0..e.edge).step_by(stripe as usize){tiles.push(Tile{phase,origin:[0,y,e.slot],workgroups:[e.edge.div_ceil(8),stripe.min(e.edge-y).div_ceil(8),1]});}
  }}
  for (group,rect) in rects.iter().enumerate(){if rect.contains(&0)||rect.iter().any(|v|*v>16384){return Err("invalid capture rectangle");}
   let bounded=bounded_groups&(1<<group)!=0;
   let tile_width=if bounded {128}else{rect[0]};let tile_height=if bounded {16}else{stripe};
   for y in (0..rect[1]).step_by(tile_height as usize){for x in (0..rect[0]).step_by(tile_width as usize){
    tiles.push(Tile{phase:Phase::Capture,origin:[x,y,group as u32*2],workgroups:[tile_width.min(rect[0]-x).div_ceil(8),tile_height.min(rect[1]-y).div_ceil(8),2]});
   }}
  }
  Ok(Self{tiles,completed:0,pending:None,cancelled:false})
 }
 pub fn next(&self)->Option<Tile>{if self.cancelled||self.pending.is_some(){None}else{self.tiles.get(self.completed).copied()}}
 pub fn submitted(&mut self,serial:u64)->Result<(),&'static str>{if self.next().is_none(){return Err("no admissible tile");}self.pending=Some((serial,self.completed));Ok(())}
 pub fn completed(&mut self,serial:u64){if self.pending.is_some_and(|p|p.0<=serial){let (_,at)=self.pending.take().unwrap();self.completed=at+1;}}
 pub fn cancel(&mut self){self.cancelled=true;}
 pub fn can_release(&self)->bool{self.pending.is_none()}
 pub fn publishable(&self)->bool{!self.cancelled&&self.pending.is_none()&&self.completed==self.tiles.len()}
 pub fn pending_tile(&self)->Option<Tile>{self.pending.map(|(_,i)|self.tiles[i])}
 pub fn total(&self)->usize{self.tiles.len()}
 pub fn progress(&self)->usize{self.completed}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn bounded_full_panel_tiles_cover_both_samples_once(){
  let p=Plan::with_bounded_capture(&[],&[[271,37]],64,1).unwrap();let mut hits=vec![0;271*37*2];
  for t in p.tiles.iter().filter(|t|t.phase==Phase::Capture){assert!(t.workgroups[0]<=16&&t.workgroups[1]<=2);for z in 0..2{for y in t.origin[1]..t.origin[1]+t.workgroups[1]*8{for x in t.origin[0]..t.origin[0]+t.workgroups[0]*8{if x<271&&y<37{hits[(z*37*271+y*271+x)as usize]+=1;}}}}}
  assert!(hits.into_iter().all(|n|n==1));
 }
 #[test]fn only_completed_whole_generation_can_publish(){let mut p=Plan::new(&[],&[[31,65]],32).unwrap();assert_eq!(p.total(),4);let mut serial=1;while p.next().is_some(){assert!(!p.publishable());p.submitted(serial).unwrap();assert!(p.next().is_none());p.completed(serial-1);assert!(!p.can_release());p.completed(serial);serial+=1;}assert!(p.publishable());}
 #[test]fn cancellation_waits_for_pending_gpu_then_never_publishes(){let mut p=Plan::new(&[],&[[32,32]],32).unwrap();p.submitted(10).unwrap();p.cancel();assert!(p.next().is_none()&&!p.can_release()&&!p.publishable());p.completed(10);assert!(p.can_release()&&!p.publishable());}
 #[test]fn ordered_stages_preserve_clean_entries_and_cover_both_samples(){let p=Plan::new(&[Entry{slot:0,edge:128,dirty:true,ready:true},Entry{slot:2,edge:64,dirty:false,ready:true}],&[[17,35]],32).unwrap();let t=&p.tiles;assert_eq!(t.iter().filter(|x|x.phase==Phase::Panel).count(),4);assert_eq!(t.iter().filter(|x|x.phase==Phase::General).count(),4);assert_eq!(t.iter().filter(|x|x.phase==Phase::Resolve).count(),6);assert!(t.iter().filter(|x|x.phase==Phase::Capture).all(|x|x.workgroups[2]==2));let mut hits=vec![0;17*35*2];for tile in t.iter().filter(|x|x.phase==Phase::Capture){for z in 0..2{for y in tile.origin[1]..tile.origin[1]+tile.workgroups[1]*8{for x in 0..tile.workgroups[0]*8{if x<17&&y<35{hits[(z*35*17+y*17+x)as usize]+=1;}}}}}assert!(hits.iter().all(|x|*x==1));}
}
