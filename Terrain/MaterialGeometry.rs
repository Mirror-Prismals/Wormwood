//! Immutable continuous terrain occupancy for bounded off-callback queries.
//! Material thickness is measured through the accepted height field; triangle
//! bounds are never promoted into invented solid volumes.
use crate::audio::{
    geometry::{Geometry, Hit, Snapshot},
    *,
};
use serde_json::Value;

pub(crate) struct Surface {
    revision: u64,
    edits: Vec<Value>,
    surfaces: Snapshot,
    volumes: volumes::Volumes,
    underground: Option<super::underground::Field>,
    venue: Option<Snapshot>,
}
impl Surface {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        let edits = value["terrain"]["edits"]
            .as_array()
            .ok_or("accepted terrain edits required")?;
        if !super::tool_cut::valid_edits(&value["terrain"]["edits"]) {
            return Err("invalid accepted terrain edits".into());
        }
        // Volumetric additions are accepted material cells, never broadphase boxes.
        if value["solids"]
            .as_array()
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err("terrain adapter does not accept broadphase solid bounds".into());
        }
        Ok(Self {
            revision: value["revision"]
                .as_u64()
                .ok_or("geometry revision required")?,
            edits: edits.clone(),
            surfaces: Snapshot::parse(value)?,
            volumes: volumes::Volumes::parse(value)?,
            underground: super::underground::Field::parse(&value["underground"])?,
            venue: if value["venue"].is_null(){None}else{Some(Snapshot::parse(&value["venue"])?)},
        })
    }
    fn signed_distance(&self, point: V) -> f64 {
        let radius = length(point);
        if radius < 1. {
            return -super::RADIUS;
        }
        let normal = mul(point, 1. / radius);
        let height = super::tool_cut::height(normal, super::height(normal), &self.edits, true);
        // Caves and excavation only remove land. This point is already in
        // open air; the acoustic crossing search needs only its sign here.
        let clearance = radius - super::RADIUS - height;
        if clearance > 0.05 { return clearance; }
        self.underground
            .as_ref()
            .map_or(radius - super::RADIUS - height, |field| {
                field.distance(point, height)
            })
    }
    fn crossing(&self, start: V, direction: V, mut a: f64, mut b: f64, inside: bool) -> f64 {
        for _ in 0..24 {
            let midpoint = (a + b) * 0.5;
            if (self.signed_distance(add(start, mul(direction, midpoint))) <= 0.) == inside {
                a = midpoint;
            } else {
                b = midpoint;
            }
        }
        (a + b) * 0.5
    }
    /// Return measured solid intervals along the finite segment. Crossing
    /// search uses 5 cm samples and 24 bisections; sub-sample tangencies remain
    /// a documented resolution limit, not a claim of exact analytic roots.
    pub(crate) fn intervals(&self, start: V, end: V) -> Result<Vec<[f64; 2]>, String> {
        let delta = sub(end, start);
        let distance = length(delta);
        if !distance.is_finite()
            || distance > 512.
            || [start, end].iter().flatten().any(|n| !n.is_finite())
        {
            return Err("material query requires a finite segment of at most 512 metres".into());
        }
        if distance < 1e-9 {
            return Ok(Vec::new());
        }
        let direction = mul(delta, 1. / distance);
        let steps = (distance / 0.05).ceil().max(1.) as usize;
        let mut inside = self.signed_distance(start) <= 0.;
        let mut begin = 0.;
        let mut previous = 0.;
        let mut intervals = Vec::new();
        for i in 1..=steps {
            let along = distance * i as f64 / steps as f64;
            let next = self.signed_distance(add(start, mul(direction, along))) <= 0.;
            if inside != next {
                let boundary = self.crossing(start, direction, previous, along, inside);
                if inside {
                    intervals.push([begin, boundary]);
                } else {
                    begin = boundary;
                }
                inside = next;
            }
            previous = along;
        }
        if inside {
            intervals.push([begin, distance]);
        }
        intervals.extend(self.volumes.intervals(start, direction, distance));
        if let Some(venue)=&self.venue {intervals.extend(venue.solid_intervals(start,end));}
        Ok(volumes::union(intervals))
    }
    fn terrain_trace(&self, start: V, direction: V, distance: f64) -> Option<Hit> {
        self.terrain_trace_bounded(start, direction, distance, distance)
    }
    fn terrain_trace_bounded(&self, start: V, direction: V, distance: f64, nearest: f64) -> Option<Hit> {
        if !distance.is_finite() || !(0.0..=512.).contains(&distance) {
            return None;
        }
        let direction = unit(direction);
        if length(direction) < 0.5 {
            return None;
        }
        let steps = (distance / 0.05).ceil().max(1.) as usize;
        let mut inside = self.signed_distance(start) <= 0.;
        let mut previous = 0.;
        for i in 1..=steps {
            // Use the ORIGINAL sample grid and bisection bracket. Shortening
            // distance would shift samples and could miss narrow crossings.
            // Evaluate one point beyond the nearer hit to close that bracket.
            if previous > nearest { break; }
            let along = distance * i as f64 / steps as f64;
            let next = self.signed_distance(add(start, mul(direction, along))) <= 0.;
            if inside != next {
                let distance = self.crossing(start, direction, previous, along, inside);
                let position = add(start, mul(direction, distance));
                let gradient = std::array::from_fn(|axis| {
                    let mut a = position;
                    let mut b = position;
                    a[axis] += 0.01;
                    b[axis] -= 0.01;
                    self.signed_distance(a) - self.signed_distance(b)
                });
                return Some(Hit {
                    position,
                    normal: unit(gradient),
                    distance,
                });
            }
            inside = next;
            previous = along;
        }
        None
    }
}
impl Geometry for Surface {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn trace(&self, start: V, direction: V, distance: f64) -> Option<Hit> {
        let original_direction = direction;
        let direction = unit(direction);
        let surface = self.surfaces.trace(start, direction, distance);
        let material = self.volumes.trace(start, direction, distance);
        let surface = match (surface, material) {
            (Some(a), Some(b)) => Some(if a.distance < b.distance { a } else { b }),
            (a, b) => a.or(b),
        };
        let venue=self.venue.as_ref().and_then(|v|v.trace(start,direction,distance));
        let nearest = [surface, venue].into_iter().flatten()
            .map(|hit| hit.distance).fold(distance, f64::min);
        let terrain = self.terrain_trace_bounded(start, original_direction, distance, nearest);
        [terrain, surface, venue]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }
    fn wall_distance(&self, start: V, end: V, max_distance: f64) -> f64 {
        let delta = sub(end, start);
        let distance = length(delta);
        let end = if distance > max_distance {
            add(start, mul(unit(delta), max_distance))
        } else {
            end
        };
        self.intervals(start, end)
            .map(|rows| rows.iter().map(|r| r[1] - r[0]).sum())
            .unwrap_or(0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn surface(edits: Value) -> Surface {
        Surface::parse(&json!({"revision":1,"origin":[0.,super::super::RADIUS,0.],"terrain":{"edits":edits},"triangles":[],"solids":[]})).unwrap()
    }
    #[test]
    fn continuous_terrain_supplies_hit_and_measured_solid_interval() {
        let s = surface(json!([]));
        let r = super::super::RADIUS;
        let hit = s.trace([0., r + 2., 0.], [0., -1., 0.], 4.).unwrap();
        assert!((hit.distance - 2.).abs() < 1e-6);
        assert!(hit.normal[1] > 0.99);
        assert!((s.wall_distance([0., r + 2., 0.], [0., r - 2., 0.], 4.) - 2.).abs() < 1e-6);
    }
    #[test]
    fn accepted_excavation_changes_hit_and_thickness_without_a_box() {
        let s = surface(json!([{"normal":[0.,1.,0.],"radius":2.,"depth":0.8}]));
        let r = super::super::RADIUS;
        let hit = s.trace([0., r + 2., 0.], [0., -1., 0.], 4.).unwrap();
        assert!((hit.distance - 2.8).abs() < 1e-6);
        assert!((s.wall_distance([0., r + 2., 0.], [0., r - 2., 0.], 4.) - 1.2).abs() < 1e-6);
    }
    #[test]
    fn bounded_terrain_trace_matches_full_grid_near_venue_and_excavation() {
        let venue = crate::theatre::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let r = super::super::RADIUS;
        let origin = [0.,r+0.5,0.];
        for edits in [json!([]),json!([{"normal":[0.,1.,0.],"radius":2.,"depth":0.8}])] {
            let s = Surface::parse(&json!({"revision":1,"origin":origin,"terrain":{"edits":edits},"triangles":[],"solids":[],"venue":venue.geometry_snapshot(origin)})).unwrap();
            for start in [[0.,r+2.,0.],[0.,r+8.,-52.],[0.,r+30.,-90.],[90.,r+3.,0.]] {
                for i in 0..40 {
                    let z = 1.-2.*(i as f64+0.5)/40.;
                    let angle = i as f64*2.39996323;
                    let d = [(1.-z*z).sqrt()*angle.cos(),z,(1.-z*z).sqrt()*angle.sin()];
                    for distance in [3.013,128.] {
                        let terrain=s.terrain_trace(start,d,distance);
                        let surfaces=s.surfaces.trace(start,unit(d),distance);
                        let material=s.volumes.trace(start,unit(d),distance);
                        let surface=match(surfaces,material){(Some(a),Some(b))=>Some(if a.distance<b.distance{a}else{b}),(a,b)=>a.or(b)};
                        let venue=s.venue.as_ref().and_then(|v|v.trace(start,unit(d),distance));
                        let reference=[terrain,surface,venue].into_iter().flatten().min_by(|a,b|a.distance.total_cmp(&b.distance));
                        let bits=|h:Option<Hit>|h.map(|h|(h.distance.to_bits(),h.position.map(f64::to_bits),h.normal.map(f64::to_bits)));
                        assert_eq!(bits(s.trace(start,d,distance)),bits(reference),"{start:?} {d:?} {distance}");
                    }
                }
            }
        }
        // The last sample bracket still crosses the nearest-hit bound.
        let s=surface(json!([]));
        for nearest in [0.,1.99999,2.,2.00001,4.] {
            let h=s.terrain_trace_bounded([0.,r+2.,0.],[0.,-1.,0.],4.013,nearest);
            if nearest>=2. {assert!((h.unwrap().distance-2.).abs()<1e-6);}
        }
    }

}
