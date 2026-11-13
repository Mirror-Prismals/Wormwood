//! Reduced muscle/hand actuators, not an anatomical muscle-fibre model.
//! Activation, shortening speed, reach and fatigue bound delivered force.
use super::*;
#[derive(Clone)]
pub(super) struct Muscles {
    pub effort: f32,
    pub activation: f32,
    pub fatigue: f32,
    pub force: f32,
    pub work: f32,
    pub peak: f32,
    previous_target: Option<[V; 2]>,
    target_velocity: [V; 2],
}
impl Default for Muscles {
    fn default() -> Self {
        Self {
            effort: 1.,
            activation: 0.,
            fatigue: 0.,
            force: 0.,
            work: 0.,
            peak: 0.,
            previous_target: None,
            target_velocity: [[0.; 3]; 2],
        }
    }
}
impl Muscles {
    pub fn begin(&mut self, target: [V; 2], dt: f32) -> [V; 2] {
        let tau = if self.effort > self.activation {
            0.055
        } else {
            0.12
        };
        self.activation += (self.effort.clamp(0., 1.) - self.activation) * (1. - (-dt / tau).exp());
        self.fatigue = (self.fatigue
            + dt * (0.035 * self.activation.powi(2)
                - 0.10 * (1. - self.activation) * self.fatigue))
            .clamp(0., 0.65);
        let speed = self.previous_target.map_or([[0.; 3]; 2], |old| {
            std::array::from_fn(|i| {
                let v = scale(sub(target[i], old[i]), 1. / dt);
                scale(v, (4. / length(v).max(4.)).min(1.))
            })
        });
        self.previous_target = Some(target);
        self.force = 0.;
        for i in 0..2 {
            self.target_velocity[i] = add(
                self.target_velocity[i],
                scale(
                    sub(speed[i], self.target_velocity[i]),
                    1. - (-dt / 0.025).exp(),
                ),
            );
        }
        self.target_velocity
    }
    pub fn hand_force(
        &mut self,
        error: V,
        velocity: V,
        wanted_velocity: V,
        reach: f32,
        limit: f32,
        effective_mass: f32,
        dt: f32,
    ) -> V {
        let activation = self.activation * (1. - self.fatigue);
        let length_factor = (1. - 0.55 * ((reach - 0.62) / 0.45).powi(2)).clamp(0.25, 1.);
        let shortening = dot(velocity, unit(error)).max(0.);
        let velocity_factor = 1. / (1. + shortening / 3.);
        let cap = limit * length_factor * velocity_factor * (0.12 + 0.88 * activation);
        let stiffness = 1800. + 6200. * activation;
        let damping = 2. * (stiffness * effective_mass).sqrt();
        // Follow target velocity during a swing; absolute-velocity damping
        // removed the momentum the old controller was trying to create.
        let f = scale(
            add(
                scale(error, stiffness),
                scale(sub(wanted_velocity, velocity), damping),
            ),
            1. / (1. + (damping * dt + stiffness * dt * dt) / effective_mass),
        );
        let f = scale(f, (cap / length(f).max(cap)).min(1.));
        self.force += length(f);
        self.peak = self.peak.max(self.force);
        self.work += dot(f, velocity).max(0.) * dt;
        f
    }
    pub fn report(&self) -> Value {
        json!({"activation":self.activation,"fatigue":self.fatigue,"hand_force_n":self.force,"peak_hand_force_n":self.peak,"positive_work_j":self.work,"model":"reduced force-length-velocity actuators; prescribed supported torso"})
    }
}
