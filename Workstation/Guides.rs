//! Reconcile worker observations with stable, bounded presentation markers.
use serde_json::{Value, json};
use std::collections::BTreeSet;

const MATCH_SECONDS: f64 = 0.04;
const MAX_MARKERS: usize = 1024;
const MAX_REGIONS: usize = 512;
const MAX_SECONDS: f64 = 86_400.;

#[derive(Clone)]
struct Marker {
    id: String,
    time: f64,
    reveal: f64,
}

struct Region {
    data: Value,
    markers: Vec<Marker>,
}

fn number(data: &Value, key: &str, low: f64, high: f64) -> Result<f64, String> {
    data[key]
        .as_f64()
        .filter(|n| n.is_finite() && (low..=high).contains(n))
        .ok_or_else(|| format!("invalid rhythm guide {key}"))
}

fn beats(data: &Value) -> Result<Vec<f64>, String> {
    let rows = data["beat_seconds"]
        .as_array()
        .filter(|rows| !rows.is_empty() && rows.len() <= MAX_MARKERS)
        .ok_or("rhythm guide requires 1..1024 beat positions")?;
    let mut result = Vec::with_capacity(rows.len());
    for value in rows {
        let time = value
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=MAX_SECONDS).contains(n))
            .ok_or("invalid rhythm guide beat position")?;
        if result.last().is_some_and(|last| *last >= time) {
            return Err("rhythm guide beat positions must increase".into());
        }
        result.push(time);
    }
    number(data, "pulse_bpm", 1., 1000.)?;
    Ok(result)
}

fn old_region(data: &Value, observed: f64) -> Result<Region, String> {
    let times = beats(data)?;
    let rows = data["markers"].as_array();
    if rows.is_some_and(|rows| rows.len() != times.len()) {
        return Err("rhythm guide marker/beat count mismatch".into());
    }
    let mut markers = Vec::with_capacity(times.len());
    for (index, time) in times.into_iter().enumerate() {
        let (id, reveal) = if let Some(rows) = rows {
            let row = &rows[index];
            if (number(row, "time_seconds", 0., MAX_SECONDS)? - time).abs() > 1e-9 {
                return Err("rhythm guide marker/beat time mismatch".into());
            }
            let id = row["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or("invalid rhythm guide marker id")?
                .to_owned();
            (id, number(row, "reveal_at_seconds", 0., MAX_SECONDS)?)
        } else {
            // Accept an earlier document snapshot that predates marker-level
            // identities, retaining its original reveal time where present.
            let id = data["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256)
                .ok_or("legacy rhythm guide requires an id")?;
            let reveal = if data["reveal_at_seconds"].is_null() {
                observed
            } else {
                number(data, "reveal_at_seconds", 0., MAX_SECONDS)?
            };
            (format!("{id}:{index}"), reveal)
        };
        markers.push(Marker { id, time, reveal });
    }
    Ok(Region {
        data: data.clone(),
        markers,
    })
}

fn compatible(left: &Region, right: &Region) -> bool {
    let a = left.data["pulse_bpm"].as_f64().unwrap();
    let b = right.data["pulse_bpm"].as_f64().unwrap();
    if (a - b).abs() > a.max(b) * 0.02 {
        return false;
    }
    let gap = right.markers[0].time - left.markers.last().unwrap().time;
    let period = (60. / a + 60. / b) * 0.5;
    gap.abs() <= MATCH_SECONDS || (gap - period).abs() <= period * 0.12
}

fn render(mut region: Region, revision: u64) -> Value {
    let first = region.markers.first().unwrap();
    let last = region.markers.last().unwrap();
    region.data["id"] = json!(format!("region:{}", first.id));
    region.data["start_seconds"] = json!(first.time);
    region.data["end_seconds"] = json!(last.time);
    region.data["beat_seconds"] = json!(region.markers.iter().map(|m| m.time).collect::<Vec<_>>());
    region.data["markers"] = json!(
        region
            .markers
            .iter()
            .map(|m| json!({
                "id":m.id,"time_seconds":m.time,"reveal_at_seconds":m.reveal,
                "reveal_duration_seconds":0.25
            }))
            .collect::<Vec<_>>()
    );
    region.data["document_revision"] = json!(revision);
    // Pulse regularity never establishes a musical meter. Animation belongs
    // to individual markers so a rolling observation cannot restart a run.
    region.data["meter"] = Value::Null;
    if let Some(object) = region.data.as_object_mut() {
        object.remove("reveal_at_seconds");
        object.remove("reveal_duration_seconds");
    }
    region.data
}

pub(super) fn reconcile(
    old: &[Value],
    report: &Value,
    generation: u64,
    revision: u64,
    observed: f64,
) -> Result<Vec<Value>, String> {
    if !observed.is_finite() || !(0.0..=MAX_SECONDS).contains(&observed) {
        return Err("invalid rhythm guide observation time".into());
    }
    if old.len() > MAX_REGIONS {
        return Err("master rhythm region capacity exceeded".into());
    }
    let start = number(report, "window_start_seconds", 0., MAX_SECONDS)?;
    let end = number(report, "window_end_seconds", start, MAX_SECONDS)?;
    if end <= start {
        return Err("rhythm guide replacement requires a nonempty analyzed window".into());
    }
    let chunk = report["chunk_start_frame"]
        .as_u64()
        .ok_or("rhythm guide report requires chunk_start_frame")?;
    let incoming = report["regions"]
        .as_array()
        .filter(|regions| regions.len() <= MAX_REGIONS)
        .ok_or("invalid rhythm guide regions")?;
    let prior = old
        .iter()
        .map(|r| old_region(r, observed))
        .collect::<Result<Vec<_>, _>>()?;
    let mut prior_markers: Vec<_> = prior.iter().flat_map(|r| r.markers.iter()).collect();
    prior_markers.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.id.cmp(&b.id)));
    let mut reused = BTreeSet::new();
    let mut regions = Vec::new();
    for (region_index, data) in incoming.iter().enumerate() {
        let mut markers = Vec::new();
        for (beat_index, time) in beats(data)?.into_iter().enumerate() {
            if time < start || time > end {
                return Err("rhythm guide beat lies outside the analyzed window".into());
            }
            let first = prior_markers.partition_point(|m| m.time < time - MATCH_SECONDS);
            let previous = prior_markers[first..]
                .iter()
                .take_while(|m| m.time <= time + MATCH_SECONDS)
                .filter(|m| !reused.contains(&m.id))
                .min_by(|a, b| {
                    (a.time - time)
                        .abs()
                        .total_cmp(&(b.time - time).abs())
                        .then(a.id.cmp(&b.id))
                });
            let marker = if let Some(previous) = previous {
                reused.insert(previous.id.clone());
                Marker {
                    id: previous.id.clone(),
                    time,
                    reveal: previous.reveal,
                }
            } else {
                Marker {
                    id: format!("{generation}:{chunk}:{region_index}:{beat_index}"),
                    time,
                    reveal: observed,
                }
            };
            markers.push(marker);
        }
        regions.push(Region {
            data: data.clone(),
            markers,
        });
    }
    for old in prior {
        // The analyzed interval replaces only its own evidence. A refined
        // incoming boundary marker also replaces any retained copy of its ID.
        for before in [true, false] {
            let markers: Vec<_> = old
                .markers
                .iter()
                .filter(|m| {
                    !reused.contains(&m.id)
                        && if before {
                            m.time < start
                        } else {
                            m.time >= end
                        }
                })
                .cloned()
                .collect();
            if !markers.is_empty() {
                regions.push(Region {
                    data: old.data.clone(),
                    markers,
                });
            }
        }
    }
    regions.sort_by(|a, b| a.markers[0].time.total_cmp(&b.markers[0].time));
    let mut merged: Vec<Region> = Vec::new();
    for mut region in regions {
        if let Some(previous) = merged.last_mut() {
            // Adjacent observations can share a pulse, including tempo-change
            // boundaries. Keep one identity even if their BPMs cannot merge.
            while !region.markers.is_empty() {
                let last = previous.markers.last_mut().unwrap();
                let first = &region.markers[0];
                if (first.time - last.time).abs() > MATCH_SECONDS {
                    break;
                }
                if first.reveal < last.reveal {
                    *last = first.clone();
                }
                region.markers.remove(0);
            }
            if region.markers.is_empty() {
                continue;
            }
            if previous.markers.len() + region.markers.len() <= MAX_MARKERS
                && compatible(previous, &region)
            {
                previous.markers.extend(region.markers);
                let span = previous.markers.last().unwrap().time - previous.markers[0].time;
                if span > 0. {
                    previous.data["pulse_bpm"] =
                        json!(60. * (previous.markers.len() - 1) as f64 / span);
                }
                if let (Some(a), Some(b)) = (
                    previous.data["regularity_score"].as_f64(),
                    region.data["regularity_score"].as_f64(),
                ) {
                    previous.data["regularity_score"] = json!(a.min(b));
                }
                continue;
            }
        }
        merged.push(region);
    }
    if merged.len() > MAX_REGIONS {
        return Err("master rhythm region capacity reached".into());
    }
    Ok(merged
        .into_iter()
        .map(|region| render(region, revision))
        .collect())
}
