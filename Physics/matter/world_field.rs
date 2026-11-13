//! The unstrained material world. Landform/cave functions supply occupied volume;
//! this module assigns its substance. Refinement changes resolution, not material.
//! No atlas indices, biome textures, workshop specimen outlines or particle effects.
use super::{material::Kind, *};
use crate::terrain;
type D = [f64; 3];

pub(crate) const TURF_DEPTH: f64 = 0.085;
pub(crate) const TURF_COLOR: V = [0.21, 0.39, 0.17];
pub(crate) const ROOT_COLOR: V = [0.29, 0.21, 0.11];
pub(crate) const STONE_COLOR: V = terrain::rock::BROWN;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Substance {
    Turf,
    Dirt,
    FineSand,
    Snow,
    Stone,
    Bedrock,
    Water,
}
impl Substance {
    pub fn name(self) -> &'static str {
        match self {
            Self::Turf => "turf",
            Self::Dirt => "dirt",
            Self::FineSand => "fine sand",
            Self::Snow => "snow",
            Self::Stone => "stone",
            Self::Bedrock => "bedrock",
            Self::Water => "water",
        }
    }
    pub fn color_at(self, point: D) -> V {
        if self==Self::Turf && terrain::regions::region(point)==terrain::regions::Region::Jungle {terrain::sample(point).color.map(|v|v as f32)} else if self == Self::FineSand && terrain::nightworld::active(point) {terrain::nightworld::color(point)} else if self == Self::Stone { terrain::rock::color(point) } else { self.color() }
    }
    pub fn color(self) -> V {
        match self {
            Self::Turf => TURF_COLOR,
            Self::Dirt => Kind::Dirt.law().color,
            Self::FineSand => Kind::FineSand.law().color,
            Self::Snow => Kind::Snow.law().color,
            Self::Stone => STONE_COLOR,
            Self::Bedrock => [0.19, 0.21, 0.24],
            Self::Water => Kind::Water.law().color,
        }
    }
}

/// Material depth is measured from the undisturbed landform, including its
/// carved drainage bed. A cave is a void in this volume, never another texture.
pub(crate) fn substance(p: D) -> Substance {
    let r = p.iter().map(|v| v * v).sum::<f64>().sqrt();
    let up = p.map(|v| v / r.max(1e-9));
    let elevation = r - terrain::RADIUS;
    if elevation <= terrain::BEDROCK_ELEVATION + 0.025 {
        return Substance::Bedrock;
    }
    let sample = terrain::sample(up);
    let depth = sample.height - elevation;
    if terrain::nightworld::active(p) {return if depth<8. {Substance::FineSand}else{Substance::Stone};}
    if (terrain::regions::polar(up) || terrain::regions::region(up)==terrain::regions::Region::Snow) && depth < sample.soil_depth { return Substance::Snow; }
    if terrain::regions::sandy(up) && depth < sample.soil_depth { return Substance::FineSand; }
    if depth > sample.soil_depth || sample.soil_depth < 0.20 {
        return Substance::Stone;
    }
    // Sediment occupies a real layer beneath accepted water bodies. Both the
    // rendered bed and the granular solver obtain this same classification.
    if terrain::drainage::water(up).is_some_and(|w| w.level > sample.height + 0.02) {
        return if depth < sample.soil_depth.min(0.45) {
            Substance::FineSand
        } else {
            Substance::Dirt
        };
    }
    if depth <= TURF_DEPTH {
        Substance::Turf
    } else {
        Substance::Dirt
    }
}

#[derive(Clone)]
pub(crate) struct Field {
    pub caves: Option<terrain::underground::Field>,
    pub domains: Vec<terrain::material_domain::Domain>,
}
thread_local! {
    static AIM_FIELD:std::cell::RefCell<Option<(Value,Value,std::sync::Arc<Field>)>>=const{std::cell::RefCell::new(None)};
}
impl Field {
    /// The aiming ray changes every frame; its geological field does not.
    /// Retain one immutable parsed field and invalidate on actual world edits.
    pub fn shared_for_aim(surface:&Value)->Result<std::sync::Arc<Self>,String>{
        AIM_FIELD.with(|slot|{
            let mut cache=slot.borrow_mut();
            if let Some((caves,edits,field))=&*cache {if caves==&surface["underground"]&&edits==&surface["edits"]{return Ok(field.clone());}}
            let field=std::sync::Arc::new(Self::from_surface(surface)?);
            *cache=Some((surface["underground"].clone(),surface["edits"].clone(),field.clone()));Ok(field)
        })
    }

    pub fn from_surface(surface: &Value) -> Result<Self, String> {
        let mut caves = terrain::underground::Field::parse(&surface["underground"])?;
        // Domain ownership is applied once, outside the geological field.
        if let Some(c) = &mut caves {
            c.set_material_domains(&[]);
        }
        let domains = terrain::material_domain::from_edits(
            surface["edits"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        );
        Ok(Self { caves, domains })
    }
    pub fn rest_distance(&self, p: D) -> f64 {
        let r = p.iter().map(|v| v * v).sum::<f64>().sqrt();
        let h = terrain::height(p.map(|v| v / r.max(1e-9)));
        self.caves
            .as_ref()
            .map_or(r - terrain::RADIUS - h, |c| c.distance(p, h))
            .min(r - terrain::RADIUS - terrain::BEDROCK_ELEVATION)
    }
    pub fn distance(&self, p: D) -> f64 {
        terrain::material_domain::carve_bedrock(p,self.rest_distance(p),&self.domains)
    }
    pub fn boundary(&self, mut p: D) -> (D, D) {
        // Cave scores need not be unit signed distances. Project using the
        // actual gradient rather than multiplying their value by a unit normal.
        for _ in 0..8 {
            let value = self.distance(p);
            if value.abs() < 0.0001 {
                break;
            }
            let gradient: D = std::array::from_fn(|k| {
                let mut a = p;
                let mut b = p;
                a[k] += 0.004;
                b[k] -= 0.004;
                (self.distance(a) - self.distance(b)) / 0.008
            });
            let square = gradient.iter().map(|v| v * v).sum::<f64>().max(1e-12);
            for k in 0..3 {
                p[k] -= (value * gradient[k] / square).clamp(-0.5, 0.5);
            }
        }
        (p, self.normal(p))
    }
    pub fn normal(&self, p: D) -> D {
        let n: D = std::array::from_fn(|k| {
            let mut a = p;
            let mut b = p;
            a[k] += 0.004;
            b[k] -= 0.004;
            self.distance(a) - self.distance(b)
        });
        let l = n.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        n.map(|v| v / l)
    }
    pub fn refined(&self, p: D) -> bool {
        self.domains.iter().any(|d| d.distance(p) < 0.002)
    }
}

/// The same crown construction is used by resting landscape turf and by the
/// deforming root mat. The latter supplies the current tissue frame and origin.
pub(crate) fn grass_crown(
    base: V,
    normal: V,
    axis: V,
    salt: i32,
    leaves: usize,
    mut emit: impl FnMut([V; 3], V),
) {
    let other = unit(cross(normal, axis));
    for leaf in 0..leaves {
        let seed = salt.wrapping_add((leaf as i32).wrapping_mul(137));
        let angle = (noise(seed) + leaf as f32 * 0.618) * std::f32::consts::TAU;
        let direction = add(scale(axis, angle.cos()), scale(other, angle.sin()));
        let side = unit(cross(direction, normal));
        let height = 0.044 + 0.018 * (noise(seed.wrapping_add(23)) + 1.);
        let width = 0.0022 + 0.0006 * noise(seed.wrapping_add(19));
        let middle = add(
            base,
            add(
                scale(normal, height * 0.65),
                scale(direction, height * 0.18),
            ),
        );
        let tip = add(
            base,
            add(
                scale(normal, height * 0.87),
                scale(direction, height * 0.52),
            ),
        );
        let [a, b] = [-1., 1.].map(|s| add(base, scale(side, s * width)));
        let [c, d] = [-1., 1.].map(|s| add(middle, scale(side, s * width * 0.7)));
        let shade = 0.92 + 0.15 * noise(seed.wrapping_add(7));
        let color = [0.31 * shade, 0.52 * shade, 0.19 * shade];
        for tri in [[a, b, c], [b, d, c], [c, d, tip]] {
            emit(tri, color);
        }
    }
}

#[cfg(test)]mod aim_field_tests {
 use super::*;
 #[test]fn aim_field_reuses_immutable_geology_and_invalidates_edits(){
  let cave:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
  let mut surface=serde_json::json!({"underground":cave,"edits":[]});
  let a=Field::shared_for_aim(&surface).unwrap();let b=Field::shared_for_aim(&surface).unwrap();assert!(std::sync::Arc::ptr_eq(&a,&b));
  surface["edits"]=serde_json::json!([{"normal":[0.,1.,0.],"radius":4.,"depth":1.}]);let c=Field::shared_for_aim(&surface).unwrap();assert!(!std::sync::Arc::ptr_eq(&a,&c));
  let reference=Field::from_surface(&surface).unwrap();for x in -8..8 {for y in [-50.,-9.,1.,15.]{let p=terrain::underground::world([x as f64,y,-13.]);assert_eq!(c.distance(p).to_bits(),reference.distance(p).to_bits());}}
  surface["underground"]["enabled"]=serde_json::json!(false);let d=Field::shared_for_aim(&surface).unwrap();assert!(!std::sync::Arc::ptr_eq(&c,&d));assert!(d.caves.is_none());

 }
}
