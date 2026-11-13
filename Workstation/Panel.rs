//! Pure mapping between an explicit flat panel and a free-time document viewport.
//! Coordinates are independent of asset formats: +right increases u, +up
//! increases v, and normal selects the interactive front face. UV (0, 0) is
//! the lower-left corner of the centered safe rectangle, not a mesh UV atlas.
use serde_json::{Value, json};

type Vector = [f64; 3];
const MAX_COORDINATE: f64 = 1.0e9;
const MAX_SECONDS: f64 = 1.0e9;
const BASIS_TOLERANCE: f64 = 1.0e-6;

fn dot(a: Vector, b: Vector) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(v: Vector) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}

fn vector(input: &Value, key: &str) -> Result<Vector, String> {
    let values = input[key]
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| format!("{key} requires three finite coordinates"))?;
    let mut result = [0.; 3];
    for (result, value) in result.iter_mut().zip(values) {
        *result = value
            .as_f64()
            .filter(|value| value.is_finite() && value.abs() <= MAX_COORDINATE)
            .ok_or_else(|| format!("{key} exceeds the finite coordinate bounds"))?;
    }
    Ok(result)
}

fn number(input: &Value, key: &str, minimum: f64, maximum: f64) -> Result<f64, String> {
    input[key]
        .as_f64()
        .filter(|value| value.is_finite() && (minimum..=maximum).contains(value))
        .ok_or_else(|| format!("{key} must be finite and within {minimum}..={maximum}"))
}

struct Panel {
    origin: Vector,
    right: Vector,
    up: Vector,
    normal: Vector,
    width: f64,
    height: f64,
    start: f64,
    duration: f64,
}

impl Panel {
    fn parse(input: &Value) -> Result<Self, String> {
        if !input.is_object() {
            return Err("panel mapping requires an object".into());
        }
        let origin = vector(input, "origin")?;
        let mut right = vector(input, "right")?;
        let mut up = vector(input, "up")?;
        let normal = vector(input, "normal")?;
        for axis in [right, up, normal] {
            if (length(axis) - 1.).abs() > BASIS_TOLERANCE {
                return Err("panel basis axes must be unit length".into());
            }
        }
        if [dot(right, up), dot(right, normal), dot(up, normal)]
            .iter()
            .any(|value| value.abs() > BASIS_TOLERANCE)
        {
            return Err("panel basis axes must be perpendicular".into());
        }
        // Remove only accepted floating-point drift so inverse mappings share
        // an exactly orthonormal plane. The supplied normal retains its side.
        let right_length = length(right);
        right = right.map(|component| component / right_length);
        let along_right = dot(up, right);
        up = std::array::from_fn(|axis| up[axis] - right[axis] * along_right);
        let up_length = length(up);
        up = up.map(|component| component / up_length);
        let mut perpendicular = cross(right, up);
        if dot(perpendicular, normal) < 0. {
            perpendicular = perpendicular.map(|component| -component);
        }
        let width = number(input, "width_m", 1.0e-4, 1.0e4)?;
        let height = number(input, "height_m", 1.0e-4, 1.0e4)?;
        let start = number(input, "start_seconds", -MAX_SECONDS, MAX_SECONDS)?;
        let duration = number(input, "seconds_per_width", 1.0e-6, MAX_SECONDS)?;
        if start + duration > MAX_SECONDS {
            return Err("panel viewport end exceeds the time bounds".into());
        }
        // Every point in the safe rectangle must remain within the same
        // coordinate domain accepted for ray origins and panel origins.
        if (0..3).any(|axis| {
            origin[axis].abs() + right[axis].abs() * width * 0.5 + up[axis].abs() * height * 0.5
                > MAX_COORDINATE
        }) {
            return Err("panel safe rectangle exceeds the coordinate bounds".into());
        }
        Ok(Self {
            origin,
            right,
            up,
            normal: perpendicular,
            width,
            height,
            start,
            duration,
        })
    }

    fn mapped(&self, u: f64, v: f64) -> Value {
        let x = (u - 0.5) * self.width;
        let y = (v - 0.5) * self.height;
        let point: Vector = std::array::from_fn(|axis| {
            self.origin[axis] + self.right[axis] * x + self.up[axis] * y
        });
        json!({
            "uv": [u, v],
            "local_m": [x, y],
            "world_position": point,
            "time_seconds": self.start + u * self.duration
        })
    }
}

/// Locate a front-facing ray on the panel's safe control rectangle.
/// Requests use origin/right/up/normal, width_m/height_m,
/// start_seconds/seconds_per_width, and ray_origin/direction. A geometric
/// miss returns null; malformed vectors or viewport bounds return an error.
pub(super) fn locate(input: &Value) -> Result<Value, String> {
    let panel = Panel::parse(input)?;
    let ray_origin = vector(input, "ray_origin")?;
    let direction = vector(input, "direction")?;
    let magnitude = length(direction);
    if magnitude < 1.0e-12 {
        return Err("panel ray direction must be nonzero".into());
    }
    let direction = direction.map(|component| component / magnitude);
    let denominator = dot(direction, panel.normal);
    if denominator >= -1.0e-9 {
        // Parallel rays and back-face approaches.
        return Ok(Value::Null);
    }
    let to_plane: Vector = std::array::from_fn(|axis| panel.origin[axis] - ray_origin[axis]);
    let distance = dot(to_plane, panel.normal) / denominator;
    if !distance.is_finite() || !(0.0..=4.0 * MAX_COORDINATE).contains(&distance) {
        return Ok(Value::Null);
    }
    // Subtract the nearby plane origin before taking panel-axis dot products;
    // avoid an unnecessary large world-coordinate addition and subtraction.
    let relative: Vector = std::array::from_fn(|axis| direction[axis] * distance - to_plane[axis]);
    let u = dot(relative, panel.right) / panel.width + 0.5;
    let v = dot(relative, panel.up) / panel.height + 0.5;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return Ok(Value::Null);
    }
    let mut result = panel.mapped(u, v);
    result["distance_m"] = json!(distance);
    Ok(result)
}

/// Map a document time and normalized vertical position onto the same plane.
/// A valid point outside the visible time/vertical interval returns null.
pub(super) fn project(input: &Value) -> Result<Value, String> {
    let panel = Panel::parse(input)?;
    let time = number(input, "time_seconds", -MAX_SECONDS, MAX_SECONDS)?;
    let v = number(input, "v", -MAX_COORDINATE, MAX_COORDINATE)?;
    let u = (time - panel.start) / panel.duration;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return Ok(Value::Null);
    }
    Ok(panel.mapped(u, v))
}
