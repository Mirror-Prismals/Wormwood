//! Per-substep inertial targets shared by stand-alone and layered materials.
//! Iterative contact corrections do not reapply gravity or tool acceleration.
use super::*;

pub(super) struct Step {
    pub old: Vec<V>,
    pub predicted: Vec<V>,
    pub shift: Vec<V>,
}
impl Step {
    pub fn new(p: &[V], velocity: &[V], weights: &[f32], dt: f32) -> Self {
        Self {
            old: p.to_vec(),
            predicted: p
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    if weights[i] == 0. {
                        p
                    } else {
                        add(p, add(scale(velocity[i], dt), [0., -9.81 * dt * dt, 0.]))
                    }
                })
                .collect(),
            shift: vec![[0.; 3]; p.len()],
        }
    }
    pub fn target(&mut self, current: &[V], q: &mut [V]) {
        for i in 0..q.len() {
            self.shift[i] = add(self.shift[i], sub(q[i], current[i]));
            q[i] = add(self.predicted[i], self.shift[i]);
        }
    }
}

pub(super) fn table_positions() -> [V; 4] {
    [[-1., 0., -1.], [1., 0., -1.], [1., 0., 1.], [-1., 0., 1.]]
}
pub(super) fn tool_surface(tool: &Tool, start: usize) -> Value {
    json!({"start":start,"count":tool.p.len(),"triangles":tool.faces,
        "motion":0,"self_collision":false,"friction":0.60})
}
pub(super) fn table_surface(tool:&Tool, start: usize) -> Value {
    json!({"start":start,"count":tool.ground.len(),"triangles":tool.ground_faces,
        "motion":2,"self_collision":false,"friction":0.75})
}
