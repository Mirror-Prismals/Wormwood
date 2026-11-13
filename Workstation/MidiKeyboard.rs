//! Cardinal's actual computer-keyboard/preview sender. Only the application
//! thread calls this; the existing EnvironmentAudioSystem timestamps events.
use serde_json::{Value, json};
use std::collections::BTreeMap;
/// GLFW key codes paired with Cardinal's exact base pitches (A=C3).
pub(crate) const KEY_MAP: [(i32, u8); 22] = [
    (280, 47),
    (65, 48),
    (87, 49),
    (83, 50),
    (69, 51),
    (68, 52),
    (70, 53),
    (84, 54),
    (71, 55),
    (89, 56),
    (72, 57),
    (85, 58),
    (74, 59),
    (75, 60),
    (79, 61),
    (76, 62),
    (80, 63),
    (59, 64),
    (39, 65),
    (93, 66),
    (257, 67),
    (92, 68),
];
#[derive(Clone, Copy, Default)]
pub(crate) struct Mic {
    pub available: bool,
    pub peak: f64,
    pub rms: f64,
}
#[derive(Default)]
pub(crate) struct MicMeter {
    pub enabled: bool,
    peak: f64,
    rms: f64,
    blocks: u64,
    seen: bool,
}
impl MicMeter {
    pub fn set_enabled(&mut self, v: bool) {
        self.enabled = v;
        if !v {
            self.reset();
        }
    }
    fn reset(&mut self) {
        self.peak = 0.;
        self.rms = 0.;
        self.blocks = 0;
        self.seen = false;
    }
    pub fn update(&mut self, input: Option<&[f32]>, rate: u32) {
        let Some(input) = input.filter(|s| self.enabled && !s.is_empty() && rate > 0) else {
            self.reset();
            return;
        };
        let peak = input.iter().fold(0f64, |a, v| a.max(f64::from(*v).abs()));
        let sum = input.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let seconds = input.len() as f64 / f64::from(rate);
        self.peak = peak.max(self.peak * (-seconds / 0.055).exp());
        self.rms = (sum / input.len() as f64).sqrt();
        self.blocks = self.blocks.saturating_add(1);
        self.seen |= peak > 0.00001;
    }
    pub fn snapshot(&self) -> Value {
        json!({"enabled":self.enabled,"available":self.enabled&&self.blocks>0&&self.seen,"peak":self.peak,"rms":self.rms,"blocks":self.blocks})
    }
}
impl Mic {
    pub fn parse(v: &Value) -> Self {
        Self {
            available: v["available"] == true,
            peak: v["peak"].as_f64().filter(|n| n.is_finite()).unwrap_or(0.),
            rms: v["rms"].as_f64().filter(|n| n.is_finite()).unwrap_or(0.),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Velocity {
    pub mic_enabled: bool,
    pub fallback: f64,
    pub min: f64,
    pub max: f64,
    pub quiet_db: f64,
    pub loud_db: f64,
    pub noise_multiplier: f64,
    pub curve: f64,
}
impl Default for Velocity {
    fn default() -> Self {
        Self {
            mic_enabled: true,
            fallback: 0.80,
            min: 0.12,
            max: 1.,
            quiet_db: -48.,
            loud_db: -12.,
            noise_multiplier: 2.2,
            curve: 0.72,
        }
    }
}
impl Velocity {
    pub fn parse(v: &Value) -> Self {
        let number = |key: &str, fallback: f64| {
            v[key]
                .as_f64()
                .or_else(|| v[key].as_str().and_then(|s| s.parse().ok()))
                .filter(|n| n.is_finite())
                .unwrap_or(fallback)
        };
        let min = number("ComputerMidiKeyboardMicMinVelocity", 0.12).clamp(0.01, 1.);
        let quiet = number("ComputerMidiKeyboardMicQuietDb", -48.);
        Self {
            mic_enabled: v["ComputerMidiKeyboardMicVelocityEnabled"]
                .as_bool()
                .unwrap_or(true),
            fallback: number("ComputerMidiKeyboardFallbackVelocity", 0.80).clamp(0.01, 1.),
            min,
            max: number("ComputerMidiKeyboardMicMaxVelocity", 1.).clamp(min, 1.),
            quiet_db: quiet,
            loud_db: number("ComputerMidiKeyboardMicLoudDb", -12.).max(quiet + 1.),
            noise_multiplier: number("ComputerMidiKeyboardMicNoiseMultiplier", 2.2).max(1.),
            curve: number("ComputerMidiKeyboardMicCurve", 0.72).clamp(0.15, 4.),
        }
    }
    fn attack(self, mic: Mic, floor: f64) -> f64 {
        if !self.mic_enabled || !mic.available {
            return self.fallback;
        }
        let threshold = 10f64
            .powf(self.quiet_db / 20.)
            .max(floor * self.noise_multiplier);
        let loud = 10f64.powf(self.loud_db / 20.).max(threshold * 1.01);
        let normalized =
            ((mic.peak.max(threshold) / threshold).ln() / (loud / threshold).ln()).clamp(0., 1.);
        self.min + (self.max - self.min) * normalized.powf(self.curve)
    }
}
#[derive(Clone, Default)]
pub(crate) struct Keys {
    pub down: [bool; 22],
    pub octave: [bool; 2],
    pub focused: bool,
    pub cancelled: bool,
    pub modifier: bool,
}
impl Keys {
    pub fn parse(editor: &Value) -> Self {
        let v = &editor["computer_midi"];
        Self {
            down: std::array::from_fn(|i| v["keys"][i] == true),
            octave: std::array::from_fn(|i| v["octave"][i] == true),
            focused: editor["focused"] != false,
            cancelled: v["cancelled"] == true,
            modifier: editor["command"] == true || editor["alt"] == true,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub track: String,
    pub pitch: u8,
    pub velocity: f64,
    pub preview: bool,
}
impl Event {
    pub fn request(&self) -> Value {
        json!({"track":self.track,"pitch":self.pitch,"velocity":self.velocity,"source":if self.preview{"preview"}else{"keyboard"}})
    }
}
type Key = (String, u8, bool);
pub(crate) struct Sender {
    enabled: bool,
    octave: i8,
    was_down: [bool; 22],
    was_octave: [bool; 2],
    attack: [f64; 22],
    noise_floor: f64,
    remembered: Option<String>,
    desired: BTreeMap<Key, f64>,
    delivered: BTreeMap<Key, f64>,
    pub preview_enabled: bool,
}
impl Default for Sender {
    fn default() -> Self {
        Self {
            enabled: false,
            octave: 0,
            was_down: [false; 22],
            was_octave: [false; 2],
            attack: [0.; 22],
            noise_floor: 0.002,
            remembered: None,
            desired: BTreeMap::new(),
            delivered: BTreeMap::new(),
            preview_enabled: true,
        }
    }
}
impl Sender {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, v: bool) {
        self.enabled = v;
        if !v {
            self.desired.retain(|(_, _, preview), _| *preview);
            self.attack = [0.; 22];
        }
    }
    pub fn release_all(&mut self) {
        self.desired.clear();
        self.attack = [0.; 22];
    }
    pub fn status(&self) -> Value {
        json!({"enabled":self.enabled,"octave":self.octave,"selected_midi_track":self.remembered,"sounding_notes":self.delivered.len(),"pending_events":self.events().len(),"preview_enabled":self.preview_enabled})
    }
    pub fn target(
        &mut self,
        snapshot: &Value,
        selected: Option<&str>,
        piano: Option<&str>,
    ) -> Option<String> {
        let valid = |id: &str| {
            snapshot["tracks"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["id"] == id && t["kind"] == "midi"))
        };
        if let Some(id) = selected.filter(|id| valid(id)) {
            self.remembered = Some(id.into());
        }
        if self.remembered.as_deref().is_some_and(|id| !valid(id)) {
            self.remembered = None;
        }
        piano
            .filter(|id| valid(id))
            .map(str::to_owned)
            .or_else(|| self.remembered.clone())
    }
    pub fn update(
        &mut self,
        keys: &Keys,
        target: Option<&str>,
        blocked: bool,
        dt: f64,
        mic: Mic,
        velocity: Velocity,
        preview: Option<(&str, u8, f64)>,
    ) {
        self.desired.clear();
        let unavailable = blocked || !keys.focused || keys.cancelled;
        let keyboard_blocked = unavailable || !self.enabled || keys.modifier;
        if keyboard_blocked {
            self.was_down = keys.down;
            self.was_octave = keys.octave;
            self.attack = [0.; 22];
        } else {
            if keys.octave[0] && !self.was_octave[0] {
                self.octave = (self.octave - 1).max(-3);
            }
            if keys.octave[1] && !self.was_octave[1] {
                self.octave = (self.octave + 1).min(4);
            }
            self.was_octave = keys.octave;
            if let Some(target) = target {
                let pressed = keys
                    .down
                    .iter()
                    .zip(self.was_down)
                    .any(|(down, old)| *down && !old);
                if velocity.mic_enabled && mic.available && !pressed {
                    let target = mic.rms.clamp(0.00001, 0.35);
                    let rate = if target > self.noise_floor { 0.45 } else { 2.5 };
                    let blend = 1. - (-dt.max(0.) * rate).exp();
                    self.noise_floor += (target - self.noise_floor) * blend;
                }
                let attack = if pressed {
                    velocity.attack(mic, self.noise_floor)
                } else {
                    0.
                };
                for (i, (_, base)) in KEY_MAP.iter().enumerate() {
                    if keys.down[i] {
                        if !self.was_down[i] {
                            self.attack[i] = attack;
                        }
                        let pitch = (*base as i16 + self.octave as i16 * 12).clamp(0, 127) as u8;
                        let v = self
                            .desired
                            .entry((target.into(), pitch, false))
                            .or_insert(0f64);
                        *v = v.max(self.attack[i]);
                    } else {
                        self.attack[i] = 0.;
                    }
                    self.was_down[i] = keys.down[i];
                }
            }
        }
        if !unavailable && self.preview_enabled {
            if let Some((target, pitch, v)) = preview {
                if v.is_finite() && v > 0. {
                    self.desired
                        .insert((target.into(), pitch, true), v.clamp(1. / 127., 1.));
                }
            }
        }
        self.desired.retain(|_, v| *v > 0.);
    }
    /// Offs precede ons. Only acknowledge a successfully queued native event;
    /// failure leaves it pending so a release cannot be silently forgotten.
    pub fn events(&self) -> Vec<Event> {
        let mut events = Vec::new();
        for (key, value) in &self.delivered {
            if self.desired.get(key) != Some(value) {
                events.push(Event {
                    track: key.0.clone(),
                    pitch: key.1,
                    preview: key.2,
                    velocity: 0.,
                });
            }
        }
        for (key, value) in &self.desired {
            if self.delivered.get(key) != Some(value) {
                events.push(Event {
                    track: key.0.clone(),
                    pitch: key.1,
                    preview: key.2,
                    velocity: *value,
                });
            }
        }
        events
    }
    pub fn acknowledge(&mut self, e: &Event) {
        let key = (e.track.clone(), e.pitch, e.preview);
        if e.velocity > 0. {
            self.delivered.insert(key, e.velocity);
        } else {
            self.delivered.remove(&key);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn keys(index: usize) -> Keys {
        let mut k = Keys {
            focused: true,
            ..Default::default()
        };
        k.down[index] = true;
        k
    }
    fn send(s: &mut Sender) -> Vec<Event> {
        let e = s.events();
        for v in &e {
            s.acknowledge(v);
        }
        e
    }
    #[test]
    fn original_22_key_mapping_and_fallback_velocity() {
        assert_eq!(
            KEY_MAP.map(|v| v.1),
            [
                47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
                68
            ]
        );
        let mut s = Sender::default();
        s.set_enabled(true);
        s.update(
            &keys(1),
            Some("m"),
            false,
            0.016,
            Mic::default(),
            Velocity::default(),
            None,
        );
        let e = send(&mut s);
        assert_eq!((e[0].pitch, e[0].velocity), (48, 0.8));
        s.update(
            &keys(1),
            Some("m"),
            false,
            0.016,
            Mic {
                available: true,
                peak: 1.,
                rms: 1.,
            },
            Velocity::default(),
            None,
        );
        assert!(s.events().is_empty());
    }
    #[test]
    fn octave_switch_releases_then_retargets_held_note_and_is_clamped() {
        let mut s = Sender::default();
        s.set_enabled(true);
        let mut k = keys(1);
        s.update(
            &k,
            Some("a"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        send(&mut s);
        k.octave[1] = true;
        s.update(
            &k,
            Some("a"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        let e = send(&mut s);
        assert_eq!(
            e.iter().map(|e| (e.pitch, e.velocity)).collect::<Vec<_>>(),
            [(48, 0.), (60, 0.8)]
        );
        for _ in 0..10 {
            k.octave[1] = false;
            s.update(
                &k,
                Some("a"),
                false,
                0.,
                Mic::default(),
                Velocity::default(),
                None,
            );
            k.octave[1] = true;
            s.update(
                &k,
                Some("a"),
                false,
                0.,
                Mic::default(),
                Velocity::default(),
                None,
            );
        }
        assert_eq!(s.octave, 4);
    }
    #[test]
    fn focus_modifier_disable_and_target_changes_release_without_retrigger() {
        let mut s = Sender::default();
        s.set_enabled(true);
        let mut k = keys(1);
        s.update(
            &k,
            Some("a"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        send(&mut s);
        s.update(
            &k,
            Some("b"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        let e = send(&mut s);
        assert_eq!(
            (e[0].track.as_str(), e[0].velocity, e[1].track.as_str()),
            ("a", 0., "b")
        );
        k.modifier = true;
        s.update(
            &k,
            Some("b"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        assert_eq!(send(&mut s)[0].velocity, 0.);
        k.modifier = false;
        s.update(
            &k,
            Some("b"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        assert!(s.events().is_empty());
        k.down[1] = false;
        s.update(
            &k,
            Some("b"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        k.down[1] = true;
        s.update(
            &k,
            Some("b"),
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            None,
        );
        send(&mut s);
        s.set_enabled(false);
        assert_eq!(send(&mut s)[0].velocity, 0.);
    }
    #[test]
    fn release_retry_and_preview_are_independent_of_keyboard_enable() {
        let mut s = Sender::default();
        s.update(
            &Keys {
                focused: true,
                ..Default::default()
            },
            None,
            false,
            0.,
            Mic::default(),
            Velocity::default(),
            Some(("m", 60, 0.4)),
        );
        let on = send(&mut s);
        assert!(on[0].preview);
        s.release_all();
        let off = s.events();
        assert_eq!(s.events(), off);
        s.acknowledge(&off[0]);
        assert!(s.events().is_empty());
    }
    #[test]
    fn target_priority_uses_open_roll_then_selected_then_remembered() {
        let mut s = Sender::default();
        let doc = json!({"tracks":[{"id":"a","kind":"audio"},{"id":"m","kind":"midi"},{"id":"p","kind":"midi"}]});
        assert_eq!(s.target(&doc, Some("m"), Some("p")), Some("p".into()));
        assert_eq!(s.target(&doc, Some("a"), None), Some("m".into()));
    }
    #[test]
    fn mic_meter_and_attack_follow_original_55ms_envelope_and_bounds() {
        let mut m = MicMeter::default();
        m.update(Some(&[1., 1.]), 8000);
        assert_eq!(m.snapshot()["available"], false);
        m.set_enabled(true);
        m.update(Some(&[0.25; 80]), 8000);
        let first = Mic::parse(&m.snapshot());
        assert!(first.available);
        assert_eq!(first.peak, 0.25);
        m.update(Some(&[0.; 80]), 8000);
        assert!((Mic::parse(&m.snapshot()).peak - 0.25 * (-0.01f64 / 0.055).exp()).abs() < 1e-9);
        let v = Velocity::default();
        assert_eq!(
            v.attack(
                Mic {
                    available: true,
                    peak: 0.,
                    rms: 0.
                },
                0.002
            ),
            0.12
        );
        assert_eq!(
            v.attack(
                Mic {
                    available: true,
                    peak: 10.,
                    rms: 0.
                },
                0.002
            ),
            1.
        );
        m.set_enabled(false);
        assert_eq!(m.snapshot()["blocks"], 0);
    }
}
