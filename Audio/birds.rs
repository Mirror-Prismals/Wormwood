//! Cardinal's four-slot bird behavior, adapted to continuous tree perches/local up.
use super::*;
use serde_json::{Value, json};
pub(crate) const SCRIPTS: [&str; 4] = ["cardinal", "chickadee", "warbler", "wood_thrush"];
#[derive(Clone, Copy)]
pub(crate) struct Perch {
    pub site: u64,
    pub branch: u32,
    pub position: V,
    pub up: V,
}
pub(crate) fn perches(v: &Value) -> Result<Vec<Perch>, String> {
    let rows = v["perches"]
        .as_array()
        .ok_or("perch query result required")?;
    if rows.len() > 12 {
        return Err("bird perch budget exceeded".into());
    }
    rows.iter()
        .filter(|p| p["suitable"] == true)
        .map(|p| {
            Ok(Perch {
                site: p["site_id"].as_u64().ok_or("perch site required")?,
                branch: p["branch_id"]
                    .as_u64()
                    .filter(|n| *n <= u32::MAX as u64)
                    .ok_or("perch branch required")? as u32,
                position: vector(&p["position"])?,
                up: unit(vector(&p["up"])?),
            })
        })
        .collect()
}
#[derive(Clone, Copy)]
pub(crate) enum Action {
    Spawn(usize, V),
    Move(usize, V),
    Call(usize),
    Despawn(usize),
    FlightStarted(usize),
    FlightFinished(usize),
}
#[derive(Clone, Copy)]
struct Bird {
    position: V,
    perch: Perch,
    start: V,
    target: Perch,
    progress: f64,
    duration: f64,
    flying: bool,
    perch_seconds: f64,
    call_seconds: f64,
}
pub(crate) struct Flock {
    birds: [Option<Bird>; 4],
    seed: u32,
    scan: f64,
    up: V,
    pub revision: u64,
}
impl Default for Flock {
    fn default() -> Self {
        Self {
            birds: [None; 4],
            seed: 0x51a7b1d5,
            scan: 0.,
            up: [0., 1., 0.],
            revision: 0,
        }
    }
}
impl Flock {
    fn next_random(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = if x == 0 { 0x51a7b1d5 } else { x };
        self.seed
    }
    fn random(&mut self) -> f64 {
        (self.next_random() & 0xffffff) as f64 / 16777215.
    }
    pub fn set_up(&mut self, up: V) {
        self.up = unit(up);
    }
    fn player_distance(&self, position: V, player: V) -> f64 {
        let delta = sub(position, player);
        length(sub(delta, mul(self.up, dot(delta, self.up))))
    }
    pub fn needs_discovery(&self, player: V, dt: f64) -> bool {
        self.query_due()
            || self.birds.iter().flatten().any(|bird| {
                !bird.flying
                    && (bird.perch_seconds <= dt
                        || self.player_distance(bird.position, player) < 32.
                        || self.player_distance(bird.position, player) > 72.)
            })
    }
    pub fn query_due(&self) -> bool {
        self.scan <= 0.
    }
    pub fn query(&self, position: V, up: V) -> Value {
        json!({"position":position,"up":up,"min_distance":40.,"max_distance":60.,"limit":12,"seed":0x51a7b1d5u32})
    }
    pub fn tracking_query(&self, time: f64) -> Option<Value> {
        let mut keys = Vec::new();
        for b in self.birds.iter().flatten() {
            let p = if b.flying { b.target } else { b.perch };
            if !keys
                .iter()
                .any(|k: &Value| k["site_id"] == p.site && k["branch_id"] == p.branch)
            {
                keys.push(json!({"site_id":p.site,"branch_id":p.branch,"position":p.position}));
            }
        }
        (!keys.is_empty()).then(|| json!({"time":time,"keys":keys}))
    }
    pub fn refresh(&mut self, result: &Value) -> Result<Vec<Action>, String> {
        let current = perches(result)?;
        let revision = result["revision"]
            .as_u64()
            .ok_or("perch revision required")?;
        let removed = result["removed"]
            .as_array()
            .ok_or("removed perch keys required")?;
        if removed.len() > 12 {
            return Err("removed perch budget exceeded".into());
        }
        let removed: Vec<(u64, u32)> = removed
            .iter()
            .map(|key| {
                Ok((
                    key["site_id"]
                        .as_u64()
                        .ok_or("removed perch site required")?,
                    key["branch_id"]
                        .as_u64()
                        .filter(|id| *id <= u32::MAX as u64)
                        .ok_or("removed perch branch required")? as u32,
                ))
            })
            .collect::<Result<_, String>>()?;
        self.revision = revision;
        let mut actions = Vec::new();
        for (slot, entry) in self.birds.iter_mut().enumerate() {
            let Some(b) = entry.as_mut() else { continue };
            let key = if b.flying { b.target } else { b.perch };
            if removed.contains(&(key.site, key.branch)) {
                *entry = None;
                actions.push(Action::Despawn(slot));
                continue;
            }
            // Missing/unresolved keys are not evidence of habitat removal.
            if let Some(p) = current
                .iter()
                .find(|p| p.site == key.site && p.branch == key.branch)
            {
                if b.flying {
                    b.target = *p;
                } else {
                    b.perch = *p;
                    b.position = p.position;
                }
            }
        }
        Ok(actions)
    }
    pub fn sources(&self) -> Vec<(usize, V)> {
        self.birds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.map(|b| (i, b.position)))
            .collect()
    }
    pub fn clear(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        for (i, b) in self.birds.iter_mut().enumerate() {
            if b.take().is_some() {
                out.push(Action::Despawn(i));
            }
        }
        self.scan = 0.;
        out
    }
    pub fn step(
        &mut self,
        player: V,
        dt: f64,
        query: Option<&Value>,
    ) -> Result<Vec<Action>, String> {
        if !dt.is_finite() || !(0.0..=0.25).contains(&dt) {
            return Err("bird dt outside bounded frame interval".into());
        }
        self.scan -= dt;
        let mut actions = Vec::new();
        let mut candidates = Vec::new();
        if let Some(q) = query {
            candidates = perches(q)?;
            self.revision = q["revision"].as_u64().ok_or("perch revision required")?;
            let discover = self.scan <= 0.;
            if discover {
                self.scan = 2.;
            }
            for slot in 0..4 {
                if discover && slot < candidates.len() && self.birds[slot].is_none() {
                    let p = candidates[slot];
                    self.birds[slot] = Some(Bird {
                        position: p.position,
                        perch: p,
                        start: p.position,
                        target: p,
                        progress: 0.,
                        duration: 0.,
                        flying: false,
                        perch_seconds: 5. + self.random() * 7.,
                        call_seconds: 2. + self.random() * 6.,
                    });
                    actions.push(Action::Spawn(slot, p.position));
                }
            }
        }
        for slot in 0..4 {
            let Some(mut b) = self.birds[slot] else {
                continue;
            };
            let distance = self.player_distance(b.position, player);
            if b.flying {
                b.progress = (b.progress + dt / b.duration.max(0.01)).min(1.);
                let t = b.progress;
                let ease = t * t * (3. - 2. * t);
                b.position = add(
                    add(mul(b.start, 1. - ease), mul(b.target.position, ease)),
                    mul(
                        unit(add(b.perch.up, b.target.up)),
                        (t * std::f64::consts::PI).sin() * 3.,
                    ),
                );
                if t >= 1. {
                    b.position = b.target.position;
                    b.perch = b.target;
                    b.flying = false;
                    b.perch_seconds = 5. + self.random() * 7.;
                    actions.push(Action::FlightFinished(slot));
                }
            } else {
                b.perch_seconds -= dt;
                b.call_seconds -= dt;
                if b.call_seconds <= 0. {
                    actions.push(Action::Call(slot));
                    b.call_seconds = 2. + self.random() * 6.;
                }
                if b.perch_seconds <= 0. || distance < 32. || distance > 72. {
                    if !candidates.is_empty() {
                        let start = self.next_random() as usize % candidates.len();
                        let target = (0..candidates.len())
                            .map(|i| candidates[(start + i) % candidates.len()])
                            .find(|p| self.player_distance(p.position, b.position) >= 8.)
                            .unwrap_or(candidates[start]);
                        b.start = b.position;
                        b.target = target;
                        b.progress = 0.;
                        b.duration = (length(sub(target.position, b.position)) / 8.).max(0.5);
                        b.flying = true;
                        actions.push(Action::FlightStarted(slot));
                    } else if distance > 90. {
                        self.birds[slot] = None;
                        actions.push(Action::Despawn(slot));
                        continue;
                    } else {
                        b.perch_seconds = 1.;
                        self.scan = 0.;
                    }
                }
            }
            actions.push(Action::Move(slot, b.position));
            self.birds[slot] = Some(b);
        }
        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_habitat_creates_no_birds_and_respects_scan_interval() {
        let mut f = Flock::default();
        let q = json!({"revision":1,"perches":[]});
        assert!(f.step([0.; 3], 0.016, Some(&q)).unwrap().is_empty());
        assert!(!f.query_due());
        assert!(f.sources().is_empty());
    }
    #[test]
    fn finite_calls_use_four_stable_slots_and_clear_on_confirmed_habitat_removal() {
        let mut f = Flock::default();
        let rows:Vec<_>=(0..4).map(|i|json!({"site_id":i+1,"branch_id":0,"position":[40+i*4,0,0],"up":[0,1,0],"suitable":true})).collect();
        let q = json!({"revision":1,"perches":rows});
        let actions = f.step([0.; 3], 0.016, Some(&q)).unwrap();
        assert_eq!(
            actions
                .iter()
                .filter(|a| matches!(a, Action::Spawn(..)))
                .count(),
            4
        );
        assert_eq!(f.sources().len(), 4);
        let removed: Vec<_> = (0..4)
            .map(|i| json!({"site_id":i+1,"branch_id":0}))
            .collect();
        let empty = json!({"revision":2,"perches":[],"removed":removed});
        assert_eq!(f.refresh(&empty).unwrap().len(), 4);
        assert!(f.sources().is_empty());
    }
    fn habitat(x: f64) -> Value {
        json!({"revision":1,"perches":[{"site_id":1,"branch_id":7,
            "position":[x,0.,0.],"up":[0.,1.,0.],"suitable":true}],"removed":[],"unresolved":[]})
    }
    #[test]
    fn perched_bird_tracks_wind_between_population_scans() {
        let mut f = Flock::default();
        f.step([0.; 3], 0.016, Some(&habitat(45.))).unwrap();
        assert!(!f.query_due());
        let actions = f.refresh(&habitat(45.25)).unwrap();
        f.step([0.; 3], 0.016, None).unwrap();
        assert_eq!(f.sources()[0].1, [45.25, 0., 0.]);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, Action::Spawn(..) | Action::Despawn(..)))
        );
        // Random call/flight scheduling must not change habitat ranking.
        let seed = f.query([0.; 3], [0., 1., 0.])["seed"].clone();
        f.random();
        assert_eq!(f.query([0.; 3], [0., 1., 0.])["seed"], seed);
    }
    #[test]
    fn moving_destination_is_followed_and_removed_destination_cancels_bird() {
        let mut f = Flock::default();
        f.step([0.; 3], 0.016, Some(&habitat(45.))).unwrap();
        let b = f.birds[0].as_mut().unwrap();
        b.flying = true;
        b.duration = 1.;
        f.refresh(&habitat(46.)).unwrap();
        f.step([0.; 3], 0.25, None).unwrap();
        assert_eq!(f.birds[0].unwrap().target.position, [46., 0., 0.]);
        let removed = json!({"revision":2,"perches":[],"removed":[{"site_id":1,"branch_id":7}],"unresolved":[]});
        let actions = f.refresh(&removed).unwrap();
        assert!(actions.iter().any(|a| matches!(a, Action::Despawn(0))));
        assert!(f.sources().is_empty());
    }
    #[test]
    fn unresolved_keys_and_empty_discovery_do_not_remove_tracked_birds() {
        let mut f = Flock::default();
        f.step([0.; 3], 0.016, Some(&habitat(45.))).unwrap();
        let result = json!({"revision":2,"perches":[],"removed":[],"unresolved":[{"site_id":1,"branch_id":7}]});
        assert!(f.refresh(&result).unwrap().is_empty());
        f.step([0.; 3], 0.016, Some(&json!({"revision":2,"perches":[]})))
            .unwrap();
        assert_eq!(f.sources().len(), 1);
        let tracking = f.tracking_query(12.).unwrap();
        assert_eq!(tracking["keys"][0]["position"], json!([45., 0., 0.]));
        assert_eq!(tracking["time"], 12.);
    }
}
