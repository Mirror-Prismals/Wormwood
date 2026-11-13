//! Cardinal RetrospectiveCaptureSystem.cpp recall calculations, control thread.
//! The capture owner supplies serial-validated snapshots/reads. This module
//! never opens audio/MIDI devices and does not invent a project tempo.
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    pub head: u64,
    pub capacity: u64,
    pub capture_rate: u32,
    pub document_rate: u32,
    pub source_start: u64,
    pub audio_clear: u64,
    pub midi_clear: u64,
    pub last_playing_frame: Option<u64>,
    pub playhead_frame: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    pub first: u64,
    pub end: u64,
    pub start_frame: u64,
    pub length: u64,
    pub mapped: bool,
}
/// Prefer the most recent uninterrupted pass, including immediately after Stop.
/// A seek or loop wrap starts a new pass, so different takes never concatenate.
pub(crate) fn range<F>(
    clock: Clock,
    seconds: u32,
    audio: bool,
    mut read_song: F,
) -> Result<Range, String>
where
    F: FnMut(u64) -> Option<Option<u64>>,
{
    if !(8000..=192000).contains(&clock.capture_rate)
        || !(8000..=192000).contains(&clock.document_rate)
    {
        return Err("invalid retrospective sample rate".into());
    }
    let count = clock
        .capacity
        .min(seconds.clamp(5, 30) as u64 * clock.capture_rate as u64);
    let mut first = clock.head.saturating_sub(count).max(if audio {
        clock.source_start.max(clock.audio_clear)
    } else {
        clock.midi_clear
    });
    let mut end = clock.head;
    if first >= end {
        return Err("No history yet.".into());
    }
    let last = clock
        .last_playing_frame
        .filter(|last| *last >= first)
        .map(|last| last.min(end - 1));
    if let Some((last, last_song)) = last.and_then(|frame| Some((frame, read_song(frame)??))) {
        end = last + 1;
        let mut next = last_song;
        let mut i = last;
        while i > first {
            match read_song(i - 1).flatten() {
                Some(song) if song <= next && next - song <= 2 => {
                    next = song;
                    i -= 1;
                }
                _ => break,
            }
        }
        first = i;
        Ok(Range {
            first,
            end,
            start_frame: next,
            length: last_song - next + 1,
            mapped: true,
        })
    } else {
        let length = ((end - first) as f64 * clock.document_rate as f64 / clock.capture_rate as f64)
            .round() as u64;
        if length == 0 {
            return Err("No history yet.".into());
        }
        Ok(Range {
            first,
            end,
            start_frame: clock.playhead_frame,
            length,
            mapped: false,
        })
    }
}
#[derive(Debug)]
pub(crate) struct Audio {
    pub start_frame: u64,
    pub channels: usize,
    pub interleaved: Vec<f32>,
    pub mapped: bool,
}
pub(crate) fn audio<F>(range: Range, stereo: bool, mut read: F) -> Result<Audio, String>
where
    F: FnMut(u64) -> Option<[f32; 2]>,
{
    let count = range
        .end
        .checked_sub(range.first)
        .filter(|n| *n > 0 && *n <= 30 * 192000)
        .ok_or("invalid recall range")? as usize;
    let output = usize::try_from(range.length)
        .ok()
        .filter(|n| *n > 0 && *n <= 30 * 192000)
        .ok_or("invalid recall output length")?;
    let mut input = Vec::with_capacity(count);
    for frame in range.first..range.end {
        input.push(
            read(frame)
                .filter(|f| f.iter().all(|v| v.is_finite()))
                .ok_or("History changed while copying; try Recall Audio again.")?,
        );
    }
    let channels = if stereo { 2 } else { 1 };
    let mut interleaved = Vec::with_capacity(output * channels);
    for i in 0..output {
        let position = i as f64 * count as f64 / output as f64;
        let a = (position as usize).min(count - 1);
        let b = (a + 1).min(count - 1);
        let blend = (position - a as f64) as f32;
        for channel in 0..channels {
            interleaved.push(input[a][channel] + (input[b][channel] - input[a][channel]) * blend);
        }
    }
    Ok(Audio {
        start_frame: range.start_frame,
        channels,
        interleaved,
        mapped: range.mapped,
    })
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct MidiEvent {
    pub frame: u64,
    pub track: i32,
    pub pitch: u8,
    pub velocity: f32,
    pub tuning_cents: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Note {
    pub pitch: u8,
    pub start_frame: u64,
    pub length: u64,
    pub velocity: f32,
    pub tuning_cents: f64,
}
/// Events must come from the capture owner's serial-checked bounded ring, in
/// serial order. Retain note-ons before the recall window to trim held notes.
pub(crate) fn midi(
    range: Range,
    capture_rate: u32,
    document_rate: u32,
    events: &[MidiEvent],
) -> Result<Vec<Note>, String> {
    if !(8000..=192000).contains(&capture_rate)
        || !(8000..=192000).contains(&document_rate)
        || range.end <= range.first
        || range.length == 0
        || events.len() > 8192
    {
        return Err("invalid MIDI recall input".into());
    }
    if events.windows(2).any(|a| a[0].frame > a[1].frame)
        || events
            .iter()
            .any(|e| e.pitch > 127 || !e.velocity.is_finite() || !e.tuning_cents.is_finite())
    {
        return Err("invalid MIDI history snapshot".into());
    }
    let step = if range.mapped {
        1.
    } else {
        document_rate as f64 / capture_rate as f64
    };
    let mut held = BTreeMap::<(i32, u8), MidiEvent>::new();
    let mut notes = Vec::new();
    let finish = |event: MidiEvent, end: u64, notes: &mut Vec<Note>| {
        let begin = event.frame.max(range.first);
        let stop = end.min(range.end);
        if stop <= begin {
            return;
        }
        let start = ((begin - range.first) as f64 * step).round() as u64;
        if start >= range.length {
            return;
        }
        let length = (((stop - begin) as f64 * step).round() as u64)
            .max(1)
            .min(range.length - start);
        notes.push(Note {
            pitch: event.pitch,
            start_frame: start,
            length,
            velocity: event.velocity,
            tuning_cents: event.tuning_cents,
        });
    };
    for event in events {
        if event.frame >= range.end {
            break;
        }
        let key = (event.track, event.pitch);
        if let Some(active) = held.remove(&key) {
            finish(active, event.frame, &mut notes);
        }
        if event.velocity > 0. {
            held.insert(key, *event);
        }
    }
    for event in held.values() {
        finish(*event, range.end, &mut notes);
    }
    notes.sort_by_key(|note| note.start_frame);
    if notes.is_empty() {
        Err("No played MIDI notes in this history window.".into())
    } else {
        Ok(notes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn clock() -> Clock {
        Clock {
            head: 100,
            capacity: 240000,
            capture_rate: 48000,
            document_rate: 48000,
            source_start: 0,
            audio_clear: 0,
            midi_clear: 0,
            last_playing_frame: Some(99),
            playhead_frame: 800,
        }
    }
    #[test]
    fn loop_wrap_recall_only_copies_latest_uninterrupted_pass() {
        let r = range(clock(), 30, true, |i| {
            Some(Some(if i < 70 { i + 100 } else { i - 70 }))
        })
        .unwrap();
        assert_eq!(
            r,
            Range {
                first: 70,
                end: 100,
                start_frame: 0,
                length: 30,
                mapped: true
            }
        );
    }
    #[test]
    fn stop_preserves_last_playing_pass() {
        let mut c = clock();
        c.head = 140;
        let r = range(c, 30, true, |i| Some((i < 100).then_some(i + 200))).unwrap();
        assert_eq!(r.end, 100);
        assert_eq!(r.start_frame, 200);
        assert_eq!(r.length, 100);
    }
    #[test]
    fn stopped_history_places_at_playhead_and_resamples() {
        let mut c = clock();
        c.last_playing_frame = None;
        c.capture_rate = 24000;
        let r = range(c, 30, true, |_| Some(None)).unwrap();
        assert_eq!(r.start_frame, 800);
        assert_eq!(r.length, 200);
        let a = audio(r, false, |i| Some([i as f32, 0.])).unwrap();
        assert_eq!(a.interleaved.len(), 200);
        assert_eq!(a.interleaved[1], 0.5);
    }
    #[test]
    fn clear_floors_and_changed_audio_fail_without_partial_output() {
        let mut c = clock();
        c.audio_clear = 60;
        c.source_start = 70;
        let r = range(c, 30, true, |i| Some(Some(i))).unwrap();
        assert_eq!(r.first, 70);
        assert!(audio(r, true, |i| if i == 80 { None } else { Some([0.; 2]) }).is_err());
    }
    #[test]
    fn midi_trims_held_notes_at_both_edges_and_keeps_retrigger() {
        let r = Range {
            first: 10,
            end: 30,
            start_frame: 500,
            length: 20,
            mapped: true,
        };
        let events = [
            MidiEvent {
                frame: 0,
                track: 1,
                pitch: 60,
                velocity: 0.8,
                tuning_cents: 12.,
            },
            MidiEvent {
                frame: 20,
                track: 1,
                pitch: 60,
                velocity: 0.6,
                tuning_cents: 12.,
            },
            MidiEvent {
                frame: 40,
                track: 1,
                pitch: 60,
                velocity: 0.,
                tuning_cents: 12.,
            },
        ];
        let n = midi(r, 48000, 48000, &events).unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].start_frame, n[0].length), (0, 10));
        assert_eq!((n[1].start_frame, n[1].length), (10, 10));
        assert_eq!(n[0].tuning_cents, 12.);
    }
    #[test]
    fn simultaneous_pitches_on_separate_tracks_remain_distinct() {
        let r = Range {
            first: 0,
            end: 20,
            start_frame: 0,
            length: 20,
            mapped: true,
        };
        let events = [
            MidiEvent {
                frame: 0,
                track: 1,
                pitch: 60,
                velocity: 0.8,
                tuning_cents: 0.,
            },
            MidiEvent {
                frame: 0,
                track: 2,
                pitch: 60,
                velocity: 0.5,
                tuning_cents: 7.,
            },
        ];
        assert_eq!(midi(r, 48000, 48000, &events).unwrap().len(), 2);
    }
}
