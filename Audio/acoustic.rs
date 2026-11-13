//! Cardinal's listener-driven Fibonacci batches and transmission estimator.
use super::geometry::Geometry;
use super::*;
use std::collections::BTreeMap;
pub(crate) const RAYS: usize = 20;
#[derive(Clone, Copy)]
pub(crate) struct Segment {
    pub from: V,
    pub to: V,
    pub source: Option<u64>,
    pub kind: &'static str,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Source {
    pub id: u64,
    pub position: V,
    pub channel: usize,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Response {
    pub id: u64,
    pub channel: usize,
    pub source: V,
    pub gain: f64,
    pub pre_falloff: f64,
    pub occluded: bool,
    pub direction: V,
    pub pan: f64,
    pub hf_alpha: f64,
    pub echo_delay: f64,
    pub echo_gain: f64,
    pub escape: f64,
}
#[derive(Clone, Copy)]
pub(crate) struct Listener {
    pub position: V,
    pub forward: V,
    pub up: V,
}
#[derive(Default, Clone)]
struct Accum {
    green: usize,
    checks: usize,
    visible_wall: f64,
    visible_checks: usize,
    hidden_wall: f64,
    hidden_checks: usize,
    direction: V,
    directions: usize,
}
struct Ray {
    position: V,
    direction: V,
    bounce: usize,
    initialized: bool,
    finished: bool,
    escaped: bool,
    blue_sum: f64,
    blue_count: usize,
    blue_max: f64,
    last: Vec<Option<V>>,
}
struct Batch {
    listener: Listener,
    sources: Vec<Source>,
    rays: Vec<Ray>,
    accum: Vec<Accum>,
    next: usize,
    finished: usize,
    published: usize,
    escapes: usize,
    blue_sum: f64,
    blue_count: usize,
    blue_max: f64,
    revision: u64,
}
// Geometry is immutable for a revision. Reuse only exact visibility queries;
// never reduce the ray budget or suspend live game audio to improve DAW FPS.
#[derive(Default)]
struct VisibilityCache {
    revision: Option<u64>,
    entries: std::collections::HashMap<[u64;7],bool>,
}
struct VisibleGeometry<'a,G> { geometry:&'a G, cache:std::cell::RefCell<VisibilityCache> }
impl<G:Geometry> Geometry for VisibleGeometry<'_,G> {
    fn revision(&self)->u64 {self.geometry.revision()}
    fn trace(&self,o:V,d:V,n:f64)->Option<super::geometry::Hit> {self.geometry.trace(o,d,n)}
    fn wall_distance(&self,a:V,b:V,n:f64)->f64 {self.geometry.wall_distance(a,b,n)}
    fn visible(&self,a:V,b:V,n:f64)->bool {
        let key=[a[0].to_bits(),a[1].to_bits(),a[2].to_bits(),b[0].to_bits(),b[1].to_bits(),b[2].to_bits(),n.to_bits()];
        if let Some(value)=self.cache.borrow().entries.get(&key) {return *value;}
        let value=self.geometry.visible(a,b,n);
        let mut cache=self.cache.borrow_mut();
        if cache.entries.len()>=1024 {cache.entries.clear();}
        cache.entries.insert(key,value);
        value
    }
}
#[derive(Default)]
pub(crate) struct State {
    visibility: VisibilityCache,
    batch: Option<Batch>,
    last_complete: Option<f64>,
    pub responses: BTreeMap<u64, Response>,
    pub version: u64,
    pub escape: Option<f64>,
    pub debug: Vec<Segment>,
}
fn ratio(x: f64, n: usize) -> f64 {
    if n > 0 { x / n as f64 } else { 0. }
}
fn fibonacci(i: usize) -> V {
    let z = 1. - 2. * (i as f64 + 0.5) / RAYS as f64;
    let r = (1. - z * z).max(0.).sqrt();
    let phi = 2.39996323 * i as f64;
    [r * phi.cos(), r * phi.sin(), z]
}
impl Batch {
    fn new(listener: Listener, sources: &[Source], revision: u64) -> Self {
        Self {
            listener,
            sources: sources.to_vec(),
            rays: (0..RAYS)
                .map(|i| Ray {
                    position: listener.position,
                    direction: fibonacci(i),
                    bounce: 0,
                    initialized: false,
                    finished: false,
                    escaped: false,
                    blue_sum: 0.,
                    blue_count: 0,
                    blue_max: 0.,
                    last: vec![None; sources.len()],
                })
                .collect(),
            accum: vec![Accum::default(); sources.len()],
            next: 0,
            finished: 0,
            published: 0,
            escapes: 0,
            blue_sum: 0.,
            blue_count: 0,
            blue_max: 0.,
            revision,
        }
    }
    fn finish(&mut self, i: usize) {
        let ray = &mut self.rays[i];
        if ray.finished {
            return;
        }
        ray.finished = true;
        self.finished += 1;
        if ray.escaped {
            self.escapes += 1;
        } else if ray.blue_count > 0 {
            self.blue_sum += ray.blue_sum;
            self.blue_count += ray.blue_count;
            self.blue_max = self.blue_max.max(ray.blue_max);
        }
        for (s, dir) in ray.last.iter().enumerate() {
            if let Some(d) = dir {
                self.accum[s].direction = add(self.accum[s].direction, *d);
                self.accum[s].directions += 1;
            }
        }
    }
    fn step<G: Geometry>(&mut self, g: &G, debug: &mut Vec<Segment>, work_limit: usize) {
        let (mut work, mut attempts) = (0, 0);
        while work < work_limit && self.finished < RAYS && attempts < RAYS {
            let i = self.next;
            self.next = (self.next + 1) % RAYS;
            attempts += 1;
            if self.rays[i].finished {
                continue;
            }
            if self.rays[i].bounce >= 6 {
                self.finish(i);
                work += 1;
                continue;
            }
            let ray = &mut self.rays[i];
            if !ray.initialized {
                for (s, source) in self.sources.iter().enumerate() {
                    self.accum[s].checks += 1;
                    if g.visible(ray.position, source.position, 64.) {
                        self.accum[s].green += 1;
                    }
                }
                ray.initialized = true;
            }
            let Some(hit) = g.trace(ray.position, ray.direction, 64.) else {
                ray.escaped = true;
                self.finish(i);
                work += 1;
                continue;
            };
            if ray.bounce > 0 && debug.len() < 16384 {
                debug.push(Segment {
                    from: ray.position,
                    to: hit.position,
                    source: None,
                    kind: "reflection",
                });
            }
            let blue = g.visible(hit.position, self.listener.position, 64.);
            let blue_dir = unit(sub(hit.position, self.listener.position));
            if blue {
                let n = length(sub(hit.position, self.listener.position));
                ray.blue_sum += n;
                ray.blue_count += 1;
                ray.blue_max = ray.blue_max.max(n);
            }
            for (s, source) in self.sources.iter().enumerate() {
                let visible = g.visible(hit.position, source.position, 64.);
                let a = &mut self.accum[s];
                a.checks += 1;
                let wall = g.wall_distance(hit.position, source.position, 64.);
                if visible {
                    a.green += 1;
                    a.visible_wall += wall;
                    a.visible_checks += 1;
                    if blue {
                        ray.last[s] = Some(blue_dir)
                    }
                    if debug.len() < 16384 {
                        debug.push(Segment {
                            from: source.position,
                            to: hit.position,
                            source: Some(source.id),
                            kind: "visible",
                        });
                    }
                } else {
                    a.hidden_wall += wall;
                    a.hidden_checks += 1;
                    if debug.len() < 16384 {
                        let delta = sub(hit.position, source.position);
                        let distance = length(delta);
                        if let Some(first) =
                            g.trace(source.position, unit(delta), distance.min(64.))
                        {
                            debug.push(Segment {
                                from: source.position,
                                to: first.position,
                                source: Some(source.id),
                                kind: "blocked",
                            });
                        }
                    }
                }
            }
            ray.direction = unit(sub(
                ray.direction,
                mul(hit.normal, 2. * dot(ray.direction, hit.normal)),
            ));
            ray.position = add(hit.position, mul(hit.normal, 0.02));
            ray.bounce += 1;
            work += 1;
        }
    }
    fn publish<G: Geometry>(
        &self,
        g: &G,
        listener: Listener,
        previous: &BTreeMap<u64, Response>,
        final_update: bool,
        metres_per_unit: f64,
    ) -> BTreeMap<u64, Response> {
        let escape = ratio(self.escapes as f64, self.finished);
        let avg = ratio(self.blue_sum, self.blue_count);
        let echo_gain = if avg > 0. {
            (avg / (avg + 6.) * (1. - escape)).clamp(0., 1.)
        } else {
            0.
        };
        let right = unit(cross(listener.forward, listener.up));
        let up = unit(cross(right, listener.forward));
        self.sources
            .iter()
            .enumerate()
            .map(|(s, source)| {
                let a = &self.accum[s];
                let p = self.listener.position;
                let ear = [-0.6, 0.6]
                    .iter()
                    .filter(|offset| g.visible(add(p, mul(right, **offset)), source.position, 64.))
                    .count();
                let probe = [
                    right,
                    mul(right, -1.),
                    listener.forward,
                    mul(listener.forward, -1.),
                    up,
                    mul(up, -1.),
                ]
                .iter()
                .filter(|axis| g.visible(add(p, mul(**axis, 0.4)), source.position, 64.))
                .count() as f64
                    / 6.;
                let green = ratio(a.green as f64, a.checks);
                let open = green * (-0.05 * ratio(a.visible_wall, a.visible_checks)).exp();
                let leak =
                    (1. - green) * 0.5 * (-0.05 * ratio(a.hidden_wall, a.hidden_checks)).exp();
                let transmission = (probe + (1. - probe) * (open + leak)).clamp(0., 1.);
                let mut gain = transmission;
                if probe == 0. {
                    gain = gain.max((escape * 0.3).clamp(0., 0.3));
                }
                let average = if a.directions > 0 {
                    mul(a.direction, 1. / a.directions as f64)
                } else {
                    [0.; 3]
                };
                let mut direction = if length(average) > 1e-4 {
                    unit(average)
                } else {
                    [0.; 3]
                };
                let direct = sub(source.position, p);
                let use_direct = final_update && ear > 0 && length(direct) > 1e-4;
                let has_direction = use_direct || length(direction) > 1e-4;
                if use_direct {
                    direction = unit(direct);
                }
                if let Some(old) = previous.get(&source.id) {
                    if !final_update {
                        direction = old.direction;
                    } else if !use_direct {
                        let old_direction = if length(old.direction) < 1e-4 {
                            direction
                        } else {
                            old.direction
                        };
                        direction = if has_direction {
                            let mixed = add(mul(old_direction, 0.75), mul(direction, 0.25));
                            if length(mixed) > 1e-4 {
                                unit(mixed)
                            } else {
                                mixed
                            }
                        } else {
                            old_direction
                        };
                    }
                }
                (
                    source.id,
                    Response {
                        id: source.id,
                        channel: source.channel,
                        source: source.position,
                        gain,
                        pre_falloff: gain,
                        occluded: transmission < 0.2,
                        direction,
                        pan: 0.,
                        hf_alpha: 0.,
                        echo_delay: self.blue_max * metres_per_unit / 343.,
                        echo_gain,
                        escape,
                    },
                )
            })
            .collect()
    }
}
impl State {
    pub fn batch_active(&self) -> bool {
        self.batch.is_some()
    }
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }
    pub fn advance<G: Geometry>(
        &mut self,
        g: &G,
        listener: Listener,
        sources: &[Source],
        now: f64,
        metres_per_unit: f64,
    ) -> Result<(), String> {
        self.advance_budgeted(g, listener, sources, now, metres_per_unit, 6)
    }
    pub fn advance_budgeted<G: Geometry>(
        &mut self, g: &G, listener: Listener, sources: &[Source],
        now: f64, metres_per_unit: f64, work_limit: usize,
    ) -> Result<(), String> {
        let mut cache=std::mem::take(&mut self.visibility);
        if cache.revision!=Some(g.revision()) {
            cache.entries.clear(); cache.revision=Some(g.revision());
        }
        let geometry=VisibleGeometry {geometry:g,cache:std::cell::RefCell::new(cache)};
        let result=self.advance_cached(&geometry,listener,sources,now,metres_per_unit,work_limit);
        self.visibility=geometry.cache.into_inner();
        result
    }
    fn advance_cached<G:Geometry>(&mut self,g:&G,listener:Listener,sources:&[Source],now:f64,metres_per_unit:f64,work_limit:usize)->Result<(),String> {
        if sources.len() > 17
            || sources
                .iter()
                .any(|s| s.id == 0 || s.channel >= 17 || s.position.iter().any(|n| !n.is_finite()))
            || !now.is_finite()
            || !metres_per_unit.is_finite()
            || metres_per_unit <= 0.
            || [listener.position, listener.forward, listener.up]
                .iter()
                .flatten()
                .any(|n| !n.is_finite())
            || length(cross(listener.forward, listener.up)) < 1e-6
        {
            return Err("invalid acoustic frame".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        if sources.iter().any(|s| !ids.insert(s.id)) {
            return Err("duplicate acoustic source identity".into());
        }
        if sources.is_empty() {
            self.invalidate();
            return Ok(());
        }
        self.responses.retain(|id, _| ids.contains(id));
        let changed = self.batch.as_ref().is_some_and(|b| {
            b.revision != g.revision()
                || b.sources.len() != sources.len()
                || b.sources
                    .iter()
                    .zip(sources)
                    .any(|(a, b)| a.id != b.id || a.channel != b.channel)
        });
        if changed {
            self.batch = None;
            self.last_complete = None;
            self.debug.clear();
        }
        if self.batch.is_none() && self.last_complete.is_none_or(|t| now < t || now - t >= 0.4) {
            self.batch = Some(Batch::new(listener, sources, g.revision()));
            self.debug.clear();
        }
        if let Some(b) = self.batch.as_mut() {
            b.step(g, &mut self.debug, work_limit);
            if b.finished > 0 && (b.finished != b.published || b.finished == RAYS) {
                self.responses = b.publish(
                    g,
                    listener,
                    &self.responses,
                    b.finished == RAYS,
                    metres_per_unit,
                );
                self.version += 1;
                self.escape = Some(ratio(b.escapes as f64, b.finished));
                b.published = b.finished;
            }
            if b.finished == RAYS {
                self.last_complete = Some(now);
                self.batch = None;
            }
        }
        // Ray batches retain their sampled scene, but moving sources must not
        // inherit an old batch position during the 0.4-second publication gap.
        // Keep reflected directions until a current direct path is established.
        for source in sources {
            if let Some(response) = self.responses.get_mut(&source.id) {
                response.source = source.position;
                response.channel = source.channel;
                if g.visible(listener.position, source.position, 64.) {
                    response.direction = unit(sub(source.position, listener.position));
                }
            }
        }
        Ok(())
    }
}
/// Cardinal head response: refreshed every frame, separate from ray publication.
pub(crate) fn head_response(r: &mut Response, listener: Listener, rate: f64) {
    let distance = length(sub(r.source, listener.position));
    r.gain = (r.pre_falloff * (4. / distance.max(4.)).powf(1.5)).clamp(0., 1.);
    let right = unit(cross(listener.forward, listener.up));
    let direction_length = length(r.direction);
    let direction = if direction_length > 1e-4 {
        unit(r.direction)
    } else {
        [0.; 3]
    };
    let width = ((distance - 2.) / 5.).clamp(0., 1.);
    r.pan = dot(direction, right).clamp(-1., 1.) * width * width * (3. - 2. * width);
    // The original treats an absent direction as forward-facing (no head
    // shadow), rather than applying a side-facing low-pass filter.
    let dot_forward = if direction_length > 1e-4 {
        dot(direction, unit(listener.forward)).clamp(-1., 1.)
    } else {
        1.
    };
    let t = ((1. - dot_forward) * 0.5).clamp(0., 1.);
    let head = 18000. * (2000_f64 / 18000.).powf(t);
    let air = 18000. * (7500_f64 / 18000.).powf(((distance - 4.) / 56.).clamp(0., 1.));
    r.hf_alpha = if rate > 0. {
        (1. - (-2. * std::f64::consts::PI * head.min(air) / rate).exp()).clamp(0., 1.)
    } else {
        0.
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_visibility_reuse_invalidates_on_endpoints_and_geometry_revision() {
        struct Count { calls:std::cell::Cell<usize>, revision:u64 }
        impl Geometry for Count {
            fn revision(&self)->u64 {self.revision}
            fn trace(&self,_:V,_:V,_:f64)->Option<super::super::geometry::Hit> {None}
            fn wall_distance(&self,_:V,_:V,_:f64)->f64 {0.}
            fn visible(&self,a:V,b:V,_:f64)->bool {self.calls.set(self.calls.get()+1);a[0]<=b[0]}
        }
        let mut g=Count{calls:std::cell::Cell::new(0),revision:1};
        let mut state=State::default();
        let mut source=Source{id:1,channel:0,position:[1.,0.,0.]};
        // A completed response makes the current direct-path check run each
        // frame; work_limit=0 isolates that exact query from reflection work.
        state.responses.insert(1,Response{id:1,channel:0,source:source.position,gain:0.,pre_falloff:1.,occluded:false,direction:[0.;3],pan:0.,hf_alpha:0.,echo_delay:0.,echo_gain:0.,escape:0.});
        state.advance_budgeted(&g,listener(),&[source],0.,1.,0).unwrap();
        state.advance_budgeted(&g,listener(),&[source],0.01,1.,0).unwrap();
        assert_eq!(g.calls.get(),1);
        assert_eq!(state.responses[&1].direction,[1.,0.,0.]);
        source.position=[-1.,0.,0.];
        state.advance_budgeted(&g,listener(),&[source],0.02,1.,0).unwrap();
        assert_eq!(g.calls.get(),2);
        // Occlusion keeps the previously reflected direction, exactly as the
        // uncached algorithm does; moving endpoints must never reuse visibility.
        assert_eq!(state.responses[&1].direction,[1.,0.,0.]);
        g.revision=2;
        state.advance_budgeted(&g,listener(),&[source],0.03,1.,0).unwrap();
        assert_eq!(g.calls.get(),3);
        let mut moved=listener();moved.position=[-2.,0.,0.];
        state.advance_budgeted(&g,moved,&[source],0.04,1.,0).unwrap();
        assert_eq!(g.calls.get(),4);
    }
    struct Open;
    impl Geometry for Open {
        fn revision(&self) -> u64 {
            1
        }
        fn trace(&self, _: V, _: V, _: f64) -> Option<super::super::geometry::Hit> {
            None
        }
        fn wall_distance(&self, _: V, _: V, _: f64) -> f64 {
            0.
        }
    }
    fn listener() -> Listener {
        Listener {
            position: [0.; 3],
            forward: [0., 0., -1.],
            up: [0., 1., 0.],
        }
    }
    #[test]
    fn open_field_escapes_without_echo_and_preserves_source_identity() {
        let mut s = State::default();
        let source = Source {
            id: 9,
            position: [0., 0., -8.],
            channel: 12,
        };
        for i in 0..4 {
            s.advance(&Open, listener(), &[source], i as f64 / 60., 1.)
                .unwrap();
        }
        let r = s.responses[&9];
        assert_eq!(r.escape, 1.);
        assert_eq!(r.pre_falloff, 1.);
        assert_eq!(r.echo_gain, 0.);
        assert_eq!(r.channel, 12);
        assert_eq!(r.direction, [0., 0., -1.]);
        s.advance(&Open, listener(), &[], 0.1, 1.).unwrap();
        assert!(s.responses.is_empty());
    }
    #[test]
    fn distance_is_applied_once_and_head_turn_changes_pan() {
        let mut r = Response {
            id: 1,
            channel: 12,
            source: [8., 0., 0.],
            gain: 1.,
            pre_falloff: 1.,
            occluded: false,
            direction: [1., 0., 0.],
            pan: 0.,
            hf_alpha: 0.,
            echo_delay: 0.,
            echo_gain: 0.,
            escape: 1.,
        };
        head_response(&mut r, listener(), 48000.);
        let gain = r.gain;
        assert!((gain - 0.5f64.powf(1.5)).abs() < 1e-10);
        assert!((r.pan - 1.).abs() < 1e-10);
        head_response(&mut r, listener(), 48000.);
        assert_eq!(r.gain, gain);
        head_response(
            &mut r,
            Listener {
                forward: [1., 0., 0.],
                ..listener()
            },
            48000.,
        );
        assert!(r.pan.abs() < 1e-10);
    }
    #[test]
    fn moving_source_pose_updates_between_completed_ray_batches() {
        let mut s = State::default();
        let mut source = Source {
            id: 9,
            position: [0., 0., -8.],
            channel: 12,
        };
        for i in 0..4 {
            s.advance(&Open, listener(), &[source], i as f64 / 60., 1.)
                .unwrap();
        }
        let version = s.version;
        source.position = [8., 0., 0.];
        s.advance(&Open, listener(), &[source], 0.1, 1.).unwrap();
        assert_eq!(s.version, version);
        let mut response = s.responses[&9];
        assert_eq!(response.source, source.position);
        head_response(&mut response, listener(), 48000.);
        assert!((response.pan - 1.).abs() < 1e-10);
        assert!((response.gain - 0.5f64.powf(1.5)).abs() < 1e-10);
    }
}
