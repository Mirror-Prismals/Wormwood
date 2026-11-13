//! Cardinal DawWarpSystem.cpp map semantics. BPM is REQUIRED from the transport
//! owner; this module never invents a global or clip-local clock for audio.
use super::warp_dsp::Anchor;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Marker {
    pub beat: f64,
    pub source: f64,
    pub manual: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct Map {
    pub enabled: bool,
    pub transpose_semitones: f64,
    markers: Vec<Marker>,
}
impl Map {
    pub(crate) fn from_markers(markers:Vec<Marker>)->Result<Self,String>{let mut map=Self{enabled:false,transpose_semitones:0.,markers:Vec::new()};map.replace(markers)?;Ok(map)}
    pub(crate) fn new(
        source_offset: u64,
        frames: u64,
        bpm: f64,
        rate: u32,
    ) -> Result<Self, String> {
        clock(bpm, rate)?;
        if frames == 0 || source_offset.checked_add(frames).is_none() {
            return Err("invalid source range".into());
        }
        Ok(Self {
            enabled: false,
            transpose_semitones: 0.,
            markers: vec![
                Marker {
                    beat: 0.,
                    source: source_offset as f64,
                    manual: true,
                },
                Marker {
                    beat: (frames as f64 * bpm / (60. * rate as f64)).max(1e-9),
                    source: (source_offset + frames) as f64,
                    manual: true,
                },
            ],
        })
    }
    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }
    pub(crate) fn replace(&mut self, mut markers: Vec<Marker>) -> Result<(), String> {
        if !(2..=4096).contains(&markers.len()) {
            return Err("warp map requires2..4096markers".into());
        }
        if markers
            .iter()
            .any(|m| !m.beat.is_finite() || !m.source.is_finite() || m.source < 0.)
        {
            return Err("warp markers require finite non-negative source positions".into());
        }
        markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        if markers
            .windows(2)
            .any(|m| m[1].beat <= m[0].beat || m[1].source <= m[0].source)
        {
            return Err("warp markers must increase in beat and source order".into());
        }
        self.markers = markers;
        Ok(())
    }
    pub(crate) fn add(&mut self, marker: Marker) -> Result<(), String> {
        let mut next = self.markers.clone();
        next.push(marker);
        self.replace(next)
    }
    pub(crate) fn move_marker(
        &mut self,
        index: usize,
        beat: f64,
        source: f64,
    ) -> Result<(), String> {
        let mut next = self.markers.clone();
        let marker = next.get_mut(index).ok_or("unknown warp marker")?;
        marker.beat = beat;
        marker.source = source;
        self.replace(next)
    }
    pub(crate) fn remove(&mut self, index: usize) -> Result<(), String> {
        if self.markers.len() <= 2 || index >= self.markers.len() {
            return Err("cannot remove warp endpoint pair or unknown marker".into());
        }
        let mut next = self.markers.clone();
        next.remove(index);
        self.replace(next)
    }
    pub(crate) fn source_at_beat(&self, beat: f64) -> f64 {
        let i = self
            .markers
            .partition_point(|m| m.beat <= beat)
            .clamp(1, self.markers.len() - 1);
        let a = self.markers[i - 1];
        let b = self.markers[i];
        a.source + (b.source - a.source) * (beat - a.beat) / (b.beat - a.beat)
    }
    pub(crate) fn beat_at_source(&self, source: f64) -> f64 {
        let i = self
            .markers
            .partition_point(|m| m.source <= source)
            .clamp(1, self.markers.len() - 1);
        let a = self.markers[i - 1];
        let b = self.markers[i];
        a.beat + (b.beat - a.beat) * (source - a.source) / (b.source - a.source)
    }
    pub(crate) fn source_at_local_frame(
        &self,
        source_offset: f64,
        local_frame: f64,
        bpm: f64,
        rate: u32,
    ) -> Result<f64, String> {
        clock(bpm, rate)?;
        Ok(if self.enabled {
            self.source_at_beat(
                self.beat_at_source(source_offset) + local_frame * bpm / (60. * rate as f64),
            )
        } else {
            source_offset + local_frame
        })
    }
    pub(crate) fn local_frame_at_source(
        &self,
        source_offset: f64,
        source: f64,
        bpm: f64,
        rate: u32,
    ) -> Result<f64, String> {
        clock(bpm, rate)?;
        Ok(if self.enabled {
            (self.beat_at_source(source) - self.beat_at_source(source_offset)) * 60. * rate as f64
                / bpm
        } else {
            source - source_offset
        })
    }
    /// Caller applies all returned lengths atomically to every linked clip sharing
    /// this map, exactly as SetClipWarpEnabled preserves source ranges in Cardinal.
    pub(crate) fn toggle_ranges(
        &mut self,
        enabled: bool,
        ranges: &[(u64, u64)],
        bpm: f64,
        rate: u32,
    ) -> Result<Vec<u64>, String> {
        clock(bpm, rate)?;
        let ends: Vec<f64> = ranges
            .iter()
            .map(|(offset, length)| {
                self.source_at_local_frame(*offset as f64, *length as f64, bpm, rate)
            })
            .collect::<Result<_, _>>()?;
        let mut next = self.clone();
        next.enabled = enabled;
        let mut lengths = Vec::with_capacity(ranges.len());
        for ((offset, _), end) in ranges.iter().zip(ends) {
            let length = next.local_frame_at_source(*offset as f64, end, bpm, rate)?;
            if !length.is_finite() || length <= 0. || length >= u64::MAX as f64 {
                return Err("invalid toggled warp extent".into());
            }
            lengths.push((length.round() as u64).max(1));
        }
        self.enabled = enabled;
        Ok(lengths)
    }
    /// Convert authored beat markers to the exact sample schedule consumed by the
    /// worker. Spectral-treated source PCM must be supplied BEFORE this render.
    pub(crate) fn anchors(
        &self,
        source_offset: u64,
        bpm: f64,
        rate: u32,
    ) -> Result<Vec<Anchor>, String> {
        clock(bpm, rate)?;
        let beat = self.beat_at_source(source_offset as f64);
        let samples_per_beat = 60. * rate as f64 / bpm;
        Ok(self
            .markers
            .iter()
            .map(|m| Anchor {
                destination: (m.beat - beat) * samples_per_beat,
                source: m.source,
            })
            .collect())
    }
    pub(crate) fn pitch(&self) -> Result<f64, String> {
        if !self.transpose_semitones.is_finite() || self.transpose_semitones.abs() > 24. {
            return Err("warp pitch must be within +/-24semitones".into());
        }
        Ok(2f64.powf(self.transpose_semitones / 12.))
    }
}
fn clock(bpm: f64, rate: u32) -> Result<(), String> {
    if !bpm.is_finite() || bpm < 1. || !(8000..=192000).contains(&rate) {
        Err("an authoritative positive BPM and sample rate are required".into())
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_default_map_is_identity_at_creation_clock() {
        let mut m = Map::new(1000, 48000, 120., 48000).unwrap();
        m.enabled = true;
        assert_eq!(
            m.source_at_local_frame(1000., 12345., 120., 48000).unwrap(),
            13345.
        );
        assert_eq!(m.anchors(1000, 120., 48000).unwrap()[0].destination, 0.);
    }
    #[test]
    fn enabling_and_disabling_preserve_linked_source_ends() {
        let mut m = Map::new(1000, 48000, 120., 48000).unwrap();
        let lengths = m
            .toggle_ranges(true, &[(1000, 48000), (25000, 24000)], 60., 48000)
            .unwrap();
        assert_eq!(lengths, [96000, 48000]);
        let lengths = m
            .toggle_ranges(false, &[(1000, 96000), (25000, 48000)], 60., 48000)
            .unwrap();
        assert_eq!(lengths, [48000, 24000]);
    }
    #[test]
    fn failed_crossing_marker_edit_is_atomic() {
        let mut m = Map::new(0, 48000, 120., 48000).unwrap();
        m.add(Marker {
            beat: 1.,
            source: 24000.,
            manual: true,
        })
        .unwrap();
        let before = m.markers.clone();
        assert!(m.move_marker(1, 0., 25000.).is_err());
        assert_eq!(m.markers, before);
    }
    #[test]
    fn map_roundtrips_and_extrapolates_trimmed_ranges() {
        let m = Map::new(4000, 48000, 96., 48000).unwrap();
        for frame in [-1000., 4000., 30000., 80000.] {
            assert!((m.source_at_beat(m.beat_at_source(frame)) - frame).abs() < 1e-8);
        }
    }
}
