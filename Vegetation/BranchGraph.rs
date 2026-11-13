use super::{PlantState, Site, math::*, profiles::Profile};
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Branch {
    pub id: u32,
    pub parent: Option<u32>,
    pub start: V,
    pub end: V,
    pub radius: f64,
    pub order: u8,
}
#[derive(Clone, Debug)]
pub struct Tree {
    pub site: Site,
    pub height: f64,
    pub crown_radius: f64,
    pub branches: Vec<Branch>,
    pub frame: Option<[V; 3]>,
}
impl Tree {
    pub fn generate(site: Site, state: &PlantState, profile: Profile) -> Self {
        let maturity =
            (1. - (-state.age_years.max(0.) / profile.growth_years).exp()).clamp(0.04, 1.);
        let h = profile.mature_height * (0.78 + random(site.id, 9) * 0.40) * maturity;
        let spread = h * profile.crown_spread;
        let mut branches = Vec::new();
        let lean = [
            (random(site.id, 12) - 0.5) * h * 0.08,
            0.,
            (random(site.id, 13) - 0.5) * h * 0.08,
        ];
        for i in 0..4 {
            let a = i as f64 / 4.;
            let b = (i + 1) as f64 / 4.;
            branches.push(Branch {
                id: i,
                parent: if i == 0 { None } else { Some(i - 1) },
                start: [lean[0] * a * a, h * a, lean[2] * a * a],
                end: [lean[0] * b * b, h * b, lean[2] * b * b],
                radius: h * 0.019 * (1. - a * 0.83),
                order: 0,
            });
        }
        for tier in 0..3 {
            for arm in 0..3 {
                let k = tier * 3 + arm;
                let a = random(site.id, 18) * std::f64::consts::TAU + k as f64 * 2.39996323;
                let y = h * (0.36 + tier as f64 * 0.16 + random(site.id, 30 + k) * 0.07);
                let r =
                    spread * (0.92 - tier as f64 * 0.15) * (0.8 + random(site.id, 50 + k) * 0.28);
                let start = [lean[0] * (y / h).powi(2), y, lean[2] * (y / h).powi(2)];
                let end = add(start, [a.cos() * r, r * profile.branch_rise, a.sin() * r]);
                let id = branches.len() as u32;
                let branch = Branch {
                    id,
                    parent: Some(((y / h * 4.).floor() as u32).min(3)),
                    start,
                    end,
                    radius: h * 0.007 * (1. - tier as f64 * 0.18),
                    order: 1,
                };
                branches.push(branch);
                for twig in 0..2 {
                    let t = 0.58 + twig as f64 * 0.30;
                    let start = add(start, mul(sub(end, start), t));
                    let angle = a + if twig == 0 { -0.70 } else { 0.62 };
                    let end = add(
                        start,
                        [angle.cos() * r * 0.43, r * 0.30, angle.sin() * r * 0.43],
                    );
                    branches.push(Branch {
                        id: branches.len() as u32,
                        parent: Some(id),
                        start,
                        end,
                        radius: h * 0.0025,
                        order: 2,
                    });
                }
            }
        }
        if let Some(asset) = super::species::asset(profile) {
            // Same graph is edited/exported in Blender and used for wind, wood and perches.
            let angle=random(site.id,18)*std::f64::consts::TAU;
            let transform=|p:V| [h*(p[0]*angle.cos()-p[2]*angle.sin()),h*p[1],h*(p[0]*angle.sin()+p[2]*angle.cos())];
            branches=asset.branches.iter().map(|b| Branch {id:b.id,parent:b.parent,start:transform(b.start),end:transform(b.end),radius:b.radius*h,order:b.order}).collect();
        }
        // Descendants of a broken branch are absent in every consumer, including perches.
        let mut removed = state.broken_branches.clone();
        for b in &branches {
            if b.parent.is_some_and(|p| removed.contains(&p)) {
                removed.insert(b.id);
            }
        }
        branches.retain(|b| !removed.contains(&b.id));
        Self {
            site,
            height: h,
            crown_radius: spread,
            branches,
            frame: None,
        }
    }
    pub fn world(&self, local: V) -> V {
        if let Some(frame) = self.frame {
            return add(self.site.origin, add(add(mul(frame[0], local[0]), mul(frame[1], local[1])), mul(frame[2], local[2])));
        }
        let (e, n) = basis(self.site.normal);
        add(
            self.site.origin,
            add(
                add(mul(e, local[0]), mul(self.site.normal, local[1])),
                mul(n, local[2]),
            ),
        )
    }
    pub fn world_direction(&self, local: V) -> V {
        if let Some(frame) = self.frame {
            return add(add(mul(frame[0],local[0]),mul(frame[1],local[1])),mul(frame[2],local[2]));
        }
        let (e, n) = basis(self.site.normal);
        add(
            add(mul(e, local[0]), mul(self.site.normal, local[1])),
            mul(n, local[2]),
        )
    }
    pub fn wind(&self, point: V, time: f64) -> V {
        if self.frame.is_some() { return point; }
        // Root remains fixed; the same continuous displacement field moves wood,
        // attached leaves and queried perches. Flutter is a separate leaf rotation.
        let t = (point[1] / self.height.max(0.1)).clamp(0., 1.);
        let phase = random(self.site.id, 77) * 6.283185307;
        let gust = (time * 0.83 + phase).sin() * 0.65 + (time * 1.71 + phase * 0.7).sin() * 0.35;
        add(
            point,
            [
                self.height * 0.009 * t * t * gust,
                0.,
                self.height * 0.004 * t * t * (time * 0.63 + phase).sin(),
            ],
        )
    }
    pub fn wind_normal(&self, point: V, normal: V, time: f64) -> V {
        if self.frame.is_some() { return unit(normal); }
        // wind() is constant in height outside its clamped bending interval.
        if point[1] <= 0. || point[1] >= self.height.max(0.1) {
            return unit(normal);
        }
        let t = (point[1] / self.height.max(0.1)).clamp(0., 1.);
        let phase = random(self.site.id, 77) * 6.283185307;
        let gust = (time * 0.83 + phase).sin() * 0.65 + (time * 1.71 + phase * 0.7).sin() * 0.35;
        // Inverse transpose of the analytic wind deformation Jacobian.
        let dxdy = 0.018 * t * gust;
        let dzdy = 0.008 * t * (time * 0.63 + phase).sin();
        unit([
            normal[0],
            normal[1] - dxdy * normal[0] - dzdy * normal[2],
            normal[2],
        ])
    }
}
