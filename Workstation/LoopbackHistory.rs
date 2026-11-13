//! Control-thread mirror of Cardinal's bottom-panel retrospective buffer.
//! Audio capture is supplied by the capture owner. This ring is never itself a
//! device callback; source switches require that owner's acknowledgment before
//! new frames are appended. Silence and missing history retain elapsed time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Microphone,
    Player,
}
impl Source {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Microphone => "MIC",
            Self::Player => "PLAYER AUDIO",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Microphone => "SOURCE: MIC",
            Self::Player => "SOURCE: PLAYER",
        }
    }
    pub(crate) fn channels(self) -> usize {
        if self == Self::Player { 2 } else { 1 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub start: u64,
    pub end: u64,
}
#[derive(Debug)]
pub(crate) struct Copy {
    pub sample_rate: u32,
    pub channels: usize,
    pub interleaved: Vec<f32>,
    pub absolute_start: u64,
}
#[derive(Debug)]
pub(crate) struct History {
    source: Source,
    rate: u32,
    memory_seconds: u32,
    frames: Vec<[f32; 2]>,
    write: usize,
    valid: usize,
    head: u64,
    selection: Option<Selection>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            source: Source::Microphone,
            rate: 0,
            memory_seconds: 30,
            frames: Vec::new(),
            write: 0,
            valid: 0,
            head: 0,
            selection: None,
        }
    }
}
impl History {
    pub(crate) fn source(&self) -> Source {
        self.source
    }
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }
    pub(crate) fn memory_seconds(&self) -> u32 {
        self.memory_seconds
    }
    pub(crate) fn valid_frames(&self) -> usize {
        self.valid
    }
    pub(crate) fn head(&self) -> u64 {
        self.head
    }
    pub(crate) fn oldest(&self) -> u64 {
        self.head - self.valid as u64
    }
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.selection
    }
    pub(crate) fn configure(&mut self, source: Source, rate: u32) -> Result<(), String> {
        if !(8000..=192000).contains(&rate) {
            return Err("loopback rate must be 8–192 kHz".into());
        }
        let preserve = source == self.source && rate == self.rate;
        let capacity = rate as usize * self.memory_seconds as usize;
        if preserve && capacity == self.frames.len() {
            return Ok(());
        }
        let keep = if preserve {
            self.valid.min(capacity)
        } else {
            0
        };
        let mut next = vec![[0.; 2]; capacity];
        for (i, value) in next.iter_mut().take(keep).enumerate() {
            *value = self
                .frame(self.head - keep as u64 + i as u64)
                .unwrap_or([0.; 2]);
        }
        self.frames = next;
        self.write = keep % capacity;
        self.valid = keep;
        self.source = source;
        self.rate = rate;
        if !preserve {
            self.head = 0;
            self.selection = None;
        } else {
            self.clamp_selection();
        }
        Ok(())
    }
    pub(crate) fn cycle_memory(&mut self) -> Result<(), String> {
        self.memory_seconds = match self.memory_seconds {
            5 => 10,
            10 => 20,
            20 => 30,
            _ => 5,
        };
        if self.rate > 0 {
            self.configure(self.source, self.rate)?;
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self) {
        self.valid = 0;
        self.write = 0;
        self.head = 0;
        self.selection = None;
    }
    /// Drain at most 65536 frames, as Cardinal does. Gaps inside a capture span
    /// are passed as None and stored as silence; callers may resume at head().
    pub(crate) fn drain<F>(
        &mut self,
        source: Source,
        floor: u64,
        head: u64,
        mut read: F,
    ) -> Result<usize, String>
    where
        F: FnMut(u64) -> Option<[f32; 2]>,
    {
        if source != self.source || self.frames.is_empty() {
            return Err("loopback capture source/rate is not acknowledged".into());
        }
        if floor > head {
            return Err("loopback history floor exceeds head".into());
        }
        let floor = floor.max(head.saturating_sub(self.frames.len() as u64));
        if self.head < floor || self.head > head {
            self.head = floor;
            self.valid = 0;
            self.write = 0;
            self.selection = None;
        }
        let end = head.min(self.head.saturating_add(65536));
        let count = (end - self.head) as usize;
        while self.head < end {
            let frame = read(self.head)
                .filter(|v| v.iter().all(|x| x.is_finite()))
                .unwrap_or([0.; 2]);
            self.frames[self.write] = if self.source == Source::Microphone {
                [frame[0], 0.]
            } else {
                frame
            };
            self.write = (self.write + 1) % self.frames.len();
            self.valid = (self.valid + 1).min(self.frames.len());
            self.head += 1;
        }
        self.clamp_selection();
        Ok(count)
    }
    pub(crate) fn frame(&self, absolute: u64) -> Option<[f32; 2]> {
        if absolute < self.oldest() || absolute >= self.head || self.frames.is_empty() {
            return None;
        }
        let oldest = (self.write + self.frames.len() - self.valid) % self.frames.len();
        Some(self.frames[(oldest + (absolute - self.oldest()) as usize) % self.frames.len()])
    }
    pub(crate) fn frame_at_fraction(&self, x: f64) -> Option<u64> {
        if self.valid == 0 || !x.is_finite() {
            return None;
        }
        let relative = ((x.clamp(0., 1.) * self.valid as f64).floor() as usize).min(self.valid - 1);
        Some(self.oldest() + relative as u64)
    }
    pub(crate) fn select(&mut self, anchor: u64, cursor: u64) {
        self.selection = Some(Selection {
            start: anchor.min(cursor),
            end: anchor.max(cursor).saturating_add(1).min(self.head),
        });
        self.clamp_selection();
    }
    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
    }
    fn clamp_selection(&mut self) {
        self.selection = self.selection.and_then(|s| {
            let start = s.start.max(self.oldest());
            let end = s.end.min(self.head);
            (end > start).then_some(Selection { start, end })
        });
    }
    pub(crate) fn copy_selection(&self) -> Option<Copy> {
        let selected = self.selection?;
        let mut interleaved =
            Vec::with_capacity((selected.end - selected.start) as usize * self.source.channels());
        for absolute in selected.start..selected.end {
            let frame = self.frame(absolute)?;
            interleaved.push(frame[0]);
            if self.source == Source::Player {
                interleaved.push(frame[1]);
            }
        }
        Some(Copy {
            sample_rate: self.rate,
            channels: self.source.channels(),
            interleaved,
            absolute_start: selected.start,
        })
    }
    /// Cardinal's bounded waveform envelope, in chronological source order.
    pub(crate) fn waveform(&self, columns: usize) -> Vec<[[f32; 2]; 2]> {
        let columns = columns.clamp(1, 1000);
        let mut result = Vec::with_capacity(columns);
        if self.valid == 0 {
            return result;
        }
        for column in 0..columns {
            let begin = column * self.valid / columns;
            let end = ((column + 1) * self.valid / columns)
                .max(begin + 1)
                .min(self.valid);
            if begin >= self.valid {
                break;
            }
            let stride = ((end - begin) / 64).max(1);
            let mut envelope = [[1f32, -1f32]; 2];
            for relative in (begin..end).step_by(stride).chain(std::iter::once(end - 1)) {
                let frame = self.frame(self.oldest() + relative as u64).unwrap();
                for channel in 0..self.source.channels() {
                    let value = frame[channel].clamp(-1., 1.);
                    envelope[channel][0] = envelope[channel][0].min(value);
                    envelope[channel][1] = envelope[channel][1].max(value);
                }
            }
            if self.source == Source::Microphone {
                envelope[1] = [0.; 2];
            }
            result.push(envelope);
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_keeps_silence_and_exact_absolute_selection() {
        let mut h = History::default();
        h.configure(Source::Microphone, 8000).unwrap();
        h.drain(Source::Microphone, 0, 10, |i| {
            if i == 4 { None } else { Some([i as f32, 99.]) }
        })
        .unwrap();
        h.select(2, 7);
        let c = h.copy_selection().unwrap();
        assert_eq!(c.channels, 1);
        assert_eq!(c.absolute_start, 2);
        assert_eq!(c.interleaved, vec![2., 3., 0., 5., 6., 7.]);
    }
    #[test]
    fn source_change_resets_and_rejects_stale_frames() {
        let mut h = History::default();
        h.configure(Source::Microphone, 8000).unwrap();
        h.drain(Source::Microphone, 0, 5, |_| Some([1., 2.]))
            .unwrap();
        h.select(0, 3);
        h.configure(Source::Player, 8000).unwrap();
        assert!(h.selection().is_none());
        assert_eq!(h.valid_frames(), 0);
        assert!(
            h.drain(Source::Microphone, 0, 10, |_| Some([1., 2.]))
                .is_err()
        );
        h.drain(Source::Player, 0, 2, |_| Some([1., 2.])).unwrap();
        h.select(0, 1);
        assert_eq!(
            h.copy_selection().unwrap().interleaved,
            vec![1., 2., 1., 2.]
        );
    }
    #[test]
    fn memory_resize_retains_newest_frames_and_clamps_selection() {
        let mut h = History::default();
        h.configure(Source::Player, 8000).unwrap();
        h.drain(Source::Player, 0, 60000, |i| Some([i as f32, 0.]))
            .unwrap();
        h.select(10000, 30000);
        h.cycle_memory().unwrap();
        assert_eq!(h.memory_seconds(), 5);
        assert_eq!(h.oldest(), 20000);
        assert_eq!(
            h.selection(),
            Some(Selection {
                start: 20000,
                end: 30001
            })
        );
        assert_eq!(h.frame(20000).unwrap()[0], 20000.);
    }
    #[test]
    fn wrapped_history_selection_expires() {
        let mut h = History::default();
        h.configure(Source::Microphone, 8000).unwrap();
        h.cycle_memory().unwrap();
        h.drain(Source::Microphone, 0, 40000, |i| Some([i as f32, 0.]))
            .unwrap();
        h.select(0, 5);
        h.drain(Source::Microphone, 0, 40010, |i| Some([i as f32, 0.]))
            .unwrap();
        assert_eq!(h.oldest(), 10);
        assert!(h.selection().is_none());
        assert_eq!(h.frame_at_fraction(1.), Some(40009));
    }
    #[test]
    fn catchup_is_bounded_and_continues_at_head() {
        let mut h = History::default();
        h.configure(Source::Microphone, 8000).unwrap();
        assert_eq!(
            h.drain(Source::Microphone, 0, 200000, |_| Some([0.; 2]))
                .unwrap(),
            65536
        );
        assert_eq!(h.head(), 65536);
        assert_eq!(
            h.drain(Source::Microphone, 0, 200000, |_| Some([0.; 2]))
                .unwrap(),
            65536
        );
        assert_eq!(h.head(), 131072);
    }
    #[test]
    fn sample_rate_change_clears_and_bad_config_preserves() {
        let mut h = History::default();
        h.configure(Source::Microphone, 8000).unwrap();
        h.drain(Source::Microphone, 0, 10, |_| Some([1., 0.]))
            .unwrap();
        assert!(h.configure(Source::Player, 0).is_err());
        assert_eq!(h.valid_frames(), 10);
        h.configure(Source::Microphone, 16000).unwrap();
        assert_eq!(h.valid_frames(), 0);
        assert_eq!(h.head(), 0);
    }
}
