//! Optional control-thread observations; never consulted by cache decisions.
use super::{Candidate, Entry, SLOTS, TIMELINE_FIRST, TIMELINE_END, translated_planes_match};
use serde_json::{Value, json};

const UPDATE_LIMIT: u64 = 256;
const DIFF_LIMIT: usize = 16;

struct Previous {
    target: usize,
    key: Vec<u32>,
    descriptor: [[f32; 4]; 8],
    dirty: bool,
}

pub(super) struct CacheCauses {
    update: u64,
    previous: [Option<Previous>; SLOTS],
    records: [Option<Value>; SLOTS],
    report: Value,
}

impl CacheCauses {
    pub(super) fn new() -> Self {
        Self { update: 0, previous: std::array::from_fn(|_| None),
            records: std::array::from_fn(|_| None),
            report: json!({"enabled":true,"schema_version":1,"update_index":0,
                "status":"awaiting_update","records":[]}) }
    }

    fn active(&self) -> bool { self.update > 0 && self.update <= UPDATE_LIMIT }

    pub(super) fn begin(&mut self, slots: &[Option<Entry>; SLOTS]) {
        self.update = self.update.saturating_add(1);
        self.records = std::array::from_fn(|_| None);
        self.previous = std::array::from_fn(|slot| self.active().then(|| slots[slot].as_ref()
            .map(|entry| Previous { target: entry.target, key: entry.key.clone(),
                descriptor: entry.descriptor, dirty: entry.dirty })).flatten());
        if !self.active() {
            self.report = json!({"enabled":true,"schema_version":1,"update_index":self.update,
                "status":"sample_budget_exhausted","sampled_updates":UPDATE_LIMIT,
                "dropped_updates":self.update-UPDATE_LIMIT,"update_limit":UPDATE_LIMIT,"records":[]});
        }
    }

    pub(super) fn place(&mut self, slot: usize, at_placement: Option<&Entry>,
                       candidate: &Candidate, reused: bool) {
        if !self.active() { return; }
        let old = self.previous[slot].as_ref();
        let old_key = old.map(|entry| entry.key.as_slice()).unwrap_or(&[]);
        let (changed, first) = key_diff(old_key, &candidate.key);
        let allocation = &candidate.descriptor;
        let decision = if reused { "retained" } else if at_placement.is_some() { "replaced" } else { "new" };
        let slot_identity = match old {
            None => "previously_empty",
            Some(old) if old.target == candidate.target => "same_target",
            Some(_) => "different_target",
        };
        self.records[slot] = Some(json!({
            "slot":slot,"target":candidate.target,"mode":allocation[4][3],
            "previous_target":old.map(|entry|entry.target),
            "previous_mode":old.map(|entry|entry.descriptor[4][3]),
            "previous_at_placement":at_placement.map(|entry|entry.target),
            "slot_identity":slot_identity,"decision":decision,"admitted":true,
            "pending_before_update":old.is_some_and(|entry|entry.dirty),
            "pending_at_placement":at_placement.is_some_and(|entry|entry.dirty),
            "pending_inherited":reused && at_placement.is_some_and(|entry|entry.dirty),
            "newly_dirty":!reused,"dirty_after_placement":!reused || at_placement.is_some_and(|entry|entry.dirty),
            "allocation_before":old.map(|entry|allocation_info(&entry.descriptor)),
            "allocation_after":allocation_info(allocation),
            "base_changed":old.map(|entry|entry.descriptor[0][2]!=allocation[0][2]),
            "capacity_changed":old.map(|entry|entry.descriptor[6][0]!=allocation[6][0]),
            "allocation_changed":old.map(|entry|entry.descriptor[0][2]!=allocation[0][2]
                || entry.descriptor[6][0]!=allocation[6][0]),
            "pixel_size_or_stride_changed":old.map(|entry|entry.descriptor[4][..3]!=allocation[4][..3]),
            "mode_changed":old.map(|entry|entry.descriptor[4][3]!=allocation[4][3]),
            "translated_plane_mismatch":at_placement.map(|entry|
                !translated_planes_match(&entry.panel,&candidate.panel)),
            "key_comparison_scope":"positional bits against this slot at update start; null means absent",
            "key_comparison_same_target":old.is_some_and(|entry|entry.target==candidate.target),
            "old_key_words":old_key.len(),"new_key_words":candidate.key.len(),
            "old_key_layout":old.map(|entry|key_layout(entry.target,&entry.descriptor,entry.key.len())),
            "new_key_layout":key_layout(candidate.target,allocation,candidate.key.len()),
            "changed_key_word_count":changed,"first_changed_key_words":first,
            "changed_key_words_truncated":changed>DIFF_LIMIT
        }));
    }

    pub(super) fn allocation_failed(&mut self, slot: usize, at_placement: Option<&Entry>,
                                    candidate: &Candidate) {
        if !self.active() { return; }
        // Reuse only the diagnostic record formatter; there was no placement.
        self.place(slot, at_placement, candidate, false);
        let record = self.records[slot].as_mut().unwrap();
        record["decision"] = json!("declined");
        record["admitted"] = json!(false);
        record["decline_reason"] = json!("pixel_allocation_unavailable");
        record["candidate_shape"] = allocation_info(&candidate.descriptor);
        for name in ["allocation_after","allocation_changed","base_changed","capacity_changed",
                     "dirty_after_placement","translated_plane_mismatch"] {
            record[name] = Value::Null;
        }
        record["newly_dirty"] = json!(false);
    }

    pub(super) fn removed(&mut self, slot: usize, reason: &'static str) {
        if !self.active() { return; }
        let Some(old) = &self.previous[slot] else { return; };
        let (changed, first) = key_diff(&old.key, &[]);
        self.records[slot] = Some(json!({"slot":slot,"target":null,"mode":null,
            "previous_target":old.target,"previous_mode":old.descriptor[4][3],
            "decision":"removed","admitted":false,"removal_reason":reason,
            "pending_before_update":old.dirty,"pending_inherited":false,"newly_dirty":false,
            "dirty_after_placement":null,"translated_plane_mismatch":null,
            "allocation_before":allocation_info(&old.descriptor),"allocation_after":null,
            "old_key_words":old.key.len(),"new_key_words":0,
            "old_key_layout":key_layout(old.target,&old.descriptor,old.key.len()),"new_key_layout":null,
            "key_comparison_scope":"slot removed; all old words are absent, not changed scene inputs",
            "changed_key_word_count":changed,"first_changed_key_words":first,
            "changed_key_words_truncated":changed>DIFF_LIMIT}));
    }

    pub(super) fn finish(&mut self, disabled: bool) {
        if !self.active() { return; }
        let records: Vec<_> = self.records.iter_mut().filter_map(Option::take).collect();
        self.report = json!({"enabled":true,"schema_version":1,"update_index":self.update,
            "status":if disabled {"plan_disabled_no_cache_decisions"} else {"observed"},
            "scope":"CPU placement and invalidation causes; not GPU cache hits, path coverage or completed bake work",
            "update_limit":UPDATE_LIMIT,"sampled_updates":self.update,"dropped_updates":0,
            "record_limit":SLOTS,"diff_limit":DIFF_LIMIT,"record_count":records.len(),
            "key_schema":source_schema(),"records":records});
        self.previous = std::array::from_fn(|_| None);
    }

    pub(super) fn report(&self) -> &Value { &self.report }
}

fn allocation_info(descriptor: &[[f32;4];8]) -> Value {
    json!({"base_rows":descriptor[0][2],"capacity_rows":descriptor[6][0],
        "pixel_width":descriptor[4][0],"pixel_height":descriptor[4][1],
        "pixel_stride_rows":descriptor[4][2],"mode":descriptor[4][3],
        "legacy_edge":descriptor[0][1]})
}

fn key_diff(old: &[u32], new: &[u32]) -> (usize, Vec<(usize, Option<u32>, Option<u32>)>) {
    let mut count = 0;
    let mut first = Vec::new();
    for word in 0..old.len().max(new.len()) {
        let a=old.get(word).copied(); let b=new.get(word).copied();
        if a!=b { count+=1; if first.len()<DIFF_LIMIT { first.push((word,a,b)); } }
    }
    (count,first)
}

fn key_layout(target: usize, descriptor: &[[f32;4];8], words: usize) -> Value {
    let pixel=descriptor[4][3]==1.;
    let timeline=(TIMELINE_FIRST..TIMELINE_END).contains(&target);
    if target<4 { return json!({"kind":"legacy_panel","recognized":words==172}); }
    if !pixel { return json!({"kind":"legacy_control","recognized":words==176}); }
    if timeline {
        let glyphs=words.checked_sub(320).filter(|n|n%4==0 && *n/4<=8).map(|n|n/4);
        return json!({"kind":"pixel_timeline","recognized":glyphs.is_some(),
            "glyph_reference_rows":glyphs,"glyph_words_start":204,
            "own_rows_start":glyphs.map(|n|204+n*4),"light_rows_start":glyphs.map(|n|232+n*4)});
    }
    let mips=if words==208 {Some(0)}else{
        words.checked_sub(312).filter(|n|n%4==0 && (1..=11).contains(&(*n/4))).map(|n|n/4)};
    json!({"kind":"pixel_control","recognized":mips.is_some(),"mip_rows":mips,
        "mip_words_start":208,
        "raster_identity_start":mips.filter(|n|*n>0).map(|n|208+n*4),
        "descriptor_rows_start":mips.filter(|n|*n>0).map(|n|212+n*4),
        "light_rows_start":mips.filter(|n|*n>0).map(|n|224+n*4)})
}

fn source_schema() -> Value {
    json!({"version":1,"unit":"u32 float32 bits; zero-based half-open word ranges",
        "fixed_prefix":[
            [0,4,"[frame[8].z, frame[8].w, 0, 0] path budgets"],
            [4,8,"[0, 0, 0, pose.w] scale"],
            [8,12,"derived local bounds [lo.x, lo.y, hi.x, hi.y]"],
            [12,172,"make_candidate material rows +0..40; four words per row"]],
        "material_normalization":"panel offset2.xyz=0 and offsets6..32.w=0; all nonpanel offset2.w=0; all other float bits retained",
        "legacy_control":[[172,176,"[control tree.x, tree.y, 0, 0]; pose_at(frame, instance)+1"]],
        "pixel_extension":"32 words at172 for timeline, at176 for control: pose, frame1, frame2, frame3, frame4, [frame8.x,frame8.y,0,0], descriptor4, descriptor5",
        "pixel_control_suffix":"none for length208; otherwise mip rows first (1..11), [descriptor,raster,end,1], descriptor's three rows, then22 light/config rows; exact starts in key_layout",
        "pixel_timeline_suffix":"0..8 glyph refs [codepoint,immutable_begin,immutable_end,1], then7 own rows, then22 light/config rows; exact starts in key_layout",
        "timeline_own_rows":["[0,frame248.y,frame248.z,0]","frame249 clip","instance pose","instance tree","instance size","letter codes0..3","letter codes4..7"],
        "light_rows_control":[5,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,213,214,215,216,223,244],
        "light_rows_timeline":[5,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,213,214,215,216,223,224],
        "light_normalization":"frame5.w=0; RGB bits retained",
        "limitations":"material/instance/descriptor/mip/atlas addresses are dynamic source pointers; bounds are derived. A changed glyph/mip count shifts later positional words: interpret old and new layouts separately. No fixed source row is claimed across variable spans."})
}

#[cfg(test)]
mod cache_cause_helper_tests {
    use super::*;

    #[test]
    fn cache_cause_diff_keeps_raw_bits_and_distinguishes_missing_words() {
        let old=[0x0000_0000,0x7fc0_0001,0x3f80_0000,0x8000_0000];
        let new=[0x8000_0000,0x7fc0_0002,0x3f80_0000,0x0000_0000];
        let (count,changes)=key_diff(&old,&new);
        assert_eq!(count,3);
        assert_eq!(changes,vec![(0,Some(old[0]),Some(new[0])),
            (1,Some(old[1]),Some(new[1])),(3,Some(old[3]),Some(new[3]))]);
        assert_eq!(key_diff(&[],&[0]),(1,vec![(0,None,Some(0))]));
        assert_eq!(key_diff(&[0],&[]),(1,vec![(0,Some(0),None)]));
        assert_eq!(key_diff(&old,&old),(0,Vec::new()));
    }

    #[test]
    fn cache_cause_diff_bounds_records_but_counts_all_changed_or_removed_words() {
        let old=vec![0;40]; let new=vec![1;40];
        let (count,changes)=key_diff(&old,&new);
        assert_eq!(count,40);
        assert_eq!(changes.len(),16);
        assert_eq!(changes.iter().map(|change|change.0).collect::<Vec<_>>(),(0..16).collect::<Vec<_>>());
        let (count,changes)=key_diff(&old,&new[..3]);
        assert_eq!(count,40);
        assert_eq!(changes.len(),16);
        assert_eq!(changes[3],(3,Some(0),None));
        assert_eq!(changes[15],(15,Some(0),None));
    }

    #[test]
    fn cache_cause_layout_rejects_unknown_lengths_and_keeps_dynamic_boundaries() {
        let mut descriptor=[[0.;4];8];
        assert_eq!(key_layout(0,&descriptor,172)["recognized"],true);
        assert_eq!(key_layout(0,&descriptor,176)["recognized"],false);
        assert_eq!(key_layout(4,&descriptor,176)["recognized"],true);
        assert_eq!(key_layout(4,&descriptor,172)["recognized"],false);
        descriptor[4][3]=1.;
        let unengraved=key_layout(1028,&descriptor,208);
        assert_eq!(unengraved["mip_rows"],0);
        assert!(unengraved["light_rows_start"].is_null());
        for mips in 1..=11 {
            let layout=key_layout(1028,&descriptor,312+4*mips);
            assert_eq!(layout["recognized"],true);
            assert_eq!(layout["raster_identity_start"],208+4*mips);
            assert_eq!(layout["descriptor_rows_start"],212+4*mips);
            assert_eq!(layout["light_rows_start"].as_u64().unwrap()+88,(312+4*mips) as u64);
        }
        for invalid in [0,207,209,312,313,360] {
            assert_eq!(key_layout(1028,&descriptor,invalid)["recognized"],false);
        }
        for glyphs in 0..=8 {
            let layout=key_layout(TIMELINE_FIRST,&descriptor,320+4*glyphs);
            assert_eq!(layout["recognized"],true);
            assert_eq!(layout["own_rows_start"],204+4*glyphs);
            assert_eq!(layout["light_rows_start"].as_u64().unwrap()+88,(320+4*glyphs) as u64);
        }
        for invalid in [0,319,321,356] {
            assert_eq!(key_layout(TIMELINE_FIRST,&descriptor,invalid)["recognized"],false);
        }
    }

    #[test]
    fn cache_cause_record_budget_is_per_slot_and_never_grows_on_overwrite() {
        let slots: [Option<Entry>;SLOTS]=std::array::from_fn(|slot| Some(Entry {
            target:slot,key:vec![0;40],descriptor:[[0.;4];8],panel:None,dirty:false}));
        let mut causes=CacheCauses::new(); causes.begin(&slots);
        for slot in 0..SLOTS {
            let candidate=Candidate {target:slot,key:vec![1;40],descriptor:[[0.;4];8],panel:None,area:1.};
            causes.place(slot,slots[slot].as_ref(),&candidate,false);
            causes.place(slot,slots[slot].as_ref(),&candidate,false);
        }
        causes.finish(false);
        let records=causes.report()["records"].as_array().unwrap();
        assert_eq!(records.len(),20);
        for (slot,record) in records.iter().enumerate() {
            assert_eq!(record["slot"],slot);
            assert_eq!(record["changed_key_word_count"],40);
            assert_eq!(record["first_changed_key_words"].as_array().unwrap().len(),16);
            assert_eq!(record["changed_key_words_truncated"],true);
        }
    }
}
