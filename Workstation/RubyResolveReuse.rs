//! Exact host-side invalidation for the optional unchanged-lighting resolve skip.
//! A submitted GPU cache is reusable only after a successful resolve submission.
//! The initial implementation excludes procedural backgrounds and mesh shadows.
//! The remaining complete terminal dependencies are compared as float32 bits.
#[derive(Default)]
pub(crate) struct Reuse {
    enabled: bool,
    pending: Option<Vec<u32>>,
    submitted: Option<Vec<u32>>,
}

impl Reuse {
    pub(crate) fn new() -> Self {
        Self::with_enabled(std::env::var("WORMWOOD_WORKSTATION_RESOLVE_REUSE").as_deref() == Ok("1"))
    }

    pub(crate) fn with_enabled(enabled: bool) -> Self {
        Self { enabled, ..Self::default() }
    }

    pub(crate) fn enabled(&self) -> bool { self.enabled }

    // Called before any write to the shared optical input buffer. A subsequent
    // complete-frame upload can install a candidate key; partial writes cannot.
    // Keep the submitted key because an aborted/unsubmitted frame has not
    // changed the GPU response allocation.
    pub(crate) fn invalidate_current(&mut self) { self.pending = None; }

    pub(crate) fn update(&mut self, frame: &[[f32; 4]], plan: &[[f32; 4]],
                        entries: u32, ready: usize) {
        if ready == 0 {
            self.pending = None;
            self.submitted = None;
            return;
        }
        self.pending = if self.enabled { key(frame, plan, entries, ready) } else { None };
    }

    pub(crate) fn required(&self, bake_pending: bool) -> bool {
        !self.enabled || bake_pending
            || self.pending.as_ref().is_none_or(|key| self.submitted.as_ref() != Some(key))
    }

    // Call only after the command buffer containing the declared passes was
    // submitted. Encoding, acquiring a surface, or preparing inputs is not a
    // cache commit. A bake clears resolved rows even if its inputs match.
    pub(crate) fn submitted(&mut self, resolved: bool, baked: bool) {
        if resolved { self.submitted = self.pending.clone(); }
        else if baked { self.submitted = None; }
    }
}

fn index(value: f32) -> Option<usize> {
    (value.is_finite() && value >= 0. && value < 16_777_216. && value.fract() == 0.)
        .then_some(value as usize)
}

fn append(key: &mut Vec<u32>, row: [f32; 4]) {
    key.extend(row.map(f32::to_bits));
}

// Shader source is fixed for this Reuse instance. Legacy intrinsic geometry is
// immutable per native session; a changed intrinsic key sets bake_pending. The
// pixel mode likewise keys its exact camera/material/glyph/engraving inputs.
//
// gemEnvironment/gemFloor dependencies:
//   5: ambient RGB, floor Z; 6: floor RGB/roughness; 9: light/floor enable;
//   10/11: visibility-field descriptor/domain; 16..31: three area lights.
// The field is finite. Escaped rays outside it call gemShadow, so also retain
// active panel count and all four panel optical/geometry blocks, even when the
// interpolated field itself happens to be bit-identical.
//
// Do not key exposure/AgX, meters, playback, or live legacy text: those affect
// fragment shading, not these cached linear terminal responses. For new pixel
// text, changes already require a bake, which always forces a resolve.
fn key(frame: &[[f32; 4]], plan: &[[f32; 4]], entries: u32, ready: usize) -> Option<Vec<u32>> {
    if frame.len() < 256 || plan.len() < 161 || entries == 0 || entries > 20
        || ready == 0 || ready > entries as usize
        || !frame.iter().flatten().all(|value| value.is_finite())
        || !plan[..161].iter().flatten().all(|value| value.is_finite())
        || plan[0][0] != entries as f32 || plan[0][1] < 0.5
        || frame[245][0] != 0. || frame[245][2] != 0. {
        return None;
    }
    // Bound reads to the standard finite field. Unsupported descriptors remain
    // correct by running the original resolve every frame.
    let start = index(frame[10][0])?;
    let edge = index(frame[10][1])?;
    if !(2..=64).contains(&edge) || start < 256 || frame[11][0] <= 0. || frame[11][1] <= 0. {
        return None;
    }
    let end = start.checked_add(3usize.checked_mul(edge.checked_mul(edge)?)?)?;
    let field = frame.get(start..end)?;
    let mut result = Vec::with_capacity(4 * (180 + 161 + field.len()));
    result.extend([entries, ready as u32, plan[0][1].to_bits()]);
    append(&mut result, [frame[0][1], 0., 0., 0.]);
    for at in [5, 6, 9, 10, 11] { append(&mut result, frame[at]); }
    for row in &frame[16..31] { append(&mut result, *row); }
    for row in &frame[32..192] { append(&mut result, *row); }
    // Only xyz is read by gemCachedShadow. Comparing actual values also covers
    // all floor-cache implementation choices without duplicating its CPU key.
    for row in field { result.extend(row[..3].iter().map(|value| value.to_bits())); }
    let mut active = 0;
    for slot in 0..entries as usize {
        let at = 1 + slot * 8;
        let descriptor = plan[at];
        if descriptor[3] < 0.5 { result.push(0); continue; }
        if descriptor[3] != 1. && descriptor[3] != 2. { return None; }
        active += 1;
        result.push(1);
        // Normalize ready/dirty state. Dirty is independently guarded by
        // bake_pending; 2->1 after submission must not force a redundant pass.
        append(&mut result, [descriptor[0], descriptor[1], descriptor[2], 1.]);
        for row in &plan[at + 1..at + 8] { append(&mut result, *row); }
    }
    (active == ready).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
        let mut frame = vec![[0.; 4]; 268];
        frame[0][1] = 1.;
        frame[5] = [0.02, 0.03, 0.04, -0.2];
        frame[6] = [0.1, 0.1, 0.1, 0.3];
        frame[9] = [3., 1., 0., 0.];
        frame[10] = [256., 2., -1., -1.];
        frame[11] = [2., 2., 0., 0.];
        frame[256..].fill([1., 1., 1., 1.]);
        let mut plan = vec![[0.; 4]; 161];
        plan[0] = [1., 1., 0., 0.];
        plan[1] = [0., 128., 0., 1.];
        plan[2] = [0., 0., 0., 1.];
        plan[3] = [-1., -1., 1., 1.];
        plan[4] = [1., 0., 0., 0.];
        (frame, plan)
    }

    fn committed() -> (Reuse, Vec<[f32; 4]>, Vec<[f32; 4]>) {
        let (frame, plan) = fixture();
        let mut state = Reuse::with_enabled(true);
        state.update(&frame, &plan, 1, 1);
        state.submitted(true, false);
        assert!(!state.required(false));
        (state, frame, plan)
    }

    #[test]
    fn workstation_resolve_reuse_requires_first_use_pending_bake_and_actual_submission() {
        let (frame, mut plan) = fixture();
        let mut state = Reuse::with_enabled(true);
        state.update(&frame, &plan, 1, 1);
        assert!(state.required(false));
        state.submitted(false, false);
        assert!(state.required(false));
        state.submitted(true, false);
        assert!(!state.required(false));
        assert!(state.required(true));
        plan[1][3] = 2.;
        state.update(&frame, &plan, 1, 1);
        assert!(!state.required(false)); // state2 alone is normalized
        assert!(state.required(true)); // pending bake invalidates resolved data
        state.submitted(false, true);
        assert!(state.required(false));
        state.submitted(true, true);
        assert!(!state.required(false));
    }

    #[test]
    fn workstation_resolve_reuse_keys_every_terminal_dependency_as_exact_bits() {
        let (frame, plan) = fixture();
        let original = key(&frame, &plan, 1, 1).unwrap();
        for (row, channel) in [(0, 1), (5, 0), (5, 3), (6, 0), (6, 3), (9, 0), (9, 1),
                               (10, 2), (11, 0), (16, 0), (17, 3), (18, 1), (19, 2),
                               (20, 0), (30, 2), (32, 1), (33, 2), (34, 0), (35, 1),
                               (38, 3), (183, 0), (256, 0), (267, 2)] {
            let mut changed = frame.clone();
            changed[row][channel] += 0.01;
            assert_ne!(key(&changed, &plan, 1, 1).unwrap(), original, "missing {row}:{channel}");
        }
        let mut changed = frame.clone();
        changed[32][0] = -0.;
        assert_ne!(key(&changed, &plan, 1, 1).unwrap(), original);
        for (row, channel) in [(0, 3), (8, 0), (8, 1), (200, 0), (214, 0), (244, 0), (245, 3), (256, 3)] {
            let mut changed = frame.clone();
            changed[row][channel] += 0.01;
            assert_eq!(key(&changed, &plan, 1, 1).unwrap(), original, "unrelated {row}:{channel}");
        }
    }

    #[test]
    fn workstation_resolve_reuse_tracks_pose_layout_and_disabled_reactivation() {
        let (mut state, frame, plan) = committed();
        for (row, channel) in [(1, 0), (1, 2), (2, 0), (2, 2), (2, 3), (5, 3), (6, 0)] {
            let mut changed = plan.clone();
            changed[row][channel] += 1.;
            state.update(&frame, &changed, 1, 1);
            assert!(state.required(false), "layout/pose {row}:{channel}");
            // No submission: returning to the original layout reuses the actual
            // GPU cache, not the last prepared but unsubmitted layout.
            state.update(&frame, &plan, 1, 1);
            assert!(!state.required(false));
        }
        state.update(&frame, &plan, 0, 0);
        state.update(&frame, &plan, 1, 1);
        assert!(state.required(false));
    }

    #[test]
    fn workstation_resolve_reuse_keeps_unsubmitted_and_partial_writes_pending() {
        let (mut state, mut frame, plan) = committed();
        frame[20][0] = 2.;
        state.update(&frame, &plan, 1, 1);
        assert!(state.required(false));
        state.submitted(false, false);
        assert!(state.required(false));
        state.submitted(true, false);
        assert!(!state.required(false));
        state.invalidate_current();
        assert!(state.required(false));
        state.submitted(true, false); // unsupported partial-input resolve
        state.update(&frame, &plan, 1, 1);
        assert!(state.required(false));
    }

    #[test]
    fn workstation_resolve_reuse_rejects_procedural_invalid_and_unbounded_fields() {
        let (frame, plan) = fixture();
        for (row, channel, value) in [(245, 0, 1.), (245, 2, 1.), (10, 0, 267.),
                                      (10, 1, 65.), (10, 1, 1.), (11, 0, 0.), (5, 0, f32::NAN)] {
            let mut changed = frame.clone();
            changed[row][channel] = value;
            let mut state = Reuse::with_enabled(true);
            state.update(&changed, &plan, 1, 1);
            state.submitted(true, false);
            assert!(state.required(false), "unsupported key {row}:{channel}");
        }
        let mut disabled = Reuse::with_enabled(false);
        disabled.update(&frame, &plan, 1, 1);
        disabled.submitted(true, false);
        assert!(disabled.required(false));
    }
}
