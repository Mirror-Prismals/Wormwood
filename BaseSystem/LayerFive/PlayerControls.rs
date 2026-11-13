//! Cardinal-style first-person input, adapted to the planet's radial frame.
//! Pure controller work owned by PlanetNavigationSystem.Step; no native calls.
use super::{RADIUS, V, cross, dot, normalized, rotate};
use serde_json::Value;

#[derive(Clone)]
pub(super) struct Settings {
    pub eye_height: f64,
    mouse: f64,
    pitch_limit: f64,
    flight_speed: f64,
    fast_flight_multiplier: f64,
    walk_start: f64,
    walk_speed: f64,
    sprint_speed: f64,
    walk_acceleration: f64,
    walk_deceleration: f64,
    walk_curve: f64,
    sprint_charge: f64,
    sprint_release: f64,
    sprint_curve: f64,
    default_walk_scale: f64,
    direction_tap_window: f64,
    flight_tap_window: f64,
    gravity: f64,
    jump: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            eye_height: 2.,
            mouse: 0.1f64.to_radians(),
            pitch_limit: 89f64.to_radians(),
            flight_speed: 5.,
            fast_flight_multiplier: 3.,
            walk_start: 2.3184,
            walk_speed: 5.04,
            sprint_speed: 9.,
            walk_acceleration: 1.05,
            walk_deceleration: 0.22,
            walk_curve: 1.6,
            sprint_charge: 1.45,
            sprint_release: 0.3,
            sprint_curve: 2.5,
            default_walk_scale: 0.25,
            direction_tap_window: 0.28,
            flight_tap_window: 0.32,
            gravity: 21.,
            jump: 8.,
        }
    }
}
impl Settings {
    pub fn load(root: &std::path::Path) -> Result<Self, String> {
        let file = "Procedures/player_controls.json";
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join(file)).map_err(|e| format!("{file}: {e}"))?,
        )
        .map_err(|e| format!("{file}: {e}"))?;
        if data["schema_version"] != 1 {
            return Err("unsupported player controls schema".into());
        }
        let n = |key: &str, lo: f64, hi: f64| {
            data[key]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= lo && *n <= hi)
                .ok_or_else(|| format!("{file}: invalid {key}"))
        };
        let mode = data["initial_mode"]
            .as_str()
            .ok_or("player initial_mode required")?;
        if mode != "walking" {
            return Err(
                "player initial_mode must be walking; enable cheats at runtime for flight".into(),
            );
        }
        let settings = Self {
            eye_height: n("eye_height", 0.5, 3.)?,
            mouse: n("mouse_degrees_per_pixel", 0.001, 1.)?.to_radians(),
            pitch_limit: n("pitch_limit_degrees", 1., 89.9)?.to_radians(),
            flight_speed: n("flight_speed", 0.1, 100.)?,
            fast_flight_multiplier: n("fast_flight_multiplier", 1., 100.)?,
            walk_start: n("walk_start_speed", 0.1, 30.)?,
            walk_speed: n("walk_speed", 0.1, 30.)?,
            sprint_speed: n("sprint_speed", 0.1, 100.)?,
            walk_acceleration: n("walk_acceleration_seconds", 0.02, 10.)?,
            walk_deceleration: n("walk_deceleration_seconds", 0.02, 10.)?,
            walk_curve: n("walk_curve", 0.1, 8.)?,
            sprint_charge: n("sprint_charge_seconds", 0.02, 10.)?,
            sprint_release: n("sprint_release_seconds", 0.02, 10.)?,
            sprint_curve: n("sprint_curve", 0.1, 8.)?,
            default_walk_scale: n("default_walk_scale", 0.01, 1.)?,
            direction_tap_window: n("direction_double_tap_seconds", 0.08, 0.8)?,
            flight_tap_window: n("flight_double_tap_seconds", 0.08, 0.8)?,
            gravity: n("gravity", 0.1, 100.)?,
            jump: n("jump_speed", 0.1, 30.)?,
        };
        if settings.walk_start > settings.walk_speed || settings.walk_speed > settings.sprint_speed
        {
            return Err("player speeds must satisfy walk_start <= walk <= sprint".into());
        }
        Ok(settings)
    }
}
pub(super) struct Player {
    pub position: V,
    pub velocity: V,
    pub right: V,
    pub up: V,
    pub forward: V,
    pub flying: bool,
    pub cheats: bool,
    pub noclip: bool,
    pub grounded: bool,
    pub sitting:bool,
    sit_was_down:bool,
    standing_eye:f64,
    pub settings: Settings,
    heading: V,
    pitch: f64,
    vertical_velocity: f64,
    space_was_down: bool,
    flight_tap: f64,
    movement_was_down: [bool; 4],
    direction_taps: [f64; 4],
    full_speed: [bool; 4],
    walk_charge: f64,
    sprint_charge: f64,
    sprinting: bool,
}
impl Default for Player {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}
impl Player {
    pub fn new(settings: Settings) -> Self {
        Self {
            position: [0., RADIUS + settings.eye_height, 0.],
            velocity: [0.; 3],
            right: [1., 0., 0.],
            up: [0., 1., 0.],
            forward: [0., 0., -1.],
            flying: false,
            cheats: false,
            noclip: false,
            grounded: true,
            sitting:false,sit_was_down:false,standing_eye:settings.eye_height,
            settings,
            heading: [0., 0., -1.],
            pitch: 0.,
            vertical_velocity: 0.,
            space_was_down: false,
            flight_tap: 0.,
            movement_was_down: [false; 4],
            direction_taps: [0.; 4],
            full_speed: [false; 4],
            walk_charge: 0.,
            sprint_charge: 0.,
            sprinting: false,
        }
    }
    pub fn accept_surf(&mut self,position:V,velocity:V,grounded:bool) {
        self.position=position;self.velocity=velocity;self.vertical_velocity=dot(velocity,normalized(position));self.grounded=grounded;
    }
    pub fn set_cheats(&mut self, enabled: bool, flight: bool, noclip: bool) {
        self.cheats = enabled;
        self.noclip = enabled && noclip;
        self.flying = enabled && (flight || noclip);
        self.velocity = [0.; 3];
        self.vertical_velocity = 0.;
        self.grounded = false;
        self.pause();
        self.basis();
    }
    pub fn place_spawn(&mut self, position: V, forward: V) {
        self.position=position;
        self.heading=normalized(forward);
        self.pitch=0.;
        self.velocity=[0.;3];
        self.vertical_velocity=0.;
        self.grounded=true;
        self.pause();
        self.basis();
    }
    /// Place a scripted view while retaining its full vertical look direction.
    pub fn place_view(&mut self, position: V, forward: V) {
        self.place_spawn(position, forward);
        self.pitch=dot(normalized(forward),normalized(position)).clamp(-1.,1.).asin()
            .clamp(-self.settings.pitch_limit,self.settings.pitch_limit);
        self.basis();
    }
    pub fn active(&self) -> bool {
        (!self.flying && !self.grounded)
            || dot(self.velocity, self.velocity) > 1e-6
            || self.flight_tap > 0.
            || self.direction_taps.iter().any(|t| *t > 0.)
            || self.walk_charge > 0.
            || self.sprint_charge > 0.
    }
    pub fn restrict_translation(&mut self, previous: V, accepted: V, dt: f64) {
        self.position = accepted;
        self.velocity = std::array::from_fn(|i| (accepted[i] - previous[i]) / dt.max(1e-9));
        self.vertical_velocity = dot(self.velocity, normalized(accepted));
    }
    pub fn pause(&mut self) {
        self.flight_tap = 0.;
        self.direction_taps = [0.; 4];
        self.movement_was_down = [false; 4];
        self.full_speed = [false; 4];
        self.space_was_down = false;
    }
    fn basis(&mut self) {
        let radial = normalized(self.position);
        self.heading = normalized(std::array::from_fn(|i| {
            self.heading[i] - radial[i] * dot(self.heading, radial)
        }));
        self.right = normalized(cross(self.heading, radial));
        self.forward = normalized(std::array::from_fn(|i| {
            self.heading[i] * self.pitch.cos() + radial[i] * self.pitch.sin()
        }));
        self.up = normalized(cross(self.right, self.forward));
    }
    pub fn advance(&mut self, input: &Value, surface_height: f64) {
        if input["reset"] == true {
            let mut settings=self.settings.clone();settings.eye_height=self.standing_eye;
            *self = Self::new(settings);
            return;
        }
        let dt = input["dt"].as_f64().unwrap().clamp(0., 0.05);
        let radial = normalized(self.position);
        let sit=input["sit"]==true;
        if sit && !self.sit_was_down && !self.flying {
            let next=if self.sitting {self.standing_eye}else{0.60};
            self.position=std::array::from_fn(|i|self.position[i]+radial[i]*(next-self.settings.eye_height));
            self.settings.eye_height=next;self.sitting=!self.sitting;
        }
        self.sit_was_down=sit;
        if self.flying && self.sitting {
            self.position=std::array::from_fn(|i|self.position[i]+radial[i]*(self.standing_eye-self.settings.eye_height));
            self.settings.eye_height=self.standing_eye;self.sitting=false;
        }
        self.heading = rotate(
            self.heading,
            radial,
            -input["look"][0].as_f64().unwrap() * self.settings.mouse,
        );
        self.pitch = (self.pitch - input["look"][1].as_f64().unwrap() * self.settings.mouse)
            .clamp(-self.settings.pitch_limit, self.settings.pitch_limit);
        self.basis();
        let k = &input["move"];
        let space = k[4] == true;
        let space_pressed = space && !self.space_was_down;
        self.space_was_down = space;
        self.flight_tap = (self.flight_tap - dt).max(0.);
        let mut toggled = false;
        if let Some(mode) = input["mode_request"].as_str().filter(|_| !self.noclip) {
            let flying = self.cheats && mode == "flight";
            if self.flying != flying {
                self.flying = flying;
                self.flight_tap = 0.;
                self.vertical_velocity = 0.;
                self.velocity = [0.; 3];
                toggled = true;
            }
        }
        if self.cheats && !self.noclip && space_pressed && input["mode_request"].is_null() {
            if self.flight_tap > 0. {
                self.flying = !self.flying;
                toggled = true;
                self.flight_tap = 0.;
                self.vertical_velocity = 0.;
                self.velocity = [0.; 3];
            } else {
                self.flight_tap = self.settings.flight_tap_window;
            }
        }
        let f = input["axis"][1]
            .as_f64()
            .unwrap_or(f64::from(k[0] == true) - f64::from(k[1] == true));
        let side = input["axis"][0]
            .as_f64()
            .unwrap_or(f64::from(k[3] == true) - f64::from(k[2] == true));
        let raw: V = std::array::from_fn(|i| self.heading[i] * f + self.right[i] * side);
        let magnitude = dot(raw, raw).sqrt();
        let direction = raw.map(|v| v / magnitude.max(1.));
        let moving = magnitude > 0.;
        for i in 0..4 {
            let held = k[i] == true;
            self.direction_taps[i] = (self.direction_taps[i] - dt).max(0.);
            if held && !self.movement_was_down[i] {
                self.full_speed[i] = self.direction_taps[i] > 0.;
                self.direction_taps[i] = if self.full_speed[i] {
                    0.
                } else {
                    self.settings.direction_tap_window
                };
            }
            if !held {
                self.full_speed[i] = false;
            }
            self.movement_was_down[i] = held;
        }
        if self.flying {
            let vertical = f64::from(space) - f64::from(k[5] == true);
            let travel: V = std::array::from_fn(|i| {
                if self.noclip {
                    self.forward[i] * f + self.right[i] * side + radial[i] * vertical
                } else {
                    raw[i] + radial[i] * vertical
                }
            });
            let length = dot(travel, travel).sqrt().max(1.);
            let speed = self.settings.flight_speed
                * if input["boost"] == true {
                    self.settings.fast_flight_multiplier
                } else {
                    1.
                };
            self.velocity = travel.map(|v| v / length * speed);
            self.vertical_velocity = 0.;
            self.walk_charge = 0.;
            self.sprint_charge = 0.;
            self.sprinting = false;
        } else if self.sitting {
            // Ground sitting retains momentum. Contact supplies slope reaction;
            // ordinary walking's target-speed response must not brake a surf.
            self.velocity=std::array::from_fn(|i|self.velocity[i]+direction[i]*3.*dt-radial[i]*9.81*dt);
            if self.grounded {
                let normal_speed=dot(self.velocity,radial);
                let drag=(1.-0.7*dt).max(0.);
                self.velocity=std::array::from_fn(|i|(self.velocity[i]-radial[i]*normal_speed)*drag+radial[i]*normal_speed);
            }
            if space_pressed && self.grounded {
                self.velocity=std::array::from_fn(|i|self.velocity[i]+radial[i]*5.);
                self.grounded=false;
            }
            self.vertical_velocity=dot(self.velocity,radial);
        } else {
            self.walk_charge = (self.walk_charge
                + if moving {
                    dt / self.settings.walk_acceleration
                } else {
                    -dt / self.settings.walk_deceleration
                })
            .clamp(0., 1.);
            if k[5] != true {
                self.sprinting = false;
            } else if moving && self.grounded && self.walk_charge >= 0.999 {
                self.sprinting = true;
            }
            if self.grounded {
                self.sprint_charge = (self.sprint_charge
                    + if self.sprinting && moving {
                        dt / self.settings.sprint_charge
                    } else {
                        -dt / self.settings.sprint_release
                    })
                .clamp(0., 1.);
            }
            let walk = self.settings.walk_start
                + (self.settings.walk_speed - self.settings.walk_start)
                    * self.walk_charge.powf(self.settings.walk_curve);
            let speed = (walk
                + (self.settings.sprint_speed - self.settings.walk_speed)
                    * self.sprint_charge.powf(self.settings.sprint_curve))
                * if input["mobile"] == true || self.full_speed.iter().any(|v| *v) {
                    1.
                } else {
                    self.settings.default_walk_scale
                };
            let old_vertical = dot(self.velocity, radial);
            let tangent: V = std::array::from_fn(|i| self.velocity[i] - radial[i] * old_vertical);
            let response = (dt
                / if self.grounded {
                    self.settings.walk_deceleration
                } else {
                    0.55
                })
            .min(1.);
            let mut horizontal: V = std::array::from_fn(|i| {
                tangent[i] + (direction[i] * speed - tangent[i]) * response
            });
            if !moving && dot(horizontal, horizontal) < 1e-6 {
                horizontal = [0.; 3];
            }
            if self.grounded && space_pressed && !toggled {
                self.vertical_velocity = self.settings.jump;
                self.grounded = false;
            }
            if !self.grounded {
                self.vertical_velocity -= self.settings.gravity * dt;
            }
            self.velocity =
                std::array::from_fn(|i| horizontal[i] + radial[i] * self.vertical_velocity);
        }
        for i in 0..3 {
            self.position[i] += self.velocity[i] * dt;
        }
        // Transport the horizontal frame along the spherical surface.
        let next = normalized(self.position);
        let axis = cross(radial, next);
        let length = dot(axis, axis).sqrt();
        if length > 1e-12 {
            self.heading = rotate(
                self.heading,
                axis.map(|x| x / length),
                length.atan2(dot(radial, next)),
            );
        }
        if !self.noclip {
            self.resolve_surface(surface_height);
        }
    }
    pub fn advect(&mut self, current: V, dt: f64) {
        if !current.iter().all(|n| n.is_finite()) || !dt.is_finite() || !(0.0..=0.1).contains(&dt) {
            return;
        }
        for i in 0..3 {
            self.position[i] += current[i] * dt;
        }
        self.basis();
    }
    pub fn resolve_volume_contact(&mut self, normal: V) {
        let inward = dot(self.velocity, normal).min(0.);
        self.velocity = std::array::from_fn(|i| self.velocity[i] - normal[i] * inward);
        if dot(normal, normalized(self.position)) > 0.55 && self.vertical_velocity <= 0. {
            self.vertical_velocity = 0.;
            self.grounded = true;
        }
        self.basis();
    }
    pub fn resolve_surface(&mut self, surface_height: f64) {
        let radial = normalized(self.position);
        let floor = RADIUS + surface_height + self.settings.eye_height;
        let height = dot(self.position, self.position).sqrt();
        if height < floor || (height <= floor + 1e-5 && self.vertical_velocity <= 0.) {
            self.position = radial.map(|v| v * floor);
            let inward = dot(self.velocity, radial).min(0.);
            self.velocity = std::array::from_fn(|i| self.velocity[i] - radial[i] * inward);
            self.vertical_velocity = self.vertical_velocity.max(0.);
            self.grounded = self.vertical_velocity <= 0.;
        } else {
            self.grounded = false;
        }
        self.basis();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input(space: bool) -> Value {
        json!({"dt":0.05,"move":[false,false,false,false,space,false],"look":[0.,0.],"boost":false})
    }
    #[test]
    fn sitting_preserves_feet_and_momentum_and_reset_restores_standing() {
        let mut p=Player::default();p.velocity=[7.,0.,0.];
        let mut down=input(false);down["sit"]=json!(true);let feet=p.position[1]-p.settings.eye_height;
        p.advance(&down,0.);p.resolve_surface(0.);
        assert!(p.sitting && (p.settings.eye_height-0.6).abs()<1e-9);
        assert!((p.position[1]-p.settings.eye_height-feet).abs()<1e-4);
        assert!(p.velocity[0]>6.5);
        p.advance(&down,0.);assert!(p.sitting); // Held key does not repeat toggles.
        down["reset"]=json!(true);p.advance(&down,0.);
        assert!(!p.sitting && (p.settings.eye_height-2.).abs()<1e-9);
    }
    #[test]
    fn cheats_off_rejects_flight_shortcuts_and_new_sessions_start_walking() {
        let mut p = Player::default();
        assert!(!p.cheats && !p.flying && !p.noclip);
        for space in [true, false, true, false] {
            p.advance(&input(space), 0.);
            assert!(!p.flying);
        }
        let mut mode = input(false);
        mode["mode_request"] = json!("flight");
        p.advance(&mode, 0.);
        assert!(!p.flying);
    }
    #[test]
    fn cheats_noclip_can_cross_the_floor_while_regular_flight_collides() {
        let mut p = Player::default();
        p.set_cheats(true, true, true);
        p.pitch = -0.7;
        p.basis();
        let mut moving = input(false);
        moving["move"][0] = json!(true);
        let before = dot(p.position, p.position).sqrt();
        p.advance(&moving, 0.);
        assert!(dot(p.position, p.position).sqrt() < before - 0.05);
        p.set_cheats(true, true, false);
        p.advance(&input(false), 0.);
        assert!(dot(p.position, p.position).sqrt() >= RADIUS + p.settings.eye_height - 1e-6);
    }
    #[test]
    fn cheats_disable_clears_flight_noclip_and_held_gesture_state() {
        let mut p = Player::default();
        p.set_cheats(true, false, true);
        assert!(p.flying && p.noclip);
        p.velocity = [4., 5., 6.];
        p.flight_tap = 0.2;
        p.set_cheats(false, true, true);
        assert!(!p.cheats && !p.flying && !p.noclip);
        assert_eq!(p.velocity, [0.; 3]);
        assert_eq!(p.flight_tap, 0.);
        let mut mode = input(false);
        mode["mode_request"] = json!("flight");
        p.advance(&mode, 0.);
        assert!(!p.flying);
    }
}
