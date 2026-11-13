//! Deterministic phone behavior adapted from Cardinal's PhoneSystem.
//! Native input, presentation, lighting and real sensor data belong to the
//! calling Systems. Positions and poses are continuous, with no world format.

use serde_json::{Value, json};

pub(crate) type Vector = [f64; 3];
pub(crate) const EQUIP_RADIUS: f64 = 3.25;
pub(crate) const MAX_FORECAST_POINTS: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Screen {
    #[default]
    Home,
    Flashlight,
    Camera,
    Meter,
    Weather,
}

impl Screen {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Flashlight => "flashlight",
            Self::Camera => "camera",
            Self::Meter => "meter",
            Self::Weather => "weather",
        }
    }
}

/// Display basis: right, up and outward front form a right-handed frame.
/// Position is the model's visual center; model-specific pivots stay in art.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub position: Vector,
    pub right: Vector,
    pub up: Vector,
    pub front: Vector,
}

impl Pose {
    pub(crate) fn new(position: Vector, right: Vector, up: Vector) -> Result<Self, String> {
        finite(position)?;
        let right = unit(right)?;
        let front = unit(cross(right, unit(up)?))?;
        let up = cross(front, right);
        Ok(Self { position, right, up, front })
    }

    pub(crate) fn validate(self) -> Result<(), String> {
        finite(self.position)?;
        for axis in [self.right, self.up, self.front] {
            finite(axis)?;
            if (dot(axis, axis) - 1.).abs() > 1e-6 {
                return Err("phone pose axes must have unit length".into());
            }
        }
        if dot(self.right, self.up).abs() > 1e-6
            || dot(cross(self.right, self.up), self.front) < 1. - 1e-6
        {
            return Err("phone pose requires an orthogonal right-handed basis".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HeldPose {
    pub pose: Pose,
    pub scale: f64,
}

/// Cardinal's eye-relative offsets and focus enlargement, without its model
/// cell offset. Forward points into the view; the display faces the eye.
pub(crate) fn held_pose(
    eye: Vector,
    forward: Vector,
    up: Vector,
    focused: bool,
) -> Result<HeldPose, String> {
    finite(eye)?;
    let forward = unit(forward)?;
    let right = unit(cross(forward, unit(up)?))?;
    let up = cross(right, forward);
    let (depth, vertical, side, scale) = if focused {
        (0.43, -0.01, 0., 1.8)
    } else {
        (0.58, -0.16, 0.26, 1.15)
    };
    let position = std::array::from_fn(|i| {
        eye[i] + forward[i] * depth + up[i] * vertical + right[i] * side
    });
    Ok(HeldPose { pose: Pose::new(position, right, up)?, scale })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Location {
    #[default]
    Stowed,
    Held,
    Placed(Pose),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScreenAction {
    Open(Screen),
    ToggleFlashlight,
    ResetMeter,
}

/// UV is normalized from the upper-left of Cardinal's 800 by 1200 display.
/// Match its integer-pixel inclusive hit regions, including their empty gaps.
pub(crate) fn screen_hit(screen: Screen, uv: [f64; 2]) -> Option<ScreenAction> {
    if uv.iter().any(|n| !n.is_finite() || !(0.0..=1.).contains(n)) {
        return None;
    }
    let x = (uv[0].min(0.999999) * 800.).floor() as u32;
    let y = (uv[1].min(0.999999) * 1200.).floor() as u32;
    let inside = |a, b, c, d| x >= a && x <= c && y >= b && y <= d;
    if screen == Screen::Home {
        for (bounds, destination) in [
            ([72, 170, 322, 420], Screen::Flashlight),
            ([478, 170, 728, 420], Screen::Camera),
            ([72, 620, 322, 870], Screen::Meter),
            ([478, 620, 728, 870], Screen::Weather),
        ] {
            if inside(bounds[0], bounds[1], bounds[2], bounds[3]) {
                return Some(ScreenAction::Open(destination));
            }
        }
    } else if inside(688, 16, 780, 108) {
        return Some(ScreenAction::Open(Screen::Home));
    } else if screen == Screen::Meter && inside(230, 1030, 570, 1130) {
        return Some(ScreenAction::ResetMeter);
    } else if screen == Screen::Flashlight && inside(154, 414, 646, 786) {
        return Some(ScreenAction::ToggleFlashlight);
    }
    None
}

/// Commands are edges, not held key/button levels. The caller owns debouncing,
/// target selection, hand occupancy and collision-safe placement validation.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    /// F: pick up a selected placed phone, leave focus, or place a held phone.
    Interact { observer: Vector, hands_free: bool, placement: Option<Pose> },
    /// R toggles focus only while held.
    ToggleFocus,
    /// Escape or another UI taking focus. A held light stays on.
    Blur,
    Tap([f64; 2]),
    /// Disablement releases focus and switches off light without losing place.
    Available(bool),
    /// The hand no longer owns the phone; selection is retained.
    Stow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interaction {
    Equipped,
    Placed,
}

/// Changes request work from the caller; this module creates no device, camera
/// target, light, sound, file, network operation or application Audicle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    pub changed: bool,
    pub focus_changed: bool,
    pub flashlight_changed: bool,
    pub camera_changed: bool,
    pub weather_refresh: bool,
    pub interaction: Option<Interaction>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AudioSample {
    /// Actual listener output, not microphone input or source-bus estimates.
    pub rms: f64,
    pub peak: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Meter {
    pub available: bool,
    pub db: f64,
    pub max_db: f64,
    mean_square: f64,
    refresh: f64,
}

impl Default for Meter {
    fn default() -> Self {
        Self { available: false, db: -96., max_db: -96., mean_square: 0., refresh: 0. }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Conditions {
    pub temperature_c: f64,
    pub humidity_percent: f64,
    pub wind_kmh: f64,
    pub wind_direction: String,
    pub pressure_hpa: f64,
    pub precipitation_mm_h: f64,
    pub condition: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ForecastPoint {
    pub lead_hours: f64,
    pub temperature_c: f64,
    pub precipitation_mm_h: f64,
    pub cloud_coverage: f64,
    pub condition: String,
}

/// None/current and an empty forecast mean unavailable, never invented weather.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WeatherSnapshot {
    pub current: Option<Conditions>,
    pub forecast: Vec<ForecastPoint>,
    pub status: String,
}

impl WeatherSnapshot {
    fn validate(&self) -> Result<(), String> {
        if self.forecast.len() > MAX_FORECAST_POINTS || self.status.len() > 256 {
            return Err("phone weather snapshot exceeds its bounded capacity".into());
        }
        if let Some(c) = &self.current {
            if [c.temperature_c, c.humidity_percent, c.wind_kmh, c.pressure_hpa,
                c.precipitation_mm_h].iter().any(|v| !v.is_finite())
                || !(0.0..=100.).contains(&c.humidity_percent)
                || c.wind_kmh < 0. || c.precipitation_mm_h < 0.
                || c.condition.len() > 128 || c.wind_direction.len() > 32
            {
                return Err("invalid phone weather conditions".into());
            }
        }
        for p in &self.forecast {
            if [p.lead_hours, p.temperature_c, p.precipitation_mm_h,
                p.cloud_coverage].iter().any(|v| !v.is_finite())
                || !(0.0..=24.01).contains(&p.lead_hours)
                || !(0.0..=1.).contains(&p.cloud_coverage)
                || p.precipitation_mm_h < 0. || p.condition.len() > 128
            {
                return Err("invalid phone forecast point".into());
            }
        }
        Ok(())
    }

    /// Same nearest-sample selection used for Cardinal's 24 hourly bars and
    /// four six-hour cards. Missing nearby evidence stays missing.
    pub(crate) fn at_hour(&self, hour: f64) -> Option<&ForecastPoint> {
        if !hour.is_finite() || !(0.0..=24.).contains(&hour) { return None; }
        let mut nearest = None;
        let mut distance = 3.01;
        for point in &self.forecast {
            let delta = (point.lead_hours - hour).abs();
            if delta <= distance && (nearest.is_none() || delta < distance) {
                nearest = Some(point);
                distance = delta;
            }
        }
        nearest
    }
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    available: bool,
    location: Location,
    focused: bool,
    screen: Screen,
    flashlight: bool,
    meter: Meter,
    weather: WeatherSnapshot,
    weather_refresh: f64,
}

impl Default for State {
    fn default() -> Self {
        Self { available: true, location: Location::Stowed, focused: false,
            screen: Screen::Home, flashlight: false, meter: Meter::default(),
            weather: WeatherSnapshot::default(), weather_refresh: 0. }
    }
}

impl State {
    pub(crate) fn placed(pose: Pose) -> Result<Self, String> {
        pose.validate()?;
        Ok(Self { location: Location::Placed(pose), ..Self::default() })
    }

    pub(crate) fn location(&self) -> Location { self.location }
    pub(crate) fn screen(&self) -> Screen { self.screen }
    pub(crate) fn available(&self) -> bool { self.available }
    pub(crate) fn focused(&self) -> bool { self.focused }
    pub(crate) fn flashlight_on(&self) -> bool { self.flashlight }
    pub(crate) fn camera_live(&self) -> bool {
        self.available && self.location == Location::Held && self.screen == Screen::Camera
    }
    pub(crate) fn meter(&self) -> &Meter { &self.meter }
    pub(crate) fn weather(&self) -> &WeatherSnapshot { &self.weather }

    /// Persist only the phone's identity-independent position and app state.
    /// Sensor readings and cursor ownership are transient application data.
    pub(crate) fn snapshot(&self) -> Value {
        let location = match self.location {
            Location::Stowed => json!({"kind":"stowed"}),
            Location::Held => json!({"kind":"held"}),
            Location::Placed(pose) => json!({"kind":"placed","pose":{
                "position":pose.position,"right":pose.right,"up":pose.up,"front":pose.front}}),
        };
        json!({"schema_version":1,"location":location,"screen":self.screen.name(),
            "flashlight_on":self.flashlight})
    }

    /// A restart never acquires cursor focus or reactivates a light. A saved
    /// light flag is still validated so malformed state is rejected atomically.
    pub(crate) fn restore(&mut self, snapshot: &Value) -> Result<(), String> {
        if snapshot["schema_version"].as_u64() != Some(1) {
            return Err("unsupported phone snapshot schema".into());
        }
        let screen = match snapshot["screen"].as_str() {
            Some("home") => Screen::Home,
            Some("flashlight") => Screen::Flashlight,
            Some("camera") => Screen::Camera,
            Some("meter") => Screen::Meter,
            Some("weather") => Screen::Weather,
            _ => return Err("unknown phone screen in snapshot".into()),
        };
        let light = snapshot["flashlight_on"].as_bool().ok_or("phone snapshot needs a light flag")?;
        let location = match snapshot["location"]["kind"].as_str() {
            Some("stowed") => Location::Stowed,
            Some("held") => Location::Held,
            Some("placed") => {
                let saved = &snapshot["location"]["pose"];
                let pose = Pose { position: read_vector(&saved["position"])?,
                    right: read_vector(&saved["right"])?, up: read_vector(&saved["up"])?,
                    front: read_vector(&saved["front"])? };
                pose.validate()?;
                Location::Placed(pose)
            }
            _ => return Err("unknown phone location in snapshot".into()),
        };
        if light && (location != Location::Held || screen != Screen::Flashlight) {
            return Err("saved phone light requires a held flashlight app".into());
        }
        *self = Self { available: self.available, location, screen, ..Self::default() };
        Ok(())
    }

    pub(crate) fn status(&self) -> Value {
        let mut result = self.snapshot();
        result["available"] = json!(self.available);
        result["focused"] = json!(self.focused);
        result["camera_live"] = json!(self.camera_live());
        result["meter"] = json!({"available":self.meter.available,
            "db":self.meter.db,"max_db":self.meter.max_db});
        let current = self.weather.current.as_ref().map(|c| json!({
            "temperature_c":c.temperature_c,"humidity_percent":c.humidity_percent,
            "wind_kmh":c.wind_kmh,"wind_direction":c.wind_direction,
            "pressure_hpa":c.pressure_hpa,"precipitation_mm_h":c.precipitation_mm_h,
            "condition":c.condition}));
        let forecast: Vec<Value> = self.weather.forecast.iter().map(|p| json!({
            "lead_hours":p.lead_hours,"temperature_c":p.temperature_c,
            "precipitation_mm_h":p.precipitation_mm_h,"cloud_coverage":p.cloud_coverage,
            "condition":p.condition})).collect();
        result["weather"] = json!({"current":current,"forecast":forecast,"status":self.weather.status});
        result
    }

    pub(crate) fn apply(&mut self, command: Command) -> Result<Effects, String> {
        let previous = (self.focused, self.flashlight, self.camera_live(), self.location, self.available);
        let mut effects = Effects::default();
        match command {
            Command::Available(available) => {
                self.available = available;
                if !available { self.focused = false; self.flashlight = false; }
            }
            Command::Blur => self.focused = false,
            Command::Stow => {
                self.location = Location::Stowed;
                self.focused = false;
                self.flashlight = false;
            }
            _ if !self.available => {}
            Command::ToggleFocus if self.location == Location::Held => self.focused = !self.focused,
            Command::Interact { observer, hands_free, placement } => {
                finite(observer)?;
                match self.location {
                    Location::Held if self.focused => self.focused = false,
                    Location::Held => if let Some(pose) = placement {
                        pose.validate()?;
                        self.location = Location::Placed(pose);
                        self.flashlight = false;
                        effects.interaction = Some(Interaction::Placed);
                    },
                    Location::Placed(pose) if hands_free => {
                        let delta = std::array::from_fn(|i| observer[i] - pose.position[i]);
                        if length(delta) <= EQUIP_RADIUS {
                            self.location = Location::Held;
                            effects.interaction = Some(Interaction::Equipped);
                        }
                    }
                    _ => {}
                }
            }
            Command::Tap(uv) if self.focused && self.location == Location::Held => {
                if let Some(action) = screen_hit(self.screen, uv) {
                    effects.changed = true;
                    match action {
                        ScreenAction::Open(screen) => {
                            self.screen = screen;
                            self.flashlight = screen == Screen::Flashlight;
                            if screen == Screen::Meter { self.meter = Meter::default(); }
                            if screen == Screen::Weather {
                                self.weather_refresh = 1.;
                                effects.weather_refresh = true;
                            }
                        }
                        ScreenAction::ToggleFlashlight => self.flashlight = !self.flashlight,
                        ScreenAction::ResetMeter => self.meter.max_db = self.meter.db,
                    }
                }
            }
            _ => {}
        }
        effects.focus_changed = previous.0 != self.focused;
        effects.flashlight_changed = previous.1 != self.flashlight;
        effects.camera_changed = previous.2 != self.camera_live();
        effects.changed |= effects.focus_changed || effects.flashlight_changed || effects.camera_changed
            || previous.3 != self.location || previous.4 != self.available;
        Ok(effects)
    }

    /// One bounded display tick (0..1 second). Missing native audio is exposed
    /// as unavailable instead of treating it as a measured silent listener.
    pub(crate) fn tick(&mut self, dt: f64, audio: Option<AudioSample>) -> Result<Effects, String> {
        if !dt.is_finite() || !(0.0..=1.).contains(&dt) {
            return Err("phone display dt must be within 0..1 second".into());
        }
        if let Some(sample) = audio {
            if !sample.rms.is_finite() || !sample.peak.is_finite()
                || sample.rms < 0. || sample.peak < 0. || !(sample.rms * sample.rms).is_finite()
            {
                return Err("phone meter requires finite nonnegative listener RMS and peak".into());
            }
        }
        let mut effects = Effects::default();
        if !self.available { return Ok(effects); }
        if self.screen == Screen::Meter {
            effects.changed = self.meter.available != audio.is_some();
            self.meter.available = audio.is_some();
            if let Some(sample) = audio {
                let blend = 1. - (-dt / 0.125).exp();
                self.meter.mean_square += (sample.rms * sample.rms - self.meter.mean_square) * blend;
                self.meter.db = (-96f64).max(10. * self.meter.mean_square.max(1e-10).log10());
                self.meter.max_db = self.meter.max_db.max((-96f64).max(20. * sample.peak.max(1e-5).log10()));
            }
            self.meter.refresh -= dt;
            if self.meter.refresh <= 0. {
                self.meter.refresh = 0.1;
                effects.changed = true;
            }
        }
        if self.screen == Screen::Weather && self.location != Location::Stowed {
            self.weather_refresh -= dt;
            if self.weather_refresh <= 0. {
                self.weather_refresh = 1.;
                effects.weather_refresh = true;
            }
        }
        Ok(effects)
    }

    pub(crate) fn set_weather(&mut self, mut snapshot: WeatherSnapshot) -> Result<Effects, String> {
        snapshot.validate()?;
        snapshot.forecast.sort_by(|a, b| a.lead_hours.total_cmp(&b.lead_hours));
        let changed = self.weather != snapshot;
        self.weather = snapshot;
        Ok(Effects { changed: changed && self.screen == Screen::Weather, ..Effects::default() })
    }
}

fn finite(value: Vector) -> Result<(), String> {
    if value.iter().all(|v| v.is_finite() && v.abs()<=1_000_000_000.) { Ok(()) }
    else { Err("phone pose requires finite coordinates".into()) }
}
fn read_vector(value: &Value) -> Result<Vector, String> {
    let values = value.as_array().filter(|a| a.len() == 3).ok_or("phone pose requires three coordinates")?;
    let mut vector = [0.; 3];
    for i in 0..3 { vector[i] = values[i].as_f64().ok_or("phone pose requires numeric coordinates")?; }
    finite(vector)?;
    Ok(vector)
}
fn dot(a: Vector, b: Vector) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn length(a: Vector) -> f64 { a[0].hypot(a[1]).hypot(a[2]) }
fn unit(a: Vector) -> Result<Vector, String> {
    finite(a)?;
    let size = length(a);
    if !size.is_finite() || size <= 1e-9 { return Err("phone basis requires nonzero axes".into()); }
    Ok(a.map(|v| v / size))
}
fn cross(a: Vector, b: Vector) -> Vector {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(position: Vector) -> Pose { Pose::new(position, [1., 0., 0.], [0., 1., 0.]).unwrap() }
    fn interact(observer: Vector, hands_free: bool, placement: Option<Pose>) -> Command {
        Command::Interact { observer, hands_free, placement }
    }
    fn tap(state: &mut State, x: f64, y: f64) -> Effects {
        state.apply(Command::Tap([x / 800., y / 1200.])).unwrap()
    }
    fn focused() -> State {
        let mut state = State::placed(pose([0.; 3])).unwrap();
        state.apply(interact([0.; 3], true, None)).unwrap();
        state.apply(Command::ToggleFocus).unwrap();
        state
    }

    #[test]
    fn phone_screen_hits_match_cardinal_pixels_and_reject_gaps() {
        for (point, screen) in [([197., 295.], Screen::Flashlight), ([603., 295.], Screen::Camera),
            ([197., 745.], Screen::Meter), ([603., 745.], Screen::Weather)] {
            assert_eq!(screen_hit(Screen::Home, [point[0] / 800., point[1] / 1200.]), Some(ScreenAction::Open(screen)));
        }
        assert_eq!(screen_hit(Screen::Home, [400. / 800., 295. / 1200.]), None);
        assert_eq!(screen_hit(Screen::Home, [323.5 / 800., 295. / 1200.]), None);
        assert_eq!(screen_hit(Screen::Home, [-0.001, 0.2]), None);
        assert_eq!(screen_hit(Screen::Home, [f64::NAN, 0.2]), None);
        for screen in [Screen::Flashlight, Screen::Camera, Screen::Meter, Screen::Weather] {
            assert_eq!(screen_hit(screen, [734. / 800., 62. / 1200.]), Some(ScreenAction::Open(Screen::Home)));
        }
        // Camera is a live view. Its center does not invent a shutter control.
        assert_eq!(screen_hit(Screen::Camera, [0.5, 0.5]), None);
    }

    #[test]
    fn phone_pickup_requires_free_hand_and_nearby_selected_phone() {
        let mut state = State::placed(pose([1.125, 2.5, -3.75])).unwrap();
        assert_eq!(state.apply(interact([4.376, 2.5, -3.75], true, None)).unwrap().interaction, None);
        assert_eq!(state.apply(interact([1.125, 2.5, -3.75], false, None)).unwrap().interaction, None);
        let result = state.apply(interact([4.375, 2.5, -3.75], true, None)).unwrap();
        assert_eq!(result.interaction, Some(Interaction::Equipped));
        assert_eq!(state.location(), Location::Held);
        assert!(!state.focused());
        assert!(!tap(&mut state, 197., 295.).changed);
        assert_eq!(state.screen(), Screen::Home);
    }

    #[test]
    fn phone_focus_exit_precedes_placement_and_preserves_app() {
        let mut state = focused();
        tap(&mut state, 197., 295.);
        assert!(state.flashlight_on());
        let destination = pose([2.25, 0.83, -7.125]);
        let first = state.apply(interact([0.; 3], false, Some(destination))).unwrap();
        assert!(first.focus_changed);
        assert_eq!(first.interaction, None);
        assert_eq!(state.location(), Location::Held);
        assert!(state.flashlight_on());
        let second = state.apply(interact([0.; 3], false, Some(destination))).unwrap();
        assert_eq!(second.interaction, Some(Interaction::Placed));
        assert!(!state.flashlight_on());
        assert_eq!(state.screen(), Screen::Flashlight);
        assert_eq!(state.location(), Location::Placed(destination));
        state.apply(interact(destination.position, true, None)).unwrap();
        assert!(!state.flashlight_on());
        state.apply(Command::ToggleFocus).unwrap();
        tap(&mut state, 400., 600.);
        assert!(state.flashlight_on());
        tap(&mut state, 734., 62.);
        assert_eq!(state.screen(), Screen::Home);
        assert!(!state.flashlight_on());
    }

    #[test]
    fn phone_camera_requires_held_phone_but_not_cursor_focus() {
        let mut state = focused();
        assert!(tap(&mut state, 603., 295.).camera_changed);
        assert!(state.camera_live());
        assert!(state.apply(Command::Blur).unwrap().focus_changed);
        assert!(state.camera_live());
        let placed = state.apply(interact([0.; 3], false, Some(pose([0.; 3])))).unwrap();
        assert!(placed.camera_changed);
        assert!(!state.camera_live());
        assert_eq!(state.screen(), Screen::Camera);
        assert!(state.apply(interact([0.; 3], true, None)).unwrap().camera_changed);
        state.apply(Command::Available(false)).unwrap();
        assert!(!state.camera_live());
    }

    #[test]
    fn phone_blur_keeps_light_but_disable_and_stow_release_it() {
        let mut state = focused();
        tap(&mut state, 197., 295.);
        state.apply(Command::Blur).unwrap();
        assert!(state.flashlight_on());
        state.apply(Command::Available(false)).unwrap();
        assert!(!state.focused());
        assert!(!state.flashlight_on());
        state.apply(Command::ToggleFocus).unwrap();
        assert!(!state.focused());
        state.apply(Command::Available(true)).unwrap();
        state.apply(Command::ToggleFocus).unwrap();
        tap(&mut state, 400., 600.);
        assert!(state.flashlight_on());
        state.apply(Command::Stow).unwrap();
        assert_eq!(state.location(), Location::Stowed);
        assert!(!state.focused());
        assert!(!state.flashlight_on());
    }

    #[test]
    fn phone_meter_uses_listener_rms_peak_and_reset_without_faking_missing_audio() {
        let mut state = focused();
        tap(&mut state, 197., 745.);
        state.tick(0.125, Some(AudioSample { rms: 0.5, peak: 0.8 })).unwrap();
        let expected = 10. * (0.25 * (1. - (-1f64).exp())).log10();
        assert!((state.meter().db - expected).abs() < 1e-10);
        assert!((state.meter().max_db - 20. * 0.8f64.log10()).abs() < 1e-10);
        tap(&mut state, 400., 1080.);
        assert_eq!(state.meter().max_db, state.meter().db);
        let saved = state.meter().db;
        state.tick(0.1, None).unwrap();
        assert!(!state.meter().available);
        assert_eq!(state.meter().db, saved);
        assert!(state.tick(0.1, Some(AudioSample { rms: f64::INFINITY, peak: 0. })).is_err());
        assert_eq!(state.meter().db, saved);
    }

    #[test]
    fn phone_weather_is_bounded_real_data_with_one_hz_refresh() {
        let mut state = focused();
        assert!(tap(&mut state, 603., 745.).weather_refresh);
        assert!(state.weather().current.is_none());
        assert!(!state.tick(0.5, None).unwrap().weather_refresh);
        assert!(state.tick(0.5, None).unwrap().weather_refresh);
        let point = |lead_hours| ForecastPoint { lead_hours, temperature_c: 13., precipitation_mm_h: 0.4,
            cloud_coverage: 0.7, condition: "Rain".into() };
        let snapshot = WeatherSnapshot { forecast: vec![point(24.), point(6.)], ..Default::default() };
        assert!(state.set_weather(snapshot).unwrap().changed);
        assert_eq!(state.weather().at_hour(6.).unwrap().temperature_c, 13.);
        assert!(state.weather().at_hour(12.).is_none());
        let saved = state.weather().clone();
        assert!(state.set_weather(WeatherSnapshot { forecast: vec![point(1.); 257], ..Default::default() }).is_err());
        assert_eq!(state.weather(), &saved);
        assert!(state.set_weather(WeatherSnapshot { forecast: vec![point(f64::NAN)], ..Default::default() }).is_err());
    }

    #[test]
    fn phone_pose_follows_rendered_eye_and_rejects_degenerate_frames() {
        let held = held_pose([5., 8., -10.], [0., 0., 1.], [0., 1., 0.], false).unwrap();
        assert_eq!(held.pose.position, [4.74, 7.84, -9.42]);
        assert_eq!(held.pose.front, [0., 0., -1.]);
        assert_eq!(held.scale, 1.15);
        let focus = held_pose([5., 8., -10.], [0., 0., 1.], [0., 1., 0.], true).unwrap();
        assert_eq!(focus.pose.position, [5., 7.99, -9.57]);
        assert_eq!(focus.scale, 1.8);
        assert!(held_pose([0.; 3], [0., 1., 0.], [0., 1., 0.], false).is_err());
        assert!(Pose::new([f64::NAN, 0., 0.], [1., 0., 0.], [0., 1., 0.]).is_err());
        let mut state = focused();
        state.apply(Command::Blur).unwrap();
        let invalid = Pose { front: [0., 0., -1.], ..pose([0.; 3]) };
        assert!(state.apply(interact([0.; 3], false, Some(invalid))).is_err());
        assert_eq!(state.location(), Location::Held);
    }

    #[test]
    fn phone_restore_keeps_continuous_placement_and_app_without_stale_focus_or_light() {
        let mut original = focused();
        tap(&mut original, 197., 295.);
        let mut restored = State::default();
        restored.restore(&original.snapshot()).unwrap();
        assert_eq!(restored.location(), Location::Held);
        assert_eq!(restored.screen(), Screen::Flashlight);
        assert!(!restored.focused());
        assert!(!restored.flashlight_on());
        let destination = pose([1.125, -2.375, 9.875]);
        original.apply(Command::Blur).unwrap();
        original.apply(interact([0.; 3], true, Some(destination))).unwrap();
        restored.restore(&original.snapshot()).unwrap();
        assert_eq!(restored.snapshot(), original.snapshot());
        let saved = restored.snapshot();
        let mut invalid = saved.clone();
        invalid["location"]["pose"]["front"] = json!([0., 0., -1.]);
        assert!(restored.restore(&invalid).is_err());
        assert_eq!(restored.snapshot(), saved);
        invalid = saved.clone();
        invalid["flashlight_on"] = json!(true);
        assert!(restored.restore(&invalid).is_err());
        assert_eq!(restored.snapshot(), saved);
    }
}

#[path = "Flashlight.rs"]
pub(crate) mod flashlight;

#[path = "Diagnostic.rs"]
pub(crate) mod diagnostic;
