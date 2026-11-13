//! Cardinal panel slides, expansion and Origami corner-ownership state.
//! Source: Salamander137/BaseSystem/PanelSystem.cpp, updateOrigamiOrder and
//! UpdatePanels; Host/DawSidePanelLayout.h, ToggleExpanded. This control-thread
//! state drives the actual parametric gem bounds in PanelLayout.rs.

const STATE_SPEED: f64 = 4.0;
const TAP_SECONDS: f64 = 0.5;
const TAP_RETRACTION: f64 = 0.1;
const EXPANDED_SCALE: f64 = 1.5;

/// The approved comparison used these exact original-integrator samples.
/// Parameter 0 is open, 31 is the captured closed endpoint (not mathematical 0).
const CANONICAL_LEFT_LAST: usize = 31;
const CANONICAL_LEFT_RATE: f64 = 31.0;
#[derive(Clone, Debug)]
struct CanonicalLeft {
    phase: Option<f64>,
    context_supported: bool,
    suspended: bool,
}
fn canonical_left_extension(index: usize) -> f64 {
    let mut extension = 1.0f64;
    let mut tap_age = 0.0f64;
    for _ in 0..index.min(CANONICAL_LEFT_LAST) {
        tap_age = (tap_age + 0.05).min(TAP_SECONDS);
        extension = (extension + (0.0 - extension) * 0.05 * STATE_SPEED).clamp(0.0, 1.0);
    }
    let phase = (tap_age / TAP_SECONDS).clamp(0.0, 1.0);
    extension * (1.0 - TAP_RETRACTION * (1.0 - (2.0 * phase - 1.0).abs()))
}
fn canonical_left_nearest(extension: f64) -> f64 {
    (0..=CANONICAL_LEFT_LAST).min_by(|a,b| {
        (canonical_left_extension(*a)-extension).abs()
            .total_cmp(&(canonical_left_extension(*b)-extension).abs())
    }).unwrap_or(0) as f64
}

/// Order matches Cardinal's panel state and workstation extension controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    Top = 0,
    Bottom = 1,
    Left = 2,
    Right = 3,
}

impl Edge {
    pub(crate) const ALL: [Self; 4] = [Self::Top, Self::Bottom, Self::Left, Self::Right];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|edge| edge.name() == name)
    }
}

/// Mobile has one visible full-screen workspace; None on State/Snapshot is desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MobilePage { Timeline, Panel(Edge) }
impl MobilePage {
    pub(crate) fn name(self) -> &'static str { match self {Self::Timeline=>"timeline",Self::Panel(edge)=>edge.name()} }
    pub(crate) fn parse(name: &str) -> Option<Self> { if name=="timeline" {Some(Self::Timeline)} else {Edge::parse(name).map(Self::Panel)} }
}

#[derive(Clone, Copy, Debug)]
struct Slide {
    open: bool,
    extension: f64,
    tap_age: f64,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            open: false,
            extension: 0.0,
            tap_age: TAP_SECONDS,
        }
    }
}

/// Geometry consumes visual extensions, while corner ownership follows the
/// opening targets. Ownership survives the closing animation as in Cardinal.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Snapshot {
    pub preamp_split: f64,
    pub mobile_page: Option<MobilePage>,
    pub mobile_progress: f64,
    pub fullscreen: [f64; 4],
    pub extensions: [f64; 4],
    pub open: [bool; 4],
    pub opening_order: [u64; 4],
    pub left_width_scale: f64,
    pub bottom_height_scale: f64,
    pub overview_hide: [f64; 4],
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            preamp_split: 0.,
            mobile_page: None,
            mobile_progress: 1.,
            fullscreen: [0.; 4],
            extensions: [0.0; 4],
            open: [false; 4],
            opening_order: [0; 4],
            left_width_scale: 1.0,
            bottom_height_scale: 1.0,
            overview_hide: [0.0; 4],
        }
    }
}

impl Snapshot {
    pub(crate) fn panel_visible(&self, edge: Edge) -> bool { self.mobile_page.is_none_or(|page|page==MobilePage::Panel(edge)) && !self.fullscreen.iter().enumerate().any(|(i,p)|i!=edge as usize && *p>=0.75-1.0e-6) }
    pub(crate) fn timeline_visible(&self) -> bool { self.mobile_page.is_none_or(|page|page==MobilePage::Timeline) && self.fullscreen.iter().all(|p|*p==0.) }
}

#[derive(Clone, Copy, Debug)]
struct MobileSlide { phase:f64, closing:bool }
#[derive(Clone, Debug)]
pub(crate) struct State {
    preamp_split: f64,
    preamp_open: bool,
    mobile_page: Option<MobilePage>,
    mobile_slide: Option<MobileSlide>,
    fullscreen_target: Option<Edge>,
    fullscreen_extent: f64,
    fullscreen: [f64; 4],
    fullscreen_start: [f64;4],
    fullscreen_elapsed: f64,
    slides: [Slide; 4],
    was_open: [bool; 4],
    opening_order: [u64; 4],
    sequence: u64,
    expanded: [bool; 2],
    size_scale: [f64; 2],
    overview_hidden: [bool; 4],
    overview_hide: [f64; 4],
    canonical_left: Option<CanonicalLeft>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            preamp_split: 0., preamp_open: false,
            mobile_page: None,
            mobile_slide: None,
            fullscreen_target: None,
            fullscreen_extent: 0.,
            fullscreen: [0.; 4],
            fullscreen_start: [0.;4],
            fullscreen_elapsed: 0.30,
            slides: [Slide::default(); 4],
            was_open: [false; 4],
            opening_order: [0; 4],
            sequence: 0,
            expanded: [false; 2],
            size_scale: [1.0; 2],
            overview_hidden: [false; 4],
            overview_hide: [0.0; 4],
            canonical_left: None,
        }
    }
}

impl State {
    pub(crate) fn set_preamp(&mut self, open: bool) { self.preamp_open = open; }
    pub(crate) fn fullscreen_target(&self) -> Option<Edge> { self.fullscreen_target }
    pub(crate) fn panel_view(&self) -> &'static str {
        if self.fullscreen_target.is_none() { "normal" }
        else if self.fullscreen_extent < 1. { "extended" } else { "fullscreen" }
    }
    fn set_panel_extent(&mut self, edge: Edge, extent: f64) -> bool {
        if self.mobile_page.is_some() { return false; }
        let target = if extent > 0. { Some(edge) } else { None };
        let changed = target != self.fullscreen_target || extent != self.fullscreen_extent;
        if changed { self.fullscreen_start = self.fullscreen; self.fullscreen_elapsed = 0.; }
        self.fullscreen_target = target;
        self.fullscreen_extent = extent;
        if extent > 0. { self.set_open(edge, true); }
        changed
    }
    pub(crate) fn cycle_panel_view(&mut self, edge: Edge) -> bool {
        let extent = if self.fullscreen_target != Some(edge) { 0.75 }
            else if self.fullscreen_extent < 1. { 1. } else { 0. };
        self.set_panel_extent(edge, extent)
    }
    pub(crate) fn set_fullscreen(&mut self, edge: Edge, enabled: bool) -> bool {
        if !enabled && self.fullscreen_target != Some(edge) { return false; }
        self.set_panel_extent(edge, if enabled { 1. } else { 0. })
    }
    pub(crate) fn toggle_fullscreen(&mut self, edge: Edge) -> bool {
        self.set_fullscreen(edge,self.fullscreen_target!=Some(edge))
    }
    pub(crate) fn fullscreen_animating(&self) -> bool {
        self.fullscreen.iter().enumerate().any(|(i,p)|
            (*p-if self.fullscreen_target==Some(Edge::ALL[i]){self.fullscreen_extent}else{0.}).abs()>1.0e-6)
    }

    pub(crate) fn mobile_page(&self) -> Option<MobilePage> { self.mobile_page }
    pub(crate) fn set_mobile_page(&mut self, page: Option<MobilePage>) -> bool {
        let changed=self.mobile_page!=page;self.mobile_page=page;self.mobile_slide=None;changed
    }

    /// Mobile forms a cross, not a carousel. Close must complete at Timeline
    /// before a different panel can be opened. Opposite input may reverse an
    /// opening slide continuously; repeated/orthogonal swipes do nothing.
    pub(crate) fn mobile_swipe(&mut self, direction:Edge)->bool {
        match self.mobile_page {
            Some(MobilePage::Timeline)=>{
                self.mobile_page=Some(MobilePage::Panel(direction));
                self.mobile_slide=Some(MobileSlide{phase:0.,closing:false});true
            },
            Some(MobilePage::Panel(edge))=>{
                let opposite=match edge{Edge::Top=>Edge::Bottom,Edge::Bottom=>Edge::Top,Edge::Left=>Edge::Right,Edge::Right=>Edge::Left};
                if direction!=opposite || self.mobile_slide.is_some_and(|s|s.closing){return false;}
                self.mobile_slide=Some(MobileSlide{phase:self.mobile_slide.map_or(1.,|s|s.phase),closing:true});true
            },None=>false,
        }
    }
    pub(crate) fn mobile_animating(&self)->bool {self.mobile_slide.is_some()}
    fn mobile_progress(&self)->f64 {
        let phase=self.mobile_slide.map_or(1.,|s|s.phase);phase*phase*(3.-2.*phase)
    }

    /// Explicit candidate opt-in. Does not alter targets or opening ownership.
    pub(crate) fn enable_canonical_left(&mut self, enabled: bool) {
        if enabled {
            if self.canonical_left.is_none() {
                self.canonical_left = Some(CanonicalLeft {phase: None, context_supported: true, suspended:false});
            }
        } else {
            self.canonical_left = None;
        }
    }
    /// Caller must disable admission in overview/unsupported layout contexts.
    /// The ordinary integrator then runs for the rest of this candidate session.
    /// Do not snap back from an arbitrary expanded/overview pose into the bank.
    pub(crate) fn set_canonical_left_context(&mut self, supported: bool) {
        if let Some(left) = &mut self.canonical_left {
            left.context_supported = supported;
            if !supported { left.phase = None; left.suspended=true; }
        }
    }
    pub(crate) fn canonical_left_phase(&self) -> Option<f64> {
        self.canonical_left.as_ref().and_then(|left| left.phase)
    }
    fn canonical_left_supported(&self) -> bool {
        self.fullscreen_target.is_none() && self.fullscreen.iter().all(|p|*p==0.) && self.mobile_page.is_none() && self.canonical_left.as_ref().is_some_and(|left| left.context_supported && !left.suspended)
            && !self.expanded[0] && (self.size_scale[0]-1.0).abs() <= 1.0e-6
            && self.overview_hidden.iter().all(|hidden| !hidden)
            && self.overview_hide.iter().all(|amount| *amount == 0.0)
    }
    pub(crate) fn is_open(&self, edge: Edge) -> bool {
        self.slides[edge as usize].open
    }

    /// Returns whether the target changed. Repeated held input must not restart
    /// the tap animation. Reversals retain the current slide progress.
    pub(crate) fn set_open(&mut self, edge: Edge, open: bool) -> bool {
        if !open && self.fullscreen_target==Some(edge) {self.set_fullscreen(edge,false);}
        let slide = &mut self.slides[edge as usize];
        if slide.open == open {
            return false;
        }
        slide.open = open;
        slide.tap_age = 0.0;
        self.overview_hidden[edge as usize] = false;
        true
    }

    pub(crate) fn toggle(&mut self, edge: Edge) {
        self.set_open(edge, !self.is_open(edge));
    }

    pub(crate) fn set_all_open(&mut self, open: bool) {
        for edge in Edge::ALL {
            self.set_open(edge, open);
        }
    }

    /// Only Cardinal's browser (left) and piano roll (bottom) have a second
    /// size. A closed panel opens expanded; an open panel toggles size.
    pub(crate) fn toggle_expanded(&mut self, edge: Edge) -> bool {
        let Some(index) = expansion_index(edge) else {
            return false;
        };
        let expanded = !self.is_open(edge) || !self.expanded[index];
        self.set_expanded(edge, expanded)
    }

    pub(crate) fn set_expanded(&mut self, edge: Edge, expanded: bool) -> bool {
        let Some(index) = expansion_index(edge) else {
            return false;
        };
        let changed = self.expanded[index] != expanded;
        self.expanded[index] = expanded;
        // Explicit size operations use Cardinal's open-and-expand behavior.
        let opened = self.set_open(edge, true);
        if changed && !opened {
            self.slides[edge as usize].tap_age = 0.0;
        }
        changed || opened
    }

    pub(crate) fn is_expanded(&self, edge: Edge) -> bool {
        expansion_index(edge).is_some_and(|index| self.expanded[index])
    }

    /// Overview has a third, fully outside parking position. Returning to
    /// native zoom calls this with false, preserving normal open preferences.
    pub(crate) fn set_overview_hidden(&mut self, edge: Edge, hidden: bool) -> bool {
        let state = &mut self.overview_hidden[edge as usize];
        let changed = *state != hidden;
        *state = hidden;
        changed
    }

    pub(crate) fn is_overview_hidden(&self, edge: Edge) -> bool {
        self.overview_hidden[edge as usize]
    }

    fn update_opening_order(&mut self) {
        // Cardinal deliberately assigns simultaneous openings side priority.
        // Reconcile once per frame so event delivery order cannot change it.
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let index = edge as usize;
            let slide = self.slides[index];
            if slide.open && !self.was_open[index] {
                // Sequence is only relative. Renormalize instead of wrapping
                // and accidentally granting a new panel oldest ownership.
                if self.sequence == u64::MAX {
                    let old = self.opening_order;
                    for (i, order) in self.opening_order.iter_mut().enumerate() {
                        *order = if old[i] == 0 {
                            0
                        } else {
                            1 + old.iter().filter(|n| **n > 0 && **n < old[i]).count() as u64
                        };
                    }
                    self.sequence = 4;
                }
                self.sequence += 1;
                self.opening_order[index] = self.sequence;
            } else if !slide.open && slide.extension < 0.001 {
                self.opening_order[index] = 0;
            }
            self.was_open[index] = slide.open;
        }
    }

    /// Preserves the source's per-frame first-order slide and triangular tap
    /// retraction, including its clamping on long frames. Invalid elapsed time
    /// leaves the state untouched instead of poisoning future transforms.
    pub(crate) fn advance(&mut self, dt_seconds: f64) {
        if !dt_seconds.is_finite() || dt_seconds < 0.0 {
            return;
        }
        let target=if self.preamp_open {1.}else{0.};
        self.preamp_split += (target-self.preamp_split)*(1.-(-dt_seconds.min(0.05)*18.).exp());
        if (target-self.preamp_split).abs()<0.0001 {self.preamp_split=target;}
        if self.mobile_page.is_some() {
            if let Some(slide)=&mut self.mobile_slide {
                // Keep several visible positions even when an optical frame
                // takes longer than the nominal 280ms transition. Audio and
                // other controller clocks continue using the original dt.
                let step=dt_seconds.min(0.05)/0.28;
                slide.phase=(slide.phase+if slide.closing{-step}else{step}).clamp(0.,1.);
                if (slide.closing&&slide.phase==0.) || (!slide.closing&&slide.phase==1.) {
                    if slide.closing {self.mobile_page=Some(MobilePage::Timeline);}
                    self.mobile_slide=None;
                }
            }
            return;
        }
        let canonical_phase = if self.canonical_left_supported() {
            Some(self.canonical_left_phase()
                .unwrap_or_else(|| canonical_left_nearest(self.effective_extension(Edge::Left))))
        } else {
            if let Some(left) = &mut self.canonical_left { left.phase = None; left.suspended=true; }
            None
        };
        self.fullscreen_elapsed=(self.fullscreen_elapsed+dt_seconds).min(0.30);
        let phase=self.fullscreen_elapsed/0.30;
        let ease=phase*phase*(3.-2.*phase);
        self.fullscreen=Edge::ALL.map(|edge| {
            let start=self.fullscreen_start[edge as usize];
            let target=if self.fullscreen_target==Some(edge){self.fullscreen_extent}else{0.};
            start+(target-start)*ease
        });
        self.update_opening_order();
        for slide in &mut self.slides {
            slide.tap_age = (slide.tap_age + dt_seconds).min(TAP_SECONDS);
            let target = if slide.open { 1.0 } else { 0.0 };
            slide.extension = (slide.extension
                + (target - slide.extension) * dt_seconds * STATE_SPEED)
                .clamp(0.0, 1.0);
        }
        if let Some(phase) = canonical_phase {
            let left = &mut self.slides[Edge::Left as usize];
            let direction = if left.open { -1.0 } else { 1.0 };
            let phase = (phase + direction * dt_seconds * CANONICAL_LEFT_RATE)
                .clamp(0.0, CANONICAL_LEFT_LAST as f64);
            // Sample already includes the original triangular tap modulation.
            left.extension = canonical_left_extension(phase.round() as usize);
            left.tap_age = TAP_SECONDS;
            self.canonical_left.as_mut().unwrap().phase = Some(phase);
        }
        for (index, scale) in self.size_scale.iter_mut().enumerate() {
            let target = if self.expanded[index] {
                EXPANDED_SCALE
            } else {
                1.0
            };
            *scale =
                (*scale + (target - *scale) * dt_seconds * STATE_SPEED).clamp(1.0, EXPANDED_SCALE);
        }
        let overview_ease = 1.0 - (-dt_seconds * STATE_SPEED).exp();
        for (index, state) in self.overview_hide.iter_mut().enumerate() {
            let target = if self.overview_hidden[index] {
                1.0
            } else {
                0.0
            };
            *state += (target - *state) * overview_ease;
            if (*state - target).abs() < 0.0005 {
                *state = target;
            }
            *state = state.clamp(0.0, 1.0);
        }
        self.update_opening_order();
    }

    /// Unmodulated slide state, useful for interaction state and inspection.
    pub(crate) fn extension(&self, edge: Edge) -> f64 {
        self.slides[edge as usize].extension
    }

    /// Visual extension for the 3D transform: zero is fully parked and one is
    /// fully open. A manual change briefly retracts up to ten percent, peaking
    /// after .25 seconds and ending after .5 seconds, as in Cardinal.
    pub(crate) fn effective_extension(&self, edge: Edge) -> f64 {
        let slide = self.slides[edge as usize];
        let phase = (slide.tap_age / TAP_SECONDS).clamp(0.0, 1.0);
        let wiggle = TAP_RETRACTION * (1.0 - (2.0 * phase - 1.0).abs());
        slide.extension * (1.0 - wiggle)
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            preamp_split: self.preamp_split,
            fullscreen: self.fullscreen,
            mobile_page: self.mobile_page,
            mobile_progress: self.mobile_progress(),
            extensions: Edge::ALL.map(|edge| self.effective_extension(edge)),
            open: Edge::ALL.map(|edge| self.is_open(edge)),
            opening_order: self.opening_order,
            left_width_scale: self.size_scale[0],
            bottom_height_scale: self.size_scale[1],
            overview_hide: self.overview_hide,
        }
    }

    /// The first-order source motion approaches its target asymptotically.
    /// A millionth of full travel is the presentation's settled tolerance.
    pub(crate) fn settled(&self) -> bool {
        if self.mobile_page.is_some(){return self.mobile_slide.is_none();}
        self.preamp_split == (if self.preamp_open {1.}else{0.}) && !self.fullscreen_animating() && self.slides.iter().enumerate().all(|(index,slide)| {
            if index == Edge::Left as usize && self.canonical_left_supported() {
                if let Some(phase) = self.canonical_left_phase() {
                    return phase == if slide.open { 0.0 } else { CANONICAL_LEFT_LAST as f64 };
                }
            }
            let target = if slide.open { 1.0 } else { 0.0 };
            slide.tap_age >= TAP_SECONDS && (slide.extension - target).abs() <= 1.0e-6
        }) && self.size_scale.iter().enumerate().all(|(index, scale)| {
            let target = if self.expanded[index] {
                EXPANDED_SCALE
            } else {
                1.0
            };
            (*scale - target).abs() <= 1.0e-6
        }) && self.overview_hide.iter().enumerate().all(|(index, state)| {
            let target = if self.overview_hidden[index] {
                1.0
            } else {
                0.0
            };
            (*state - target).abs() <= 1.0e-6
        })
    }
}

fn expansion_index(edge: Edge) -> Option<usize> {
    match edge {
        Edge::Left => Some(0),
        Edge::Bottom => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstation_origami_simultaneous_opening_has_cardinal_side_priority() {
        let mut motion = State::default();
        motion.set_all_open(true);
        motion.advance(0.25);
        assert_eq!(motion.snapshot().opening_order, [3, 4, 1, 2]);
        assert_eq!(motion.snapshot().extensions, [0.9; 4]);
        motion.advance(0.25);
        assert_eq!(motion.snapshot().extensions, [1.0; 4]);
    }

    #[test]
    fn workstation_origami_retains_corner_until_closed_and_reopening_gets_new_order() {
        let mut motion = State::default();
        motion.set_open(Edge::Bottom, true);
        motion.advance(0.5);
        motion.set_open(Edge::Left, true);
        motion.advance(0.5);
        assert_eq!(motion.snapshot().opening_order, [0, 1, 2, 0]);
        motion.set_open(Edge::Bottom, false);
        motion.advance(0.01);
        assert_eq!(motion.snapshot().opening_order[1], 1);
        motion.set_open(Edge::Bottom, true);
        motion.advance(0.01);
        assert_eq!(motion.snapshot().opening_order, [0, 3, 2, 0]);
        motion.set_open(Edge::Bottom, false);
        motion.advance(0.5);
        assert_eq!(motion.snapshot().opening_order[1], 0);
    }

    #[test]
    fn workstation_origami_expansion_uses_source_easing_independent_of_extension() {
        let mut motion = State::default();
        assert!(motion.toggle_expanded(Edge::Left));
        motion.advance(0.125);
        assert!(motion.is_open(Edge::Left));
        assert_eq!(motion.extension(Edge::Left), 0.5);
        assert_eq!(motion.snapshot().left_width_scale, 1.25);
        motion.set_open(Edge::Left, false);
        motion.advance(0.5);
        assert!(motion.is_expanded(Edge::Left));
        assert_eq!(motion.snapshot().left_width_scale, 1.5);
        assert!(!motion.toggle_expanded(Edge::Top));
    }
}

#[cfg(test)]
mod canonical_candidate_tests {
    use super::*;
    fn ready() -> State {
        let mut state=State::default(); state.set_all_open(true); state.advance(1.0);
        state.enable_canonical_left(true); state
    }
    #[test] fn canonical_samples_match_original_closing_bits() {
        for index in 0..=CANONICAL_LEFT_LAST {
            let mut original=State::default(); original.set_all_open(true); original.advance(1.0);
            original.set_open(Edge::Left,false);
            for _ in 0..index {original.advance(0.05);}
            original.advance(0.0);
            assert_eq!(canonical_left_extension(index).to_bits(),original.effective_extension(Edge::Left).to_bits());
        }
    }
    #[test] fn real_targets_reverse_continuously_without_restart() {
        let mut state=ready();state.set_open(Edge::Left,false);state.advance(0.4);
        let phase=state.canonical_left_phase().unwrap();let visible=state.effective_extension(Edge::Left);
        state.set_open(Edge::Left,true);state.advance(0.0);
        assert_eq!(state.canonical_left_phase(),Some(phase));
        assert_eq!(state.effective_extension(Edge::Left).to_bits(),visible.to_bits());
        state.advance(0.1);assert!((state.canonical_left_phase().unwrap()-(phase-3.1)).abs()<1.0e-12);
        state.set_open(Edge::Left,false);state.advance(0.1);assert!((state.canonical_left_phase().unwrap()-phase).abs()<1.0e-12);
    }
    #[test] fn held_target_does_not_restart_and_closed_endpoint_settles() {
        let mut state=ready();state.set_open(Edge::Left,false);
        for _ in 0..20 {assert!(!state.set_open(Edge::Left,false));state.advance(0.05);}
        state.advance(0.000001);assert_eq!(state.canonical_left_phase(),Some(31.0));
        assert_eq!(state.effective_extension(Edge::Left).to_bits(),canonical_left_extension(31).to_bits());
        assert!(state.settled());assert!(!state.is_open(Edge::Left));
    }
    #[test] fn preserves_other_panels_and_actual_opening_order() {
        let mut state=ready();let order=state.snapshot().opening_order;
        state.set_open(Edge::Left,false);state.advance(1.0);
        assert_eq!(state.snapshot().opening_order[2],0);
        state.set_open(Edge::Left,true);state.advance(0.1);
        assert_eq!(state.snapshot().opening_order[2],5);
        for i in [0,1,3] {assert_eq!(state.snapshot().opening_order[i],order[i]);assert_eq!(state.snapshot().extensions[i],1.0);}
        state.set_open(Edge::Right,false);state.advance(0.1);
        assert!(state.effective_extension(Edge::Right)<1.0);assert!(state.is_open(Edge::Left));
    }
    #[test] fn disabled_path_is_exact_and_invalid_dt_does_not_mutate() {
        let mut original=State::default();let mut candidate=State::default();candidate.enable_canonical_left(false);
        for i in 0..80 {if i%7==0 {original.toggle(Edge::Left);candidate.toggle(Edge::Left);}
            original.advance(0.013);candidate.advance(0.013);
            assert_eq!(original.snapshot().extensions,candidate.snapshot().extensions);
            assert_eq!(original.snapshot().opening_order,candidate.snapshot().opening_order);}
        candidate.enable_canonical_left(true);candidate.advance(0.01);let before=candidate.snapshot();let phase=candidate.canonical_left_phase();
        candidate.advance(f64::NAN);candidate.advance(-1.0);
        assert_eq!(candidate.snapshot().extensions,before.extensions);assert_eq!(candidate.canonical_left_phase(),phase);
    }
    #[test] fn unsupported_context_latches_original_motion_without_reentry_jump() {
        let mut state=ready();state.set_open(Edge::Left,false);state.advance(0.25);
        state.set_canonical_left_context(false);let mut original=state.clone();original.enable_canonical_left(false);
        state.advance(0.033);original.advance(0.033);
        assert_eq!(state.snapshot().extensions,original.snapshot().extensions);assert_eq!(state.canonical_left_phase(),None);
        let visible=state.effective_extension(Edge::Left);
        state.set_canonical_left_context(true);state.advance(0.0);assert_eq!(state.canonical_left_phase(),None);assert_eq!(state.effective_extension(Edge::Left),visible);
        state.set_expanded(Edge::Left,true);let mut original=state.clone();original.enable_canonical_left(false);
        state.advance(0.1);original.advance(0.1);assert_eq!(state.snapshot().extensions,original.snapshot().extensions);assert_eq!(state.canonical_left_phase(),None);
    }
}

#[cfg(test)] mod mobile_page_tests {
    use super::*;
    #[test] fn mobile_selection_is_explicit_and_preserves_desktop_targets() {
        let mut s=State::default();s.set_all_open(true);s.advance(1.);let before=s.snapshot();
        assert_eq!(s.mobile_page(),None);
        for name in ["timeline","top","bottom","left","right"] {let page=MobilePage::parse(name).unwrap();assert_eq!(page.name(),name);
            assert!(s.set_mobile_page(Some(page)));assert!(!s.set_mobile_page(Some(page)));
            let v=s.snapshot();assert_eq!(v.open,before.open);assert_eq!(v.opening_order,before.opening_order);assert_eq!(v.extensions,before.extensions);
            for edge in Edge::ALL {assert_eq!(v.panel_visible(edge),page==MobilePage::Panel(edge));}
            assert_eq!(v.timeline_visible(),page==MobilePage::Timeline);
        }
        s.set_mobile_page(None);let v=s.snapshot();assert!(Edge::ALL.iter().all(|e|v.panel_visible(*e)));assert!(v.timeline_visible());
        assert_eq!(v.extensions,before.extensions);assert_eq!(MobilePage::parse("settings"),None);
    }
}

#[cfg(test)]
mod fullscreen_tests {
    use super::*;
    fn settle(s:&mut State){for _ in 0..300{s.advance(1./60.);}}
    #[test]
    fn fullscreen_switch_and_restore_preserve_panel_preferences(){
        let mut s=State::default();s.set_all_open(true);s.set_expanded(Edge::Left,true);settle(&mut s);
        let before=s.snapshot();
        for edge in Edge::ALL {
            assert!(s.set_fullscreen(edge,true));
            for _ in 0..300 {
                s.advance(1./60.);let snap=s.snapshot();
                assert!(snap.fullscreen.iter().sum::<f64>()<=1.+1.0e-6);
                assert!(!snap.timeline_visible());
            }
            assert!(s.snapshot().panel_visible(edge));
            assert!(Edge::ALL.into_iter().filter(|e|*e!=edge).all(|e|!s.snapshot().panel_visible(e)));
        }
        assert!(s.toggle_fullscreen(Edge::Right));settle(&mut s);
        assert!(s.settled());assert!(s.snapshot().timeline_visible());
        assert_eq!(s.snapshot().open,before.open);
        assert_eq!(s.snapshot().opening_order,before.opening_order);
        assert!((s.snapshot().left_width_scale-before.left_width_scale).abs()<1.0e-6);
    }
    #[test]
    fn closing_fullscreen_panel_returns_to_editor_and_reversal_is_continuous(){
        let mut s=State::default();s.toggle_fullscreen(Edge::Left);s.advance(0.05);
        let weight=s.snapshot().fullscreen[2];s.toggle_fullscreen(Edge::Left);
        assert_eq!(s.snapshot().fullscreen[2],weight);s.advance(0.05);
        assert!(s.snapshot().fullscreen[2]<weight);
        s.toggle_fullscreen(Edge::Right);settle(&mut s);s.set_open(Edge::Right,false);settle(&mut s);
        assert!(s.snapshot().timeline_visible());assert_eq!(s.fullscreen_target(),None);
    }
}

#[cfg(test)] mod fullscreen_quality_tests {
 use super::*;
 #[test] fn expansion_duration_is_finite_and_independent_of_frame_rate(){
  let mut a=State::default();a.toggle_fullscreen(Edge::Left);let mut b=a.clone();
  a.advance(0.15);for _ in 0..15{b.advance(0.01);}
  assert!((a.snapshot().fullscreen[2]-0.5).abs()<1e-12);
  assert!((a.snapshot().fullscreen[2]-b.snapshot().fullscreen[2]).abs()<1e-12);
  a.advance(0.15);assert!(!a.fullscreen_animating());assert_eq!(a.snapshot().fullscreen[2],1.);
  a.toggle_fullscreen(Edge::Left);a.advance(0.30);assert!(!a.fullscreen_animating());assert_eq!(a.snapshot().fullscreen,[0.;4]);
 }
}
