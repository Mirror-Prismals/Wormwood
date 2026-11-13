//! Tracks Entity instances without imposing scene containers.
use crate::{base_entity::Entity, host::Host};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(crate) struct State {
    next: u64,
    owners: BTreeMap<u64, String>,
    entities: BTreeMap<u64, Arc<Entity>>,
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "Create" => {
            let id = host.entities.next + 1;
            let entity = Entity::from_spec(id, input)?;
            host.emit(json!({"event":"entity_created","entity":id,"appearance":entity.data()}))?;
            host.entities
                .owners
                .insert(id, host.invoking_system().to_owned());
            host.entities.next = id;
            host.entities.entities.insert(id, Arc::new(entity));
            Ok(json!({"entity":id}))
        }
        "Update" => {
            let id = input["entity"].as_u64().ok_or("Entity ID required")?;
            let replacement = Entity::from_spec(id, input)?;
            let old = host.entities.entities.get(&id).ok_or("unknown Entity")?;
            if old.shader() != replacement.shader() {
                return Err("updating a prepared Entity cannot change shader".into());
            }
            host.emit(json!({"event":"entity_updated","entity":id,"vertices":input["vertices"].as_array().map(Vec::len),"procedural_edits":input["procedural"]["edits"].as_array().map(Vec::len),"procedural_debris_vertices":input["procedural"]["debris"].as_array().map(Vec::len)}))?;
            host.entities.entities.insert(id, Arc::new(replacement));
            Ok(Value::Null)
        }
        "Get" => {
            let id = input["entity"].as_u64().ok_or("Entity ID required")?;
            host.entities
                .entities
                .get(&id)
                .map(|entity| entity.data())
                .ok_or_else(|| format!("unknown Entity {id}"))
        }
        "Remove" => {
            let id = input["entity"].as_u64().ok_or("Entity ID required")?;
            if !host.entities.entities.contains_key(&id) {
                return Err(format!("unknown Entity {id}"));
            }
            host.emit(json!({"event":"entity_removed","entity":id}))?;
            host.entities.entities.remove(&id);
            host.entities.owners.remove(&id);
            Ok(Value::Null)
        }
        _ => Err("unknown Entity hook".into()),
    }
}

impl State {
    pub(crate) fn snapshot(&self,id:u64)->Result<Entity,String> {
        self.shared_snapshot(id).map(|entity|entity.as_ref().clone())
    }
    pub(crate) fn shared_snapshot(&self,id:u64)->Result<Arc<Entity>,String> {
        self.entities.get(&id).cloned().ok_or_else(||format!("unknown Entity {id}"))
    }
    pub(crate) fn update_player_vertices(&mut self,id:u64,vertices:Vec<[f32;6]>)->Result<(),String>{
        let old=self.entities.get(&id).ok_or("unknown Entity")?;
        let crate::base_entity::Appearance::Visual{vertices:prior,shader,..}=&old.appearance else{return Err("player Entity must be visual".into());};
        if shader!="player_body" || old.shared_procedural.is_none() || vertices.len()!=prior.len()
            || vertices.iter().flatten().any(|v|!v.is_finite()) {
            return Err("invalid numeric player skin or topology change".into());
        }
        let replacement=Entity{id,appearance:crate::base_entity::Appearance::Visual{vertices,shader:shader.clone(),procedural:Value::Null},
            material_vertices:None,shared_procedural:old.shared_procedural.clone(),water_packet:None};
        self.entities.insert(id,Arc::new(replacement));Ok(())
    }
    pub(crate) fn update_material(&mut self,id:u64,mut procedural:Value,vertices:Vec<[f64;6]>)->Result<(),String> {
        if vertices.len()>300000 || vertices.len()%3!=0 || vertices.iter().flatten().any(|v|!v.is_finite()||v.abs()>=1e10) {
            return Err("invalid fragment appearance vertices".into());
        }
        let old=self.entities.get(&id).ok_or("unknown Entity")?;
        let crate::base_entity::Appearance::Visual {vertices:base,shader,..}=&old.appearance else {return Err("material Entity must be visual".into());};
        if shader!="planet_surface" {return Err("material update requires the planet shader".into());}
        // The public adapter omits a null cave profile. Compare that same
        // canonical envelope; equal data retains its previous validation.
        if procedural["underground"].is_null(){if let Some(o)=procedural.as_object_mut(){o.remove("underground");}}
        if old.visual_procedural()==Some(&procedural) {
            if old.material_vertices.as_ref().is_some_and(|prior|prior.len()==vertices.len()&&prior.iter().zip(&vertices).all(|(a,b)|a.map(f64::to_bits)==b.map(f64::to_bits))){return Ok(());}
            let mut next=old.as_ref().clone();next.material_vertices=Some(Arc::new(vertices));self.entities.insert(id,Arc::new(next));return Ok(());
        }
        // Validate the ordinary static envelope once, with the bulk skin kept
        // numeric. The JSON adapter reconstructs the identical public packet.
        let mut next=Entity::from_spec(id,&json!({"kind":"visual","shader":shader,"vertices":base,"procedural":procedural}))?;
        next.material_vertices=Some(std::sync::Arc::new(vertices));
        self.entities.insert(id,Arc::new(next));
        Ok(())
    }
    pub(crate) fn publish_water(&mut self,existing:Option<u64>,packet:std::sync::Arc<crate::water::presentation::Packet>,owner:String)->Result<u64,String>{
        if packet.rows.is_empty()||packet.rows.len()>24576||packet.rows.len()%3!=0{return Err("water Entity vertex budget".into());}
        crate::water_render_data::validate_numeric(&packet)?;
        let id=existing.unwrap_or(self.next+1);
        if let Some(id)=existing{if self.entities.get(&id).and_then(|e|e.shader())!=Some("water_surface"){return Err("updating water requires existing water Entity".into());}}
        let vertices=packet.rows.iter().map(|r|[r[0][0]as f32,r[0][1]as f32,r[0][2]as f32,1.,1.,1.]).collect();
        let entity=Entity{id,appearance:crate::base_entity::Appearance::Visual{vertices,shader:"water_surface".into(),procedural:Value::Null},material_vertices:None,shared_procedural:None,water_packet:Some(packet)};
        if existing.is_none(){self.next=id;self.owners.insert(id,owner);}
        self.entities.insert(id,Arc::new(entity));Ok(id)
    }
    pub(crate) fn owned_by(&self, system: &str) -> bool {
        self.owners.values().any(|s| s == system)
    }
    pub(crate) fn len(&self) -> usize {
        self.entities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_material_entity_keeps_immutable_skin_and_json_compatibility() {
        let mut state=State::default();
        let spec=json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[]}});
        state.entities.insert(1,Arc::new(Entity::from_spec(1,&spec).unwrap()));
        let skin=vec![[0.,0.,0.,0.2,0.3,0.4],[1.,0.,0.,0.2,0.3,0.4],[0.,1.,0.,0.2,0.3,0.4]];
        state.update_material(1,spec["procedural"].clone(),skin.clone()).unwrap();
        let first=state.snapshot(1).unwrap();
        let second=state.snapshot(1).unwrap();
        assert!(std::sync::Arc::ptr_eq(first.material_vertices.as_ref().unwrap(),second.material_vertices.as_ref().unwrap()));
        assert_eq!(first.data()["procedural"]["debris"],json!(skin));
        Entity::from_spec(1,&first.data()).unwrap();
        assert!(state.update_material(1,spec["procedural"].clone(),vec![[f64::NAN;6];3]).is_err());
        assert_eq!(state.snapshot(1).unwrap().data(),first.data());
        state.update_material(1,spec["procedural"].clone(),vec![]).unwrap();
        assert_eq!(first.material_vertices.as_ref().unwrap().len(),3);
        assert!(state.snapshot(1).unwrap().material_vertices.unwrap().is_empty());
    }
    #[test]
    fn scene_cpu_shared_entity_reads_reuse_storage_and_preserve_old_snapshots() {
        let mut state=State::default();
        let spec=json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[]}});
        state.entities.insert(1,Arc::new(Entity::from_spec(1,&spec).unwrap()));
        let a=state.shared_snapshot(1).unwrap();let b=state.shared_snapshot(1).unwrap();
        assert!(Arc::ptr_eq(&a,&b));assert_eq!(a.data(),state.snapshot(1).unwrap().data());
        let expected=a.data();
        state.update_material(1,spec["procedural"].clone(),vec![[0.,0.,0.,0.2,0.3,0.4];3]).unwrap();
        let next=state.shared_snapshot(1).unwrap();assert!(!Arc::ptr_eq(&a,&next));assert_eq!(a.data(),expected);
        state.entities.remove(&1);assert!(state.shared_snapshot(1).is_err());assert_eq!(a.data(),expected);
    }

}

#[cfg(test)]mod player_numeric_tests{
 use super::*;
 fn spec(vertices:Vec<[f32;6]>)->Value{json!({"kind":"visual","shader":"player_body","vertices":vertices,"procedural":{"uvs":[[0.,0.],[1.,0.],[0.,1.]],"head_triangles":[false],"texture":"Procedures/Assets/Player/prismal_player.rgba"}})}
 #[test]fn numeric_player_update_matches_json_and_preserves_prior_reader_and_shared_uvs(){
  let first=vec![[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]];
  let mut state=State::default();state.entities.insert(1,Arc::new(Entity::from_spec(1,&spec(first)).unwrap()));
  let reader=state.shared_snapshot(1).unwrap();let old=reader.data();
  let moved=vec![[0.1,0.2,-0.3,1.,1.,1.],[1.1,0.2,-0.3,1.,1.,1.],[0.1,1.2,-0.3,1.,1.,1.]];
  let expected=Entity::from_spec(1,&spec(moved.clone())).unwrap().data();
  state.update_player_vertices(1,moved).unwrap();let current=state.shared_snapshot(1).unwrap();
  assert_eq!(current.data(),expected);assert_eq!(reader.data(),old);
  assert!(Arc::ptr_eq(reader.shared_procedural.as_ref().unwrap(),current.shared_procedural.as_ref().unwrap()));
  assert!(state.update_player_vertices(1,vec![[f32::NAN;6];3]).is_err());assert_eq!(state.shared_snapshot(1).unwrap().data(),expected);
  assert!(state.update_player_vertices(1,vec![]).is_err());assert_eq!(state.shared_snapshot(1).unwrap().data(),expected);
 }
}

#[cfg(test)] mod numeric_water_entity_tests {
 use super::*;
 #[test] fn numeric_water_entity_preserves_json_snapshot_and_failed_update(){
  let rows=vec![[[0.,0.,-2.,1.],[0.,1.,0.,0.],[0.,1.,0.,0.],[0.,1.,0.,0.],[0.,0.,0.,0.],[1.,1.,2.,0.]];3];
  let packet=crate::water::presentation::Packet{frame:crate::water::math::Frame::radial([0.,6_371_000.,0.],[0.,0.,-1.]),rows:Arc::new(rows),indices:Arc::new(vec![0,1,2]),optics:json!({"time":0.,"murkiness":0.,"sky_color_intensity":0.5,"neutral_reflection_intensity":1.,"light":[1.,1.,1.],"sky_top":[0.2,0.3,0.4],"sky_bottom":[0.4,0.5,0.6],"planar_reflection":false,"mean_height":0.,"micro_phase":[0.,0.,0.]}),ripples:json!([])};
  let mut state=State::default();let id=state.publish_water(None,Arc::new(packet.clone()),"WaterRenderSystem".into()).unwrap();let before=state.shared_snapshot(id).unwrap();
  let data=before.data();assert_eq!(data["procedural"],packet.json());let roundtrip=Entity::from_spec(id,&data).unwrap();assert_eq!(roundtrip.data(),data);
  let mut next=packet.clone();next.optics["time"]=json!(1.);state.publish_water(Some(id),Arc::new(next),"WaterRenderSystem".into()).unwrap();assert_eq!(before.data(),data);
  assert!(Arc::ptr_eq(&before.water_packet.as_ref().unwrap().rows,&state.shared_snapshot(id).unwrap().water_packet.as_ref().unwrap().rows));
  let accepted=state.shared_snapshot(id).unwrap();let mut bad=packet;Arc::make_mut(&mut bad.indices)[0]=999;assert!(state.publish_water(Some(id),Arc::new(bad),"WaterRenderSystem".into()).is_err());assert!(Arc::ptr_eq(&accepted,&state.shared_snapshot(id).unwrap()));
 }
}

#[cfg(test)]mod unchanged_material_tests {
 use super::*;
 #[test]fn unchanged_material_reuses_validated_entity_but_edits_and_invalid_data_do_not(){
  let mut state=State::default();let profile=json!({"edits":[],"debris":[]});let spec=json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":profile});state.entities.insert(1,Arc::new(Entity::from_spec(1,&spec).unwrap()));
  state.update_material(1,profile.clone(),vec![]).unwrap();let accepted=state.shared_snapshot(1).unwrap();state.update_material(1,json!({"edits":[],"debris":[],"underground":null}),vec![]).unwrap();assert!(Arc::ptr_eq(&accepted,&state.shared_snapshot(1).unwrap()));
  let mut invalid=profile.clone();invalid["underground"]=json!({"schema_version":999});assert!(state.update_material(1,invalid,vec![]).is_err());assert!(Arc::ptr_eq(&accepted,&state.shared_snapshot(1).unwrap()));
  assert!(state.update_material(1,profile.clone(),vec![[f64::NAN;6];3]).is_err());assert!(Arc::ptr_eq(&accepted,&state.shared_snapshot(1).unwrap()));
  let changed=json!({"edits":[{"normal":[0.,1.,0.],"radius":2.,"depth":0.1}],"debris":[]});state.update_material(1,changed.clone(),vec![]).unwrap();assert!(!Arc::ptr_eq(&accepted,&state.shared_snapshot(1).unwrap()));assert_eq!(state.shared_snapshot(1).unwrap().data()["procedural"],changed);
 }
}
