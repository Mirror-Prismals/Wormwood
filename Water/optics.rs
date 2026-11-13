//! Cardinal's water optical equations, separated from cell/prototype data.
//! Used as the explicit material contract for native water pipelines.
use super::math::*;
pub(crate) type Rgb = [f64; 3];
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    pub murkiness: f64,
    pub sky_color_intensity: f64,
    pub neutral_reflection_intensity: f64,
    pub sky_top: Rgb,
    pub sky_bottom: Rgb,
    pub light: V,
    pub planar_reflection: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            murkiness: 0.,
            sky_color_intensity: 1.,
            neutral_reflection_intensity: 1.,
            sky_top: [0.52, 0.66, 0.95],
            sky_bottom: [0.03, 0.06, 0.18],
            light: unit([0.35, 1., 0.25]),
            planar_reflection: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Surface {
    pub ray: V,
    pub shading_normal: V,
    pub geometric_normal: V,
    pub axis_normal: V,
    pub up: V,
    pub vertical_depth: f64,
    pub thickness: f64,
    pub wave_class: u32,
    pub flow: V2,
    pub uv_meters: V2,
    pub covered: bool,
}
fn reflected(ray: V, normal: V) -> V {
    sub(ray, mul(normal, 2. * dot(ray, normal)))
}
fn orient(normal: V, ray: V) -> V {
    let normal = unit(normal);
    if dot(normal, mul(ray, -1.)) < 0. {
        mul(normal, -1.)
    } else {
        normal
    }
}
pub(crate) fn refracted(ray: V, normal: V, eta: f64) -> V {
    let c = dot(ray, normal);
    let k = (1. - eta * eta * (1. - c * c)).max(0.);
    sub(mul(ray, eta), mul(normal, eta * c + k.sqrt()))
}
pub(crate) fn optical_depth(surface: &Surface) -> f64 {
    let ray = unit(surface.ray);
    let n = orient(surface.geometric_normal, ray);
    let axis = orient(surface.axis_normal, ray);
    let bend = refracted(ray, n, 1. / 1.33);
    let depth = if dot(n, surface.up).abs() > 0.05 {
        surface.vertical_depth / dot(bend, surface.up).abs().max(0.12)
    } else {
        surface.thickness / dot(bend, axis).abs().max(0.12)
    };
    depth.clamp(0.05, 24.)
}
pub(crate) fn shade(
    settings: &Settings,
    surface: &Surface,
    time: f64,
    red_sample: Rgb,
    blue_sample: Rgb,
    undistorted_sample: Rgb,
    planar_sample: Option<Rgb>,
) -> Rgb {
    let ray = unit(surface.ray);
    let normal = orient(surface.shading_normal, ray);
    let depth = optical_depth(surface);
    let local = if surface.covered {
        undistorted_sample
    } else {
        [
            red_sample[0],
            0.5 * (red_sample[1] + blue_sample[1]),
            blue_sample[2],
        ]
    };
    let absorb =
        [0.5, 0.25, 0.1].map(|n: f64| (-depth * settings.murkiness.clamp(0., 1.) * n).exp());
    let refracted = std::array::from_fn(|i| local[i] * absorb[i] + (1. - absorb[i]) * 0.15);
    let reflection = reflected(ray, normal);
    let elevation = dot(reflection, surface.up).max(0.);
    let neutral = mix([0.88, 0.93, 0.98], [0.98, 0.99, 1.], elevation);
    let live = mix(settings.sky_bottom, settings.sky_top, elevation);
    let live_weight = settings.sky_color_intensity.clamp(0., 1.);
    let neutral_weight = (1. - live_weight) * settings.neutral_reflection_intensity.clamp(0., 1.);
    let mut opacity = live_weight + neutral_weight;
    let mut sky = mul(
        add(mul(neutral, neutral_weight), mul(live, live_weight)),
        1. / opacity.max(0.0001),
    );
    if settings.planar_reflection && !surface.covered {
        if let Some(sample) = planar_sample {
            sky = mix(sky, sample, 0.6);
            opacity = opacity * 0.4 + 0.6;
        }
    }
    // Preserve reflected colors, matching the native shader variants.
    let fresnel = 0.02 + 0.98 * (1. - dot(mul(ray, -1.), normal).max(0.)).powi(5);
    let spec = dot(reflection, unit(settings.light)).max(0.).powf(128.) * 0.5;
    let mut col = add(
        mix(refracted, sky, (fresnel * opacity).clamp(0., 1.)),
        [spec; 3],
    );
    if surface.wave_class == 3 && dot2(surface.flow, surface.flow).sqrt() > 0.1 {
        let direction = unit2(surface.flow);
        let across = [-direction[1], direction[0]];
        let along = dot2(surface.uv_meters, direction);
        let across = dot2(surface.uv_meters, across);
        let broad = (along * 0.82 - time * 2.15 + (across * 0.31).sin() * 0.75).sin();
        let fine = (along * 1.73 - time * 3.41 + (across * 0.57 + time * 0.19).sin()).sin();
        col = col.map(|n| (n * (1. + (broad * 0.65 + fine * 0.35) * 0.065)).clamp(0., 1.));
    }
    col
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ripple {
    pub origin: V2,
    pub age: f64,
    pub strength: f64,
}
pub(crate) fn ripple_height(p: V2, ripples: &[Ripple]) -> f64 {
    ripples
        .iter()
        .take(64)
        .map(|r| {
            let distance = (p[0] - r.origin[0]).hypot(p[1] - r.origin[1]);
            let offset = distance - r.age.max(0.) * 2.65;
            (offset * 10.5).sin()
                * (-offset * offset * 3.4).exp()
                * (-r.age.max(0.) * 0.72).exp()
                * r.strength.max(0.)
                * 0.055
        })
        .sum()
}

#[cfg(test)]
mod clear_water_tests {
 use super::*;
 #[test]
 fn grazing_reflection_remains_visible_without_a_second_opacity_blend(){
  let surface=Surface{ray:unit([1.,-0.02,0.]),shading_normal:[0.,1.,0.],geometric_normal:[0.,1.,0.],axis_normal:[0.,1.,0.],up:[0.,1.,0.],vertical_depth:2.,thickness:2.,wave_class:0,flow:[0.,0.],uv_meters:[0.,0.],covered:false};
  let settings=Settings{sky_top:[0.8;3],sky_bottom:[0.8;3],light:[0.,0.,1.],..Settings::default()};
  let source=[0.1;3];
  let result=shade(&settings,&surface,0.,source,source,source,None);
  assert!(result.iter().all(|c|*c>0.7),"reflection was suppressed: {result:?}");
 }
 #[test]
 fn deep_surface_preserves_underlying_scene_even_with_a_blue_sky(){
  let surface=Surface{ray:[0.,-1.,0.],shading_normal:[0.,1.,0.],geometric_normal:[0.,1.,0.],axis_normal:[0.,1.,0.],up:[0.,1.,0.],vertical_depth:24.,thickness:24.,wave_class:0,flow:[0.,0.],uv_meters:[0.,0.],covered:false};
  let settings=Settings{sky_top:[0.,0.,1.],sky_bottom:[0.,0.,1.],light:[1.,0.,0.],..Settings::default()};
  let source=[0.4,0.3,0.2];
  let result=shade(&settings,&surface,0.,source,source,source,None);
  for i in 0..3 {assert!((result[i]-source[i]).abs()<0.045,"{result:?}");}
  assert!(result[0]>result[2],"warm scene must remain warm: {result:?}");
 }
}
