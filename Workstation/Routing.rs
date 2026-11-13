//! Cardinal's four workstation buses and their stereo monitor fold.
//! Routing preserves gain and headroom; spatial processing belongs elsewhere.
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bus {
    Left,
    Sub,
    FrontFill,
    Right,
}

impl Bus {
    pub(super) fn parse(value: &Value) -> Result<Option<Self>, String> {
        match value.as_str() {
            Some("OFF") => Ok(None),
            Some("L") => Ok(Some(Self::Left)),
            Some("S") => Ok(Some(Self::Sub)),
            Some("FF") => Ok(Some(Self::FrontFill)),
            Some("R") => Ok(Some(Self::Right)),
            _ => Err("output bus must be OFF, L, S, FF or R".into()),
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Sub => 1,
            Self::FrontFill => 2,
            Self::Right => 3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Left => "L",
            Self::Sub => "S",
            Self::FrontFill => "FF",
            Self::Right => "R",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Route {
    pub left: Option<Bus>,
    pub right: Option<Bus>,
}

impl Default for Route {
    fn default() -> Self {
        Self {
            left: Some(Bus::FrontFill),
            right: Some(Bus::FrontFill),
        }
    }
}

impl Route {
    /// Sum shared-bus channels before adding them to the existing mix.
    pub(super) fn add(self, stereo: [f32; 2], buses: &mut [f32; 4]) {
        if let (Some(left), Some(right)) = (self.left, self.right) {
            if left == right {
                buses[left.index()] += stereo[0] + stereo[1];
                return;
            }
        }
        if let Some(bus) = self.left {
            buses[bus.index()] += stereo[0];
        }
        if let Some(bus) = self.right {
            buses[bus.index()] += stereo[1];
        }
    }

    pub(super) fn snapshot(self) -> Value {
        json!({
            "output_bus_l": self.left.map_or("OFF", Bus::name),
            "output_bus_r": self.right.map_or("OFF", Bus::name)
        })
    }

    /// Parse the complete patch before returning a replacement route.
    pub(super) fn from_patch(input: &Value, old: Self) -> Result<Self, String> {
        let patch = input.as_object().ok_or("routing patch must be an object")?;
        let left = match patch.get("output_bus_l") {
            Some(value) => Bus::parse(value)?,
            None => old.left,
        };
        let right = match patch.get("output_bus_r") {
            Some(value) => Bus::parse(value)?,
            None => old.right,
        };
        Ok(Self { left, right })
    }
}

pub(super) fn listen(buses: [f32; 4]) -> [f32; 2] {
    let center = 0.5 * (buses[1] + buses[2]);
    [buses[0] + center, buses[3] + center]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_preserves_each_channel_impulse() {
        let choices = ["OFF", "L", "S", "FF", "R"];
        let bus_impulses = [
            [0., 0., 0., 0.],
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        let monitor_impulses = [[0., 0.], [1., 0.], [0.5, 0.5], [0.5, 0.5], [0., 1.]];
        for (left, left_name) in choices.iter().enumerate() {
            for (right, right_name) in choices.iter().enumerate() {
                let patch = json!({"output_bus_l":left_name,"output_bus_r":right_name});
                let route = Route::from_patch(&patch, Route::default()).unwrap();
                assert_eq!(route.snapshot(), patch);
                for (stereo, expected) in [([1., 0.], left), ([0., 1.], right)] {
                    let mut buses = [0.; 4];
                    route.add(stereo, &mut buses);
                    assert_eq!(
                        buses, bus_impulses[expected],
                        "route {left_name}/{right_name}"
                    );
                    assert_eq!(listen(buses), monitor_impulses[expected]);
                }
            }
        }
    }

    #[test]
    fn default_front_fill_sums_stereo_and_monitor_halves_the_sum() {
        let route = Route::default();
        assert_eq!(
            route.snapshot(),
            json!({"output_bus_l":"FF","output_bus_r":"FF"})
        );
        let mut buses = [0.; 4];
        route.add([2., 6.], &mut buses);
        assert_eq!(buses, [0., 0., 8., 0.]);
        assert_eq!(listen(buses), [4., 4.]);
    }

    #[test]
    fn shared_bus_preserves_existing_mix_negative_samples_and_headroom() {
        let mut buses = [10., -7., 11., -13.];
        Route {
            left: Some(Bus::Left),
            right: Some(Bus::Left),
        }
        .add([12., 20.], &mut buses);
        assert_eq!(buses, [42., -7., 11., -13.]);
        assert_eq!(listen(buses), [44., -11.]);
        Route {
            left: Some(Bus::Right),
            right: Some(Bus::Right),
        }
        .add([-12., -20.], &mut buses);
        assert_eq!(buses, [42., -7., 11., -45.]);
        assert_eq!(listen(buses), [44., -43.]);
        // Preserve Cardinal's source-sum grouping as well as its gain.
        let mut cancellation = [0., 0., 1e20, 0.];
        Route::default().add([-1e20, 1.], &mut cancellation);
        assert_eq!(cancellation, [0.; 4]);
    }

    #[test]
    fn partial_patch_retains_other_channel_and_off_is_explicit() {
        let old = Route {
            left: Some(Bus::Left),
            right: Some(Bus::Right),
        };
        assert_eq!(Route::from_patch(&json!({}), old).unwrap(), old);
        let updated = Route::from_patch(&json!({"output_bus_l":"OFF"}), old).unwrap();
        assert_eq!(
            updated,
            Route {
                left: None,
                right: Some(Bus::Right)
            }
        );
        let mut buses = [3., 4., 5., 6.];
        updated.add([100., 2.], &mut buses);
        assert_eq!(buses, [3., 4., 5., 8.]);
    }

    #[test]
    fn invalid_patch_cannot_partially_commit() {
        let mut route = Route::default();
        let before = route;
        for invalid in [
            json!(null),
            json!(3),
            json!(true),
            json!("F"),
            json!("ff"),
            json!(" L"),
        ] {
            let patch = json!({"output_bus_l":"L","output_bus_r":invalid});
            let result = Route::from_patch(&patch, route).map(|next| route = next);
            assert!(result.is_err());
            assert_eq!(route, before);
        }
        assert!(Route::from_patch(&json!({"output_bus_l":null}), route).is_err());
        assert!(Route::from_patch(&Value::Null, route).is_err());
        assert!(Route::from_patch(&json!([]), route).is_err());
    }
}
