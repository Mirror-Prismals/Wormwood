//! Explicit continuous water surfaces and volumetric source ownership.
//! Terrain geometry remains owned by PlanetSurface/TerrainTool Systems.
use super::{
    flow::{Kind, WetPoint},
    hydrology::State as Terrain,
    math::*,
    ocean,
    types::{number, vector},
};
use serde_json::Value;
#[derive(Clone, Debug)]
pub(crate) struct River {
    pub id: u64,
    pub points: Vec<V>,
    pub width: f64,
}
#[derive(Clone, Debug)]
pub(crate) struct Source {
    pub id: u64,
    pub frame: Frame,
    pub min: V,
    pub max: V,
    pub target: usize,
    pub velocity: V,
}
#[derive(Clone, Debug)]
pub(crate) struct Pool { pub min:V, pub max:V }
#[derive(Default, Clone)]
pub(crate) struct State {
    pub ocean: bool,
    pub pools:Vec<Pool>,
    pub sheets:Vec<[V;3]>,
    pub generated: bool,
    pub rivers: Vec<River>,
    pub sources: Vec<Source>,
    pub revision: u64,
}
impl State {
    pub fn parse(v: &Value) -> Result<Self, String> {
        let mut out = Self {
            ocean: v["ocean"]
                .as_bool()
                .ok_or("bodies.ocean boolean required")?,
            generated: v["generated"].as_bool().unwrap_or(false),
            ..Self::default()
        };
        let rows = v["rivers"]
            .as_array()
            .filter(|a| a.len() <= 128)
            .ok_or("bounded rivers array required")?;
        for row in rows {
            let points = row["points"]
                .as_array()
                .filter(|a| (2..=1024).contains(&a.len()))
                .ok_or("river course requires 2..1024 points")?
                .iter()
                .map(vector)
                .collect::<Result<Vec<_>, _>>()?;
            let id = row["id"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("river id required")?;
            if out.rivers.iter().any(|r| r.id == id)
                || points.windows(2).any(|p| len(sub(p[1], p[0])) < 1e-5)
            {
                return Err("invalid or duplicate river course".into());
            }
            out.rivers.push(River {
                id,
                points,
                width: number(row, "width", 4., 0.1, 1000.)?,
            });
        }
        let rows = v["sources"]
            .as_array()
            .filter(|a| a.len() <= 128)
            .ok_or("bounded source array required")?;
        for row in rows {
            let id = row["id"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("source id required")?;
            let frame = super::types::frame(&row["frame"])?;
            let min = vector(&row["min"])?;
            let max = vector(&row["max"])?;
            if out.sources.iter().any(|s| s.id == id)
                || (0..3).any(|i| max[i] - min[i] < 0.96 || max[i] - min[i] > 1000.)
            {
                return Err("invalid continuous source volume".into());
            }
            out.sources.push(Source {
                id,
                frame,
                min,
                max,
                target: number(row, "target", 32., 0., 1000.)? as usize,
                velocity: vector(&row["velocity"])?,
            });
        }
        if !v["sheets"].is_null() {
            out.sheets=serde_json::from_value(v["sheets"].clone()).map_err(|e|format!("invalid authored water sheets: {e}"))?;
            if out.sheets.len()>8192 || out.sheets.iter().flatten().flatten().any(|v|!v.is_finite()) {return Err("authored water sheet budget exceeded".into());}
        }
        if let Some(rows)=v["pools"].as_array() {
            if rows.len()>16{return Err("pool count exceeds budget".into());}
            for row in rows {let min=vector(&row["min"])?;let max=vector(&row["max"])?;
                if (0..3).any(|i|max[i]<=min[i] || max[i]-min[i]>200.) {return Err("invalid pool bounds".into());}
                out.pools.push(Pool{min,max});
            }
        } else if !v["pools"].is_null(){return Err("pools must be an array".into());}
        Ok(out)
    }
    pub fn query(
        &self,
        point: V,
        mean: f64,
        wave: &ocean::State,
        terrain: &Terrain,
    ) -> Option<WetPoint> {
        self.surface(point, mean, wave, terrain)
            .filter(|w| w.point[1] >= w.bed_height && w.point[1] <= w.surface_height)
    }
    pub fn surface(
        &self,
        point: V,
        mean: f64,
        wave: &ocean::State,
        terrain: &Terrain,
    ) -> Option<WetPoint> {
        self.surface_candidate(point, mean, wave, terrain)
            .filter(|w| {
                w.bed_height < w.surface_height && (w.kind != Kind::River || w.shore_distance >= 0.)
            })
    }
    pub fn surface_candidate(
        &self,
        point: V,
        mean: f64,
        wave: &ocean::State,
        terrain: &Terrain,
    ) -> Option<WetPoint> {
        for pool in &self.pools {
            if (point[1]<0.) != (pool.min[1]<0.) {continue;}
            if point[0]>=pool.min[0] && point[0]<=pool.max[0] && point[2]>=pool.min[2] && point[2]<=pool.max[2] {
                let sign=if pool.min[1]<0. {-1.}else{1.};
                let frame=Frame::radial([0.,sign*crate::terrain::RADIUS,0.],[0.,0.,-1.]);
                let surface=if sign<0. {-pool.min[1]}else{pool.max[1]}-crate::terrain::RADIUS;
                let bed=if sign<0. {-pool.max[1]}else{pool.min[1]}-crate::terrain::RADIUS;
                let shore=(point[0]-pool.min[0]).min(pool.max[0]-point[0]).min(point[2]-pool.min[2]).min(pool.max[2]-point[2]);
                return Some(WetPoint{frame,point:frame.local(point),uv:[point[0],point[2]],surface_height:surface,mean_height:surface,bed_height:bed,shore_distance:shore,shore_gradient:[0.;2],wave_amplitude:0.,shore_direction:[0.;2],downstream_direction:[0.;2],kind:Kind::Lake});
            }
        }
        self.natural_candidate(point,mean,wave,terrain)
    }
    pub fn natural_candidate(&self,point:V,mean:f64,wave:&ocean::State,terrain:&Terrain)->Option<WetPoint> {
        if crate::terrain::nightworld::active(point){return None;}
        if len(point) - crate::terrain::RADIUS < -18. {
            return self.depths_surface(point);
        }
        let radial = unit(point);
        let phase = super::surface_mesh::chart().local(point);
        let frame = Frame::radial(mul(radial, crate::terrain::RADIUS), [0., 0., -1.]);
        // Use the actual first bed below the free surface. Unknown finite
        // coverage produces no claim of water occupancy.
        let bed_at = |height: f64| {
            terrain
                .ray(
                    mul(radial, crate::terrain::RADIUS + height + 4.),
                    mul(radial, -1.),
                    128.,
                )
                .map(|h| len(h.point) - crate::terrain::RADIUS)
        };
        for river in &self.rivers {
            let mut closest = None;
            let mut distance = f64::INFINITY;
            for pair in river.points.windows(2) {
                let d = sub(pair[1], pair[0]);
                let t = (dot(sub(point, pair[0]), d) / dot(d, d)).clamp(0., 1.);
                let p = add(pair[0], mul(d, t));
                let horizontal = sub(sub(point, p), mul(radial, dot(sub(point, p), radial)));
                let next = len(horizontal);
                if next < distance {
                    distance = next;
                    closest = Some((p, unit(d)));
                }
            }
            if distance > river.width * 0.5 + 2. {
                continue;
            }
            let (surface, direction) = closest?;
            let height = len(surface) - crate::terrain::RADIUS;
            let bed = bed_at(height)?;
            let local = frame.local(point);

            return Some(WetPoint {
                frame,
                point: local,
                uv: [phase[0], phase[2]],
                surface_height: height,
                mean_height: height,
                bed_height: bed,
                shore_distance: river.width * 0.5 - distance,
                shore_gradient: [0.; 2],
                wave_amplitude: 0.,
                shore_direction: [0., 0.],
                downstream_direction: [dot(direction, frame.right), dot(direction, frame.forward)],
                kind: Kind::River,
            });
        }
        if self.generated {
            if let Some(surface) = crate::terrain::drainage::water_candidate(radial) {
                let height = surface.level;
                let connected = self.ocean
                    && crate::terrain::drainage::ocean_connected(radial, mean) == Some(true);
                if !connected || height > mean + 0.01 {
                    let bed = bed_at(height)?;
                    let world = [surface.direction[0], 0., surface.direction[1]];
                    let world = unit(sub(world, mul(radial, dot(world, radial))));
                    return Some(WetPoint {
                        frame,
                        point: frame.local(point),
                        uv: [phase[0], phase[2]],
                        surface_height: height,
                        mean_height: height,
                        bed_height: bed,
                        shore_distance: surface.shore_distance,
                        shore_gradient: [0.; 2],
                        wave_amplitude: 0.,
                        shore_direction: [0.; 2],
                        downstream_direction: [dot(world, frame.right), dot(world, frame.forward)],
                        kind: match surface.kind {
                            crate::terrain::drainage::Kind::Lake => Kind::Lake,
                            crate::terrain::drainage::Kind::River => Kind::River,
                        },
                    });
                }
            }
            if crate::terrain::drainage::ocean_connected(radial, mean) != Some(true) {
                return None;
            }
        }
        if !self.ocean {
            return None;
        }
        let bed = bed_at(mean)?;
        let local = frame.local(point);

        let gradient = [
            dot(
                terrain
                    .ray(
                        mul(radial, crate::terrain::RADIUS + mean + 4.),
                        mul(radial, -1.),
                        128.,
                    )?
                    .normal,
                frame.right,
            ),
            dot(
                terrain
                    .ray(
                        mul(radial, crate::terrain::RADIUS + mean + 4.),
                        mul(radial, -1.),
                        128.,
                    )?
                    .normal,
                frame.forward,
            ),
        ];
        let regional=crate::terrain::regions::ocean(radial);
        let shore = if regional {crate::terrain::regions::sea_shore(radial).1} else {unit2(gradient.map(|n| -n))};
        let slope = dot2(gradient, gradient).sqrt().max(0.015);
        let distance = if regional {crate::terrain::regions::ocean_distance(radial)} else {(mean - bed) / slope};
        let chart = Frame::radial([0., crate::terrain::RADIUS, 0.], [0., 0., -1.]);
        let phase = chart.local(point);
        let surface = wave.surface([phase[0], phase[2]], mean, bed, distance);
        Some(WetPoint {
            frame,
            point: local,
            uv: [phase[0], phase[2]],
            surface_height: surface,
            mean_height: mean,
            bed_height: bed,
            shore_distance: distance,
            shore_gradient: shore.map(|n| -n),
            wave_amplitude: wave.settings.amplitude,
            shore_direction: shore,
            downstream_direction: [0., 0.],
            kind: Kind::Ocean,
        })
    }
    pub(crate) fn depths_surface(&self, point: V) -> Option<WetPoint> {
        self.depths_surface_sample(point,false)
    }
    pub(crate) fn depths_surface_boundary(&self,point:V)->Option<WetPoint> {
        self.depths_surface_sample(point,true)
    }
    fn depths_surface_sample(&self,point:V,include_dry:bool)->Option<WetPoint> {
        if crate::terrain::nightworld::active(point){return None;}
        let radial=unit(point);
        let local=crate::terrain::underground::local(point);
        let river=if include_dry {crate::terrain::depths::river_boundary(local[0],local[2])}else{crate::terrain::depths::river(local[0],local[2])}?;
        let frame=Frame::radial(mul(radial,crate::terrain::RADIUS),[0.,0.,-1.]);
        let phase=super::surface_mesh::chart().local(point);
        Some(WetPoint {
            frame, point:frame.local(point), uv:[phase[0],phase[2]],
            surface_height:river.water, mean_height:river.water,
            bed_height:river.bottom, shore_distance:river.shore,
            shore_gradient:[0.;2], wave_amplitude:0., shore_direction:[0.;2],
            downstream_direction:[0.;2], kind:Kind::River,
        })
    }
}

#[cfg(test)] mod nightworld_tests {
 use super::*;
 #[test] fn nightworld_landing_pool_has_inverted_surface_and_bed(){
  let c=crate::terrain::nightworld::shaft();let r=crate::terrain::RADIUS;
  let mut state=State::default();state.pools.push(Pool{min:[c[0]-4.7,-(r-94.6),c[2]-4.7],max:[c[0]+4.7,-(r-95.6),c[2]+4.7]});
  let terrain=Terrain::default();let wave=ocean::State::default();
  let p=crate::terrain::nightworld::world([c[0],-95.,c[2]]);
  assert!(state.depths_surface(p).is_none(),"overworld underground rivers must not appear in Nightworld");
  let wet=state.surface_candidate(p,0.,&wave,&terrain).unwrap();
  assert!((wet.surface_height+94.6).abs()<1e-7);assert!((wet.bed_height+95.6).abs()<1e-7);assert!(wet.frame.up[1]< -0.99);
  let north=[p[0],-p[1],p[2]];let actual=state.surface_candidate(north,0.,&wave,&terrain);let natural=State::default().surface_candidate(north,0.,&wave,&terrain);assert_eq!(actual.as_ref().map(|w|w.surface_height),natural.as_ref().map(|w|w.surface_height));
 }
}
