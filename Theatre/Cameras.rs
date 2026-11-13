//! Security camera selection and pan behavior ported from Cardinal.
//! Outdoor locations use the shared Mirror terrain and drainage samplers.
use crate::audio::{add, mul};
use serde_json::{Value, json};
#[derive(Clone, Copy, Debug)]
pub(crate) struct Camera {
    pub id: &'static str,
    pub position: [f64; 3],
    pub yaw: f64,
}
// Outdoor poses are resolved once against Mirror's shared continuous landscape.
pub(crate) const CAMERA_IDS: [&str; 3] = ["riverbank", "mountain", "theatre"];
pub(crate) fn sites() -> &'static [Camera; 2] {
    static SITES: std::sync::OnceLock<[Camera; 2]> = std::sync::OnceLock::new();
    SITES.get_or_init(|| {
        use crate::terrain::{self, drainage};
        let normal = |x: f64, z: f64| {
            crate::audio::unit([
                x,
                (terrain::RADIUS * terrain::RADIUS - x * x - z * z).sqrt(),
                z,
            ])
        };
        let mut mountain = (f64::NEG_INFINITY, 0., 0.);
        for z in -64..=64 {
            for x in -64..=64 {
                let (x, z) = (x as f64 * 64., z as f64 * 64.);
                let n = normal(x, z);
                let h = terrain::height(n);
                if h > mountain.0 {
                    mountain = (h, x, z);
                }
            }
        }
        let ([bx, bz], river_yaw) = drainage::camera_bank();
        let pose = |id, x: f64, z: f64, yaw| {
            let n = normal(x, z);
            Camera {
                id,
                position: crate::audio::mul(n, terrain::RADIUS + terrain::height(n) + 3.),
                yaw,
            }
        };
        [
            pose("riverbank", bx, bz, river_yaw),
            pose("mountain", mountain.1, mountain.2, 225.),
        ]
    })
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Page {
    camera: usize,
    yaw: f64,
    pitch: f64,
}
#[derive(Default)]
pub(crate) struct State {
    pages: std::collections::BTreeMap<String, Page>,
    cycle_down: bool,
    pub(crate) retained_key:Value,
    pub(crate) retained_view:Value,
}
impl State {
    pub fn update(
        &mut self,
        page: &str,
        origin: [f64; 3],
        _anchor: [f64; 3],
        input: &Value,
        dt: f64,
        blocked: bool,
        aspect: f64,
    ) -> Result<Value, String> {
        if !["timeline", "mixer", "piano_roll"].contains(&page)
            || !dt.is_finite()
            || !(0. ..=1.).contains(&dt)
            || !aspect.is_finite()
            || !(0.05..=20.).contains(&aspect)
        {
            return Err("invalid security camera view request".into());
        }
        let cycle = input["cycle"] == true;
        let next = cycle && !self.cycle_down && !blocked;
        self.cycle_down = cycle;
        let selected = self.pages.entry(page.into()).or_insert_with(|| {
            let camera = if page=="piano_roll" {1}else{0};
            Page {
                camera,
                yaw: sites()[camera].yaw,
                pitch: -8.,
            }
        });
        if input["theatre"]==true {
            selected.camera=2;selected.yaw=90.;selected.pitch=-8.;
        } else if next {
            selected.camera = (selected.camera + 1) % CAMERA_IDS.len();
            selected.yaw = if selected.camera == 2 {90.} else {sites()[selected.camera].yaw};
            selected.pitch = -8.;
        }
        if !blocked {
            let held = |i| if input["pan"][i] == true { 1. } else { 0. };
            selected.yaw = (selected.yaw + (held(1) - held(0)) * 84. * dt).rem_euclid(360.);
            selected.pitch = (selected.pitch + (held(2) - held(3)) * 66. * dt).clamp(-70., 70.);
        }
        let (yaw, pitch) = (selected.yaw.to_radians(), selected.pitch.to_radians());
        let forward = [
            yaw.cos() * pitch.cos(),
            pitch.sin(),
            yaw.sin() * pitch.cos(),
        ];
        let position = add(
            if selected.camera == 2 {add(origin, [0., 10., -44.])} else {sites()[selected.camera].position},
            add([0., 0.18, 0.], mul(forward, 0.58)),
        );
        Ok(
            json!({"id":if selected.camera==2 {"theatre"}else{sites()[selected.camera].id},"index":selected.camera,"page":page,"position":position,"yaw":yaw+std::f64::consts::FRAC_PI_2,"pitch":pitch,"forward":forward,"aspect":aspect,"near":0.05,"far":1000.,"two_sided":true,"outdoor":selected.camera != 2}),
        )
    }
    pub fn cancel_keys(&mut self) {
        self.cycle_down = false;
        self.retained_key=Value::Null;self.retained_view=Value::Null;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_pan_rates_limits_and_cycle_edges() {
        let mut s = State::default();
        let idle = json!({});
        let a = s
            .update("timeline", [0.; 3], [0., 8., -47.], &idle, 0., false, 1.)
            .unwrap();
        let turn = json!({"cycle":true,"pan":[false,true,true,false]});
        let b = s
            .update("timeline", [0.; 3], [0.; 3], &turn, 1., false, 1.)
            .unwrap();
        assert_ne!(a["id"], b["id"]);
        let c = s
            .update("timeline", [0.; 3], [0.; 3], &turn, 1., false, 1.)
            .unwrap();
        assert_eq!(b["id"], c["id"]);
        assert!((c["pitch"].as_f64().unwrap().to_degrees() - 70.).abs() < 1e-9);
    }
    #[test]
    fn page_views_restore_and_keyboard_owner_blocks_pan() {
        let mut s = State::default();
        let p = s
            .update(
                "timeline",
                [0.; 3],
                [0.; 3],
                &json!({"pan":[false,true,false,false]}),
                0.5,
                false,
                1.,
            )
            .unwrap();
        s.update(
            "piano_roll",
            [0.; 3],
            [0.; 3],
            &json!({"cycle":true}),
            0.,
            false,
            1.,
        )
        .unwrap();
        let restored = s
            .update(
                "timeline",
                [0.; 3],
                [0.; 3],
                &json!({"pan":[true,false,true,false]}),
                1.,
                true,
                1.,
            )
            .unwrap();
        assert_eq!(p, restored);
    }
    #[test]
    fn outdoor_lens_uses_world_terrain_coordinates() {
        let mut s = State::default();
        let p = s
            .update(
                "timeline",
                [0., 100., 0.],
                sites()[0].position,
                &Value::Null,
                0.,
                false,
                1.,
            )
            .unwrap();
        assert_eq!(p["id"], "riverbank");
        assert!(p["position"][1].as_f64().unwrap() > crate::terrain::RADIUS - 1000.);
        assert!(
            s.update("invalid", [0.; 3], [0.; 3], &Value::Null, 0., false, 1.)
                .is_err()
        );
    }
}

#[cfg(test)]
mod outdoor_tests {
    use super::*;
    #[test]
    fn outdoor_sites_are_distinct_and_above_shared_ground() {
        let sites = sites();
        assert_ne!(sites[0].position, sites[1].position);
        for c in sites {
            let radius = crate::audio::length(c.position);
            let ground =
                crate::terrain::RADIUS + crate::terrain::height(crate::audio::unit(c.position));
            assert!((radius - ground - 3.).abs() < 1e-6);
        }
        assert!(crate::terrain::height(crate::audio::unit(sites[1].position)) > 100.);
    }
}

#[cfg(test)] mod theatre_tests {
 use super::*;
 #[test] fn explicit_theatre_button_selects_the_camera_while_midi_owns_keys(){
  let mut state=State::default();
  let camera=state.update("timeline",[0.;3],[0.;3],&json!({"theatre":true,"pan":[true,false,true,false]}),1.,true,1.6).unwrap();
  assert_eq!(camera["id"],"theatre");
  assert!((camera["pitch"].as_f64().unwrap().to_degrees()+8.).abs()<1e-9);
  assert_eq!(state.update("timeline",[0.;3],[0.;3],&json!({}),0.,false,1.6).unwrap(),camera);
 }
 #[test] fn theatre_cycles_in_world_coordinates_and_restores_page_aim(){
  let mut state=State::default();let origin=[100.,200.,300.];
  let view=|state:&mut State,page,cycle|state.update(page,origin,[0.;3],&json!({"cycle":cycle}),0.,false,1.6).unwrap();
  assert_eq!(view(&mut state,"timeline",false)["id"],"riverbank");
  assert_eq!(view(&mut state,"timeline",true)["id"],"mountain");
  view(&mut state,"timeline",false);
  let theatre=view(&mut state,"timeline",true);
  assert_eq!(theatre["id"],"theatre");assert_eq!(theatre["outdoor"],false);
  let position=crate::audio::vector(&theatre["position"]).unwrap();
  assert!((position[0]-100.).abs()<1e-9);assert!(position[1]>209.&&position[1]<211.);assert!(position[2]>256.&&position[2]<257.);
  assert_eq!(view(&mut state,"timeline",true),theatre);
  view(&mut state,"piano_roll",false);
  assert_eq!(view(&mut state,"timeline",false),theatre);
  assert_eq!(view(&mut state,"timeline",true)["id"],"riverbank");
 }
}

#[cfg(test)]mod piano_independent_camera_tests{use super::*;#[test]fn piano_and_timeline_have_separate_default_views_and_restore_pan(){let mut s=State::default();let a=s.update("timeline",[0.;3],[0.;3],&Value::Null,0.,false,1.6).unwrap();let b=s.update("piano_roll",[0.;3],[0.;3],&json!({"pan":[false,true,false,false]}),0.25,false,1.6).unwrap();assert_eq!(a["id"],"riverbank");assert_eq!(b["id"],"mountain");assert_eq!(a,s.update("timeline",[0.;3],[0.;3],&Value::Null,0.,false,1.6).unwrap());assert_eq!(b,s.update("piano_roll",[0.;3],[0.;3],&Value::Null,0.,false,1.6).unwrap());}}
