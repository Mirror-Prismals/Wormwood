//! Accepted continuous terrain and material surfaces for bounded water collision.
use crate::{terrain, water::math::*};
use serde_json::{Value, json};
#[derive(Default, Clone)]
pub(crate) struct Cache {
    key: Vec<u8>,
    terrain_grid:Option<([u64;3],Vec<[[f64;6];3]>)>,
    anchor: Option<V>,
    cave_profile: Value,
    cave: Option<std::sync::Arc<terrain::underground::Field>>,
    revision: u64,
    base: Option<crate::water::types::Snapshot>,
    material_source: Vec<[f64; 3]>,
    material_base_revision: u64,
    material_snapshot: Option<crate::water::types::Snapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_material_snapshots_reuse_base_and_invalidate_moving_debris() {
        let mut cache = Cache::default();
        let frame = json!({"edits":[],"debris":[],"underground":null});
        let eye = [0., terrain::RADIUS + 2., 0.];
        let triangle = vec![
            [0., terrain::RADIUS + 1., 0.],
            [0.1, terrain::RADIUS + 1., 0.],
            [0., terrain::RADIUS + 1., 0.1],
        ];
        let first = cache
            .capture_numeric_material(eye, &frame, &Value::Null, triangle.clone())
            .unwrap();
        let base_revision = cache.base.as_ref().unwrap().revision;
        let second = cache
            .capture_numeric_material(eye, &frame, &Value::Null, triangle.clone())
            .unwrap();
        assert_eq!(first.revision, second.revision);
        assert!(
            cache.base.as_ref().unwrap().vertices.len()
                < cache.base.as_ref().unwrap().triangles.len() * 3
        );
        assert_eq!(
            first.triangles.len(),
            cache.base.as_ref().unwrap().triangles.len() + 1
        );
        let moved: Vec<_> = triangle.iter().map(|p| [p[0], p[1] + 0.1, p[2]]).collect();
        let third = cache
            .capture_numeric_material(eye, &frame, &Value::Null, moved)
            .unwrap();
        assert!(third.revision > first.revision);
        assert_eq!(base_revision, cache.base.as_ref().unwrap().revision);
        assert_ne!(first.vertices.last(), third.vertices.last());
        crate::water::types::Snapshot::parse(&third.packet()).unwrap();
        let empty = cache
            .capture_numeric_material(eye, &frame, &Value::Null, vec![])
            .unwrap();
        assert_eq!(
            empty.triangles.len(),
            cache.base.as_ref().unwrap().triangles.len()
        );
        assert!(
            cache
                .capture_numeric_material(eye, &frame, &Value::Null, vec![[f64::NAN; 3]; 3])
                .is_err()
        );
    }

    #[test]
    fn cached_surface_grid_preserves_moving_cave_collision_bits() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let data:Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/caves.json")).unwrap()).unwrap();
        let field=terrain::underground::Field::parse(&data).unwrap().unwrap();
        let frame=json!({"edits":[],"debris":[],"underground":field.snapshot()});
        let mut reused=Cache::default();
        for x in [0.,8.,16.,24.,48.] {
            let p=terrain::underground::world([x,-20.,-13.]);
            // A fresh reference starts with the same accepted radial phase,
            // but has no sampled-grid cache to reuse.
            let mut fresh=Cache::default();fresh.anchor=reused.anchor;
            fresh.prepare(p,&frame,&Value::Null).unwrap();
            reused.prepare(p,&frame,&Value::Null).unwrap();
            let a=reused.base.as_ref().unwrap();let b=fresh.base.as_ref().unwrap();
            assert_eq!(a.origin.map(f64::to_bits),b.origin.map(f64::to_bits));
            assert_eq!(a.triangles,b.triangles);
            assert_eq!(a.vertices.iter().map(|p|p.map(f32::to_bits)).collect::<Vec<_>>(),b.vertices.iter().map(|p|p.map(f32::to_bits)).collect::<Vec<_>>());
        }
    }

    #[test]
    fn startup_collision_snapshot_accepts_enabled_caves() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Procedures/caves.json");
        let caves: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let field = terrain::underground::Field::parse(&caves).unwrap().unwrap();
        let frame = json!({"edits":[], "debris":[], "underground":field.snapshot()});
        let mut cache = Cache::default();
        for position in [
            [0., terrain::RADIUS + 1.84, 0.],
            [12., terrain::RADIUS + 2., -8.],
            [20., terrain::RADIUS - 8., -13.],
        ] {
            let packet = cache
                .capture(position, &frame, &json!({"debris":[]}))
                .unwrap();
            assert!(
                !crate::water::types::Snapshot::parse(&packet)
                    .unwrap()
                    .triangles
                    .is_empty()
            );
        }
    }
    #[test]
    fn vertical_cave_movement_reuses_the_same_water_collision_mesh() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Procedures/caves.json");
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let cave = terrain::underground::Field::parse(&data).unwrap().unwrap();
        let frame = json!({"edits":[],"debris":[],"underground":cave.snapshot()});
        let mut cache = Cache::default();
        let first = cache
            .capture(
                [0., terrain::RADIUS + 2., 0.],
                &frame,
                &json!({"debris":[]}),
            )
            .unwrap();
        let second = cache
            .capture(
                [0., terrain::RADIUS - 20., 0.],
                &frame,
                &json!({"debris":[]}),
            )
            .unwrap();
        assert_eq!(first["revision"], second["revision"]);
        assert_eq!(first, second);
    }
}
impl Cache {
    pub fn capture_material(
        &mut self,
        position: V,
        frame: &Value,
        soil: &Value,
        material: &Value,
    ) -> Result<Value, String> {
        let vertices = material["debris"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| std::array::from_fn(|k| v[k].as_f64().unwrap_or(f64::NAN)))
            .collect();
        Ok(self
            .capture_numeric_material(position, frame, soil, vertices)?
            .packet())
    }
    pub fn capture_numeric_material(
        &mut self,
        position: V,
        frame: &Value,
        soil: &Value,
        material: Vec<[f64; 3]>,
    ) -> Result<crate::water::types::Snapshot, String> {
        if material.len() % 3 != 0 || material.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid material collision vertices".into());
        }
        self.prepare(position, frame, soil)?;
        let base = self.base.as_ref().ok_or("water collision base missing")?;
        if self.material_base_revision == base.revision && self.material_source == material {
            if let Some(snapshot) = &self.material_snapshot {
                return Ok(snapshot.clone());
            }
        }
        // Retain the accepted numeric mesh. Moving debris must not decode and
        // reallocate thousands of nested JSON arrays for unchanged terrain.
        let origin = base.origin;
        let mut vertices = base.vertices.clone();
        let mut triangles = base.triangles.clone();
        let mut unique:std::collections::HashMap<[u32;3],u32>=vertices.iter().enumerate().map(|(i,p)|(p.map(f32::to_bits),i as u32)).collect();
        for t in material.chunks_exact(3) {
            let p: [[f32; 3]; 3] =
                std::array::from_fn(|j| std::array::from_fn(|k| (t[j][k] - origin[k]) as f32));
            if p.iter()
                .any(|v| (0..3).any(|k| (v[k] as f64) < base.min[k] || (v[k] as f64) > base.max[k]))
            {
                continue;
            }
            let [a, b, c] = p.map(|p| p.map(f64::from));
            if len(cross(sub(b, a), sub(c, a))) < 1e-12 {
                continue;
            }
            let face=p.map(|p|*unique.entry(p.map(f32::to_bits)).or_insert_with(||{let i=vertices.len()as u32;vertices.push(p);i}));
            triangles.push(face);
        }
        if vertices.len() > 262144 {
            return Err("accepted material water contact budget exceeded".into());
        }
        self.revision += 1;
        let snapshot = crate::water::types::Snapshot {
            origin,
            vertices,
            triangles,
            revision: self.revision,
            min: base.min,
            max: base.max,
        };
        self.material_base_revision = base.revision;
        self.material_source = material;
        self.material_snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
    pub fn capture(&mut self, position: V, frame: &Value, soil: &Value) -> Result<Value, String> {
        self.prepare(position, frame, soil)?;
        let mut packet=self.base.as_ref().ok_or("water collision base missing")?.packet();
        packet["terrain_spacing_metres"]=json!(3.);packet["material_surface"]=json!("accepted");Ok(packet)
    }
    fn prepare(&mut self, position: V, frame: &Value, soil: &Value) -> Result<(), String> {
        if self.cave_profile!=frame["underground"] {
            let cave=terrain::underground::Field::parse(&frame["underground"])?;
            self.cave=cave.map(std::sync::Arc::new);self.cave_profile=frame["underground"].clone();
        }
        let cave=self.cave.clone();
        let affected=cave.as_ref().is_some_and(|field|field.affected(position,32.));
        // A collision patch covers 192 m. Reuse its accepted sampling phase
        // inside a 32 m guard region, independent of the visual 2 m anchor.
        // Cave mesh-window changes and every accepted world edit remain part
        // of the key below, so this never suppresses cave/edit invalidation.
        let normal=self.anchor.filter(|&n|len(sub(mul(unit(position),terrain::RADIUS),mul(n,terrain::RADIUS)))<32.)
            .unwrap_or_else(||terrain::mesh_anchor(position));
        let origin = mul(normal, terrain::RADIUS);
        let local = Frame::radial(origin, [0., 0., -1.]);
        let cave_window = terrain::underground::window(position);
        let key = serde_json::to_vec(&json!([
            normal,
            frame["edits"],
            frame["debris"],
            soil["debris"],
            frame["underground"],
            if affected {[cave_window[0],cave_window[2]]}else{[0;2]},
            affected
        ]))
        .map_err(|e| e.to_string())?;
        if self.key == key {
            return Ok(());
        }
        // The 192m terrain grid depends only on its accepted radial anchor.
        // Moving a cave window still rebuilds the cave/edit result, but does
        // not resample all 4225 unchanged surface corners.
        let grid_key=normal.map(f64::to_bits);
        if self.terrain_grid.as_ref().is_none_or(|(key,_)|*key!=grid_key){
        let mut points = Vec::new();
        const SIDE: usize = 64;
        const STEP: f64 = 3.;
        for z in 0..=SIDE {
            for x in 0..=SIDE {
                let normal =
                    unit(local.world([(x as f64 - 32.) * STEP, 0., (z as f64 - 32.) * STEP]));
                points.push(mul(normal, terrain::RADIUS + terrain::height(normal)));
            }
        }
        let mut base = Vec::new();
        for z in 0..SIDE {
            for x in 0..SIDE {
                let a = z * (SIDE + 1) + x;
                for face in [[a, a + 1, a + SIDE + 2], [a, a + SIDE + 2, a + SIDE + 1]] {
                    base.push(face.map(|i| [points[i][0], points[i][1], points[i][2], 0., 0., 0.]));
                }
            }
        }
        self.terrain_grid=Some((grid_key,base));
        }
        let base=self.terrain_grid.as_ref().unwrap().1.clone();
        let edits = frame["edits"]
            .as_array()
            .ok_or("accepted terrain edits required")?;
        let mut base = terrain::tool_cut::mesh(base, edits);
        if let Some(field) = cave {
            base = field.replace_patch(base, position, edits)?;
        }
        base = terrain::material_domain::clip(base, &terrain::material_domain::from_edits(edits));
        base.extend(terrain::material_domain::shell(
            edits,
            terrain::underground::Field::parse(&frame["underground"])?.as_ref(),
        ));
        let mut vertices = Vec::<[f32; 3]>::new();
        let mut triangles = Vec::<[u32; 3]>::new();
        for triangle in base {
            let local = triangle.map(|p| std::array::from_fn(|i| (p[i] - origin[i]) as f32));
            let [a, b, c] = local.map(|p: [f32; 3]| p.map(f64::from));
            // Cave clipping can produce microscopic slivers that collapse when
            // converted to the collision packet's local f32 coordinates.
            if len(cross(sub(b, a), sub(c, a))) < 1e-12 {
                continue;
            }
            let first = vertices.len() as u32;
            vertices.extend(local);
            triangles.push([first, first + 1, first + 2]);
        }
        for data in [&frame["debris"], &soil["debris"]] {
            if let Some(rows) = data.as_array() {
                if rows.len() % 3 != 0 {
                    return Err("accepted material triangles required".into());
                }
                for face in rows.chunks_exact(3) {
                    let first = vertices.len() as u32;
                    for row in face {
                        let p: V = std::array::from_fn(|i| row[i].as_f64().unwrap_or(f64::NAN));
                        if !finite(p) {
                            return Err("nonfinite accepted material vertex".into());
                        }
                        vertices.push(std::array::from_fn(|i| (p[i] - origin[i]) as f32));
                    }
                    triangles.push([first, first + 1, first + 2]);
                }
            }
        }
        if vertices.len() > 262144 {
            return Err("accepted water collision budget exceeded".into());
        }
        let mut min: V = std::array::from_fn(|i| {
            vertices
                .iter()
                .map(|p| p[i] as f64)
                .fold(f64::INFINITY, f64::min)
                - 0.01
        });
        let mut max: V = std::array::from_fn(|i| {
            vertices
                .iter()
                .map(|p| p[i] as f64)
                .fold(f64::NEG_INFINITY, f64::max)
                + 0.01
        });
        // Vertical query extent allows emitted parcels above/below the terrain;
        // actual collision always requires an accepted triangle hit.
        let vertical = (0..3)
            .max_by(|a, b| normal[*a].abs().total_cmp(&normal[*b].abs()))
            .unwrap();
        min[vertical] = min[vertical].min(-256.);
        max[vertical] = max[vertical].max(256.);
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("water collision revision exhausted")?;
        // Shared triangle corners need one vertex. Preserve winding and every
        // accepted face; this is indexing, not mesh simplification.
        let mut unique = std::collections::HashMap::<[u32; 3], u32>::new();
        let mut packed = Vec::new();
        let remap: Vec<_> = vertices
            .iter()
            .map(|p| {
                *unique.entry(p.map(f32::to_bits)).or_insert_with(|| {
                    let id = packed.len() as u32;
                    packed.push(*p);
                    id
                })
            })
            .collect();
        for face in &mut triangles {
            *face = face.map(|i| remap[i as usize]);
        }
        vertices = packed;
        // Numeric geometry was just constructed and validated above. Avoid a
        // serialize/parse round trip over every collision triangle.
        let base = crate::water::types::Snapshot {
            origin, vertices, triangles, revision: self.revision, min, max,
        };
        self.key = key;
        self.anchor=Some(normal);
        self.base = Some(base);
        Ok(())
    }
}

#[cfg(test)]mod coverage_reuse_tests {
 use super::*;
 #[test]fn unchanged_collision_patch_is_reused_inside_coverage_and_edits_still_invalidate(){
  let mut cache=Cache::default();let eye=[0.,terrain::RADIUS+2.,0.];
  let mut frame=json!({"edits":[],"debris":[],"underground":null});
  let first=cache.capture_numeric_material(eye,&frame,&Value::Null,vec![]).unwrap();
  let moved=cache.capture_numeric_material([eye[0]+6.,eye[1],eye[2]],&frame,&Value::Null,vec![]).unwrap();
  assert_eq!(first.revision,moved.revision);assert_eq!(first.vertices,moved.vertices);assert!(moved.contains([eye[0]+6.,eye[1],eye[2]],0.));
  frame["debris"]=json!([[0.,terrain::RADIUS+1.,0.,1.,1.,1.],[0.1,terrain::RADIUS+1.,0.,1.,1.,1.],[0.,terrain::RADIUS+1.,0.1,1.,1.,1.]]);
  let changed=cache.capture_numeric_material(eye,&frame,&Value::Null,vec![]).unwrap();assert!(changed.revision>moved.revision);
  let outside=cache.capture_numeric_material([eye[0]+48.,eye[1],eye[2]],&frame,&Value::Null,vec![]).unwrap();assert_ne!(outside.origin,changed.origin);
 }
}

#[cfg(test)]mod radial_guard_tests {
 use super::*;
 #[test]fn collision_surface_guard_is_horizontal_at_flight_and_cavern_altitudes(){
  let frame=json!({"edits":[],"debris":[],"underground":null});
  for altitude in [-70.,85.] {let mut cache=Cache::default();let a=cache.capture_numeric_material([0.,terrain::RADIUS+altitude,0.],&frame,&Value::Null,vec![]).unwrap();let b=cache.capture_numeric_material([20.,terrain::RADIUS+altitude,0.],&frame,&Value::Null,vec![]).unwrap();assert_eq!(a.origin,b.origin);assert_eq!(a.revision,b.revision);assert_eq!(a.vertices,b.vertices);let c=cache.capture_numeric_material([48.,terrain::RADIUS+altitude,0.],&frame,&Value::Null,vec![]).unwrap();assert_ne!(c.origin,a.origin);}
 }
}
