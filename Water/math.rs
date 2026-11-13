//! Continuous metre-space water helpers; no world occupancy/cell representation.
pub(crate) type V = [f64; 3];
pub(crate) type V2 = [f64; 2];
pub(crate) fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(crate) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
pub(crate) fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub(crate) fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn unit(a: V) -> V {
    mul(a, 1. / len(a).max(1e-15))
}
pub(crate) fn safe_unit(a: V, fallback: V) -> V {
    if len(a) > 1e-10 { unit(a) } else { fallback }
}
pub(crate) fn mix(a: V, b: V, t: f64) -> V {
    add(mul(a, 1. - t), mul(b, t))
}
pub(crate) fn finite(a: V) -> bool {
    a.iter().all(|x| x.is_finite())
}
pub(crate) fn dot2(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub(crate) fn unit2(a: V2) -> V2 {
    let l = dot2(a, a).sqrt();
    if l > 1e-12 {
        a.map(|n| n / l)
    } else {
        [0., -1.]
    }
}
pub(crate) fn noise(x: f64, z: f64) -> f64 {
    fn hash(x: i32, z: i32) -> f64 {
        let mut h = (x as u32).wrapping_mul(0x8da6b343) ^ (z as u32).wrapping_mul(0xd8163841);
        h ^= h >> 13;
        h = h.wrapping_mul(0x85ebca6b);
        h ^= h >> 16;
        (h & 0x00ffffff) as f64 / 8388607.5 - 1.
    }
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - x.floor();
    let fz = z - z.floor();
    let sx = fx * fx * (3. - 2. * fx);
    let sz = fz * fz * (3. - 2. * fz);
    let a = hash(ix, iz) * (1. - sx) + hash(ix.wrapping_add(1), iz) * sx;
    let b = hash(ix, iz.wrapping_add(1)) * (1. - sx)
        + hash(ix.wrapping_add(1), iz.wrapping_add(1)) * sx;
    a * (1. - sz) + b * sz
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub origin: V,
    pub right: V,
    pub up: V,
    pub forward: V,
}
impl Frame {
    pub fn radial(origin: V, heading: V) -> Self {
        let up = unit(origin);
        let tangent = sub(heading, mul(up, dot(up, heading)));
        let fallback = if up[2].abs() < 0.8 {
            [0., 0., 1.]
        } else {
            [1., 0., 0.]
        };
        let forward = safe_unit(tangent, unit(cross(fallback, up)));
        let right = unit(cross(up, forward));
        Self {
            origin,
            right,
            up,
            forward,
        }
    }
    pub fn local(&self, point: V) -> V {
        let d = sub(point, self.origin);
        [dot(d, self.right), dot(d, self.up), dot(d, self.forward)]
    }
    pub fn vector(&self, p: V) -> V {
        add(
            add(mul(self.right, p[0]), mul(self.up, p[1])),
            mul(self.forward, p[2]),
        )
    }
    pub fn world(&self, p: V) -> V {
        add(self.origin, self.vector(p))
    }
}
