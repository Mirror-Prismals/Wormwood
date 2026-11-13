//! Control-thread CC curve parsing and evaluation in fractional seconds.
//! Matches Cardinal's normalized, piecewise-linear automation curves.

use serde_json::Value;

const MAX_POINTS: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Point {
    pub(super) offset_seconds: f64,
    pub(super) value: f64,
}

pub(super) fn parse(input: &Value, duration: f64) -> Result<Vec<Point>, String> {
    if !duration.is_finite() || duration < 0.0 {
        return Err("CC duration must be finite and non-negative".into());
    }
    let points = input
        .as_array()
        .filter(|points| points.len() <= MAX_POINTS)
        .ok_or("CC points must be an array of at most 2048 points")?;
    let mut parsed = Vec::with_capacity(points.len());
    for point in points {
        let offset = point["offset_seconds"]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or("CC point offset_seconds must be finite")?;
        let value = point["value"]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or("CC point value must be finite")?;
        parsed.push(Point {
            offset_seconds: offset.clamp(0.0, duration),
            value: value.clamp(0.0, 1.0),
        });
    }
    // Both coordinates are finite. Numeric comparison preserves Cardinal's
    // equality of positive and negative zero when ordering coincident points.
    parsed.sort_by(|a, b| {
        a.offset_seconds
            .partial_cmp(&b.offset_seconds)
            .unwrap()
            .then_with(|| a.value.partial_cmp(&b.value).unwrap())
    });
    Ok(parsed)
}

/// Evaluate points ordered by `parse`, holding values at both endpoints.
/// Coincident points retain Cardinal's ordered interval/endpoint precedence.
pub(super) fn evaluate(points: &[Point], time: f64) -> f64 {
    let Some(first) = points.first() else {
        return 0.5;
    };
    if points.len() == 1 || time <= first.offset_seconds {
        return first.value.clamp(0.0, 1.0);
    }
    let last = points.last().unwrap();
    if time >= last.offset_seconds {
        return last.value.clamp(0.0, 1.0);
    }
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if time < a.offset_seconds || time > b.offset_seconds {
            continue;
        }
        if b.offset_seconds <= a.offset_seconds {
            return a.value.clamp(0.0, 1.0);
        }
        let fraction = (time - a.offset_seconds) / (b.offset_seconds - a.offset_seconds);
        return (a.value + (b.value - a.value) * fraction).clamp(0.0, 1.0);
    }
    last.value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{Point, evaluate, parse};
    use serde_json::{Value, json};

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
    }

    #[test]
    fn parsing_rejects_invalid_numbers_shapes_and_capacity() {
        for duration in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(parse(&json!([]), duration).is_err());
        }
        for invalid in [
            Value::Null,
            json!({}),
            json!([null]),
            json!([{"offset_seconds": null, "value": 0.5}]),
            json!([{"offset_seconds": 0.0, "value": "0.5"}]),
            json!([{"offset_seconds": 0.0}]),
            // JSON represents non-finite numeric values as null.
            json!([{"offset_seconds": f64::INFINITY, "value": 0.5}]),
            json!([{"offset_seconds": 0.0, "value": f64::NAN}]),
        ] {
            assert!(parse(&invalid, 1.0).is_err());
        }
        let point = json!({"offset_seconds": 0.0, "value": 0.5});
        assert_eq!(
            parse(&json!(vec![point.clone(); 2048]), 1.0).unwrap().len(),
            2048
        );
        assert!(parse(&json!(vec![point; 2049]), 1.0).is_err());
    }

    #[test]
    fn parsing_clamps_then_orders_time_and_value_without_quantizing() {
        let points = parse(
            &json!([
                {"offset_seconds": 4.0, "value": -0.5},
                {"offset_seconds": 0.123456789, "value": 0.75},
                {"offset_seconds": -2.0, "value": 1.5},
                {"offset_seconds": -0.0, "value": 0.25},
                {"offset_seconds": 2.0, "value": 0.5}
            ]),
            2.0,
        )
        .unwrap();
        assert_eq!(
            points,
            vec![
                Point {
                    offset_seconds: 0.0,
                    value: 0.25
                },
                Point {
                    offset_seconds: 0.0,
                    value: 1.0
                },
                Point {
                    offset_seconds: 0.123456789,
                    value: 0.75
                },
                Point {
                    offset_seconds: 2.0,
                    value: 0.0
                },
                Point {
                    offset_seconds: 2.0,
                    value: 0.5
                },
            ]
        );
        let zero = parse(&json!([{"offset_seconds": 9.0, "value": 0.2}]), 0.0).unwrap();
        assert_eq!(zero[0].offset_seconds, 0.0);
    }

    #[test]
    fn evaluation_interpolates_and_holds_endpoints() {
        assert_eq!(evaluate(&[], 4.0), 0.5);
        let single = [Point {
            offset_seconds: 3.0,
            value: 0.7,
        }];
        for time in [-1.0, 3.0, 9.0] {
            close(evaluate(&single, time), 0.7);
        }
        let points = parse(
            &json!([
                {"offset_seconds": 0.125, "value": 0.2},
                {"offset_seconds": 0.375, "value": 0.8}
            ]),
            1.0,
        )
        .unwrap();
        close(evaluate(&points, -1.0), 0.2);
        close(evaluate(&points, 0.125), 0.2);
        close(evaluate(&points, 0.1875), 0.35);
        close(evaluate(&points, 0.25), 0.5);
        close(evaluate(&points, 0.375), 0.8);
        close(evaluate(&points, 2.0), 0.8);
    }

    #[test]
    fn coincident_points_keep_cardinal_endpoint_and_interval_precedence() {
        let points = parse(
            &json!([
                {"offset_seconds": 0.0, "value": 0.4},
                {"offset_seconds": 0.0, "value": 0.2},
                {"offset_seconds": 1.0, "value": 0.9},
                {"offset_seconds": 1.0, "value": 0.3},
                {"offset_seconds": 2.0, "value": 0.8},
                {"offset_seconds": 2.0, "value": 0.6}
            ]),
            2.0,
        )
        .unwrap();
        close(evaluate(&points, 0.0), 0.2); // First endpoint wins.
        close(evaluate(&points, 0.5), 0.35); // Leave the later coincident point.
        close(evaluate(&points, 1.0), 0.3); // Enter the earlier coincident point.
        close(evaluate(&points, 1.5), 0.75);
        close(evaluate(&points, 2.0), 0.8); // Final endpoint wins.

        let coincident = parse(
            &json!([
                {"offset_seconds": 1.0, "value": 0.8},
                {"offset_seconds": 1.0, "value": 0.2}
            ]),
            2.0,
        )
        .unwrap();
        close(evaluate(&coincident, 1.0), 0.2);
        close(evaluate(&coincident, 1.1), 0.8);
    }
}
