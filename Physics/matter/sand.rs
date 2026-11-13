//! Fine dry sand as a frictional continuum. Computational material points
//! carry parcels of many grains, not centimetre-sized pebbles. APIC transfers
//! and a Hencky-strain Drucker-Prager return map follow the formulation in
//! Klar et al., 2016, https://math.ucdavis.edu/~jteran/papers/KGPSJT16.pdf.
//! This bounded explicit adaptation uses the workshop's OGC boundary solve;
//! accepted boundary motion is transferred back into the deformation update.
//! Parameters/resolution are engineering presets, not calibrated beach sand.
use super::*;

type M = [[f32; 3]; 3];
const I: M = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const H: f32 = 0.052;
const MU: f32 = 18_000.;
const LAMBDA: f32 = 27_000.;

fn mul(a: M, b: M) -> M {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
fn transpose(a: M) -> M {
    std::array::from_fn(|r| std::array::from_fn(|c| a[c][r]))
}
fn mv(a: M, x: V) -> V {
    a.map(|r| dot(r, x))
}
fn outer(a: V, b: V) -> M {
    std::array::from_fn(|r| std::array::from_fn(|c| a[r] * b[c]))
}
fn madd(a: &mut M, b: M, s: f32) {
    for r in 0..3 {
        for c in 0..3 {
            a[r][c] += s * b[r][c];
        }
    }
}
fn spectral(axes: M, diagonal: V) -> M {
    let scaled = std::array::from_fn(|r| std::array::from_fn(|c| axes[r][c] * diagonal[c]));
    mul(scaled, transpose(axes))
}

// Jacobi diagonalization of the symmetric left Cauchy-Green tensor. Keeping
// B=F F^T removes the need to choose a polar rotation in a nearly repeated SVD.
fn eigen(mut a: M) -> (V, M) {
    let mut u = I;
    for _ in 0..10 {
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            let off = a[p][q];
            if off.abs() < 1e-8 {
                continue;
            }
            let theta = 0.5 * (2. * off).atan2(a[q][q] - a[p][p]);
            let (s, c) = theta.sin_cos();
            let mut j = I;
            j[p][p] = c;
            j[q][q] = c;
            j[p][q] = s;
            j[q][p] = -s;
            a = mul(transpose(j), mul(a, j));
            u = mul(u, j);
        }
    }
    ([a[0][0], a[1][1], a[2][2]], u)
}

#[derive(Clone)]
struct State {
    b: M,
    affine: M,
    plastic_shear: f32,
    dilation: f32,
    plastic_volume: f32,
}
#[derive(Clone)]
pub(super) struct Sand {
    points: Vec<State>,
    yielded: usize,
    peak_pressure: f32,
    grid_nodes: usize,
}
#[derive(Default)]
struct Node {
    mass: f32,
    momentum: V,
    force: V,
    velocity: V,
}
type Grid = HashMap<[i32; 3], Node>;

// Quadratic B-splines: partition of unity with gradients summing to zero.
fn stencil(p: V) -> [([i32; 3], f32, V, V); 27] {
    let x = p.map(|x| x / H);
    let base = x.map(|x| (x - 0.5).floor() as i32);
    let f: V = std::array::from_fn(|k| x[k] - base[k] as f32);
    let w = f.map(|x| {
        [
            0.5 * (1.5 - x).powi(2),
            0.75 - (x - 1.).powi(2),
            0.5 * (x - 0.5).powi(2),
        ]
    });
    let d = f.map(|x| [(x - 1.5) / H, -2. * (x - 1.) / H, (x - 0.5) / H]);
    std::array::from_fn(|i| {
        let k = [i % 3, (i / 3) % 3, i / 9];
        let key = std::array::from_fn(|a| base[a] + k[a] as i32);
        let weight = w[0][k[0]] * w[1][k[1]] * w[2][k[2]];
        let gradient = std::array::from_fn(|a| {
            d[a][k[a]] * w[(a + 1) % 3][k[(a + 1) % 3]] * w[(a + 2) % 3][k[(a + 2) % 3]]
        });
        (key, weight, gradient, sub(key.map(|x| x as f32 * H), p))
    })
}

impl Sand {
    pub fn new(count: usize) -> Self {
        Self {
            points: vec![
                State {
                    b: I,
                    affine: [[0.; 3]; 3],
                    plastic_shear: 0.,
                    plastic_volume: 1.,
                    dilation: 0.
                };
                count
            ],
            yielded: 0,
            peak_pressure: 0.,
            grid_nodes: 0,
        }
    }
    fn project(state: &mut State, gradient: M, dt: f32, kind:Kind) -> Result<(M, f32, bool), String> {
        let mut increment = I;
        madd(&mut increment, gradient, dt);
        if dot(increment[0], cross(increment[1], increment[2])) <= 0. {
            return Err("sand deformation step inverted; retaining last valid state".into());
        }
        let trial = mul(increment, mul(state.b, transpose(increment)));
        let (eigenvalues, u) = eigen(trial);
        if eigenvalues.iter().any(|x| !x.is_finite() || *x <= 0.) {
            return Err("invalid sand elastic strain".into());
        }
        if kind == Kind::Water {
            // A weakly compressible liquid retains volume, but no shear strain.
            // Compression pressure and viscosity act through the same APIC/OGC path.
            let j=(eigenvalues.iter().product::<f32>()).sqrt().clamp(0.75,1.05);
            let pressure=(24_000.*(1.-j)).max(0.);
            state.b=spectral(I,[j.powf(2./3.);3]);
            let mut stress=spectral(I,[-pressure;3]);
            madd(&mut stress,gradient,0.08);madd(&mut stress,transpose(gradient),0.08);
            return Ok((stress,pressure,false));
        }
        if kind==Kind::Snow {
            // Stomakhin et al.: elastic stretch projection and plastic-volume
            // hardening. Snow retains limited tension and packs permanently.
            let sigma=eigenvalues.map(f32::sqrt);
            let clamped=sigma.map(|s|s.clamp(0.975,1.0075));
            state.plastic_volume=(state.plastic_volume*sigma.iter().product::<f32>()/clamped.iter().product::<f32>()).clamp(0.30,2.);
            let hardening=(10.*(1.-state.plastic_volume)).clamp(-2.,2.5).exp();
            let mu=3500.*hardening;
            let lambda=7500.*hardening;
            let j=clamped.iter().product::<f32>();
            let stress=std::array::from_fn(|i|2.*mu*(clamped[i]-1.)*clamped[i]+lambda*(j-1.)*j);
            state.b=spectral(u,clamped.map(|v|v*v));
            return Ok((spectral(u,stress),(-stress.iter().sum::<f32>()/3.).max(0.),sigma!=clamped));
        }
        let mut e = eigenvalues.map(|x| 0.5 * x.ln());
        let trace = e.iter().sum::<f32>();
        let dev = e.map(|x| x - trace / 3.);
        let norm = length(dev);
        let angle = (32. + 5. * (1. - (-state.plastic_shear).exp())).to_radians();
        let alpha = (2f32 / 3.).sqrt() * 2. * angle.sin() / (3. - angle.sin());
        let yield_amount = norm + (3. * LAMBDA + 2. * MU) / (2. * MU) * trace * alpha;
        let mut yielded = false;
        if trace + state.dilation > 0. {
            // Free expansion carries no tension. Remember the loose volume
            // so its later closure does not immediately become elastic load.
            state.dilation = (state.dilation + trace).max(0.);
            state.plastic_shear += norm;
            e = [0.; 3];
            yielded = true;
        } else {
            if state.dilation > 0. {
                let absorb = state.dilation.min(-trace);
                for x in &mut e {
                    *x += absorb / 3.;
                }
                state.dilation -= absorb;
            }
            let compression = e.iter().sum::<f32>();
            let amount = norm + (3. * LAMBDA + 2. * MU) / (2. * MU) * compression * alpha;
            if yield_amount > 0. || amount > 0. {
                let gamma = amount.max(0.).min(norm);
                if norm > 1e-8 {
                    for k in 0..3 {
                        e[k] -= gamma * dev[k] / norm;
                    }
                }
                state.plastic_shear += gamma;
                yielded = gamma > 0.;
            }
        }
        state.b = spectral(u, e.map(|x| (2. * x).exp()));
        let tr = e.iter().sum::<f32>();
        let pressure = (-(LAMBDA + 2. * MU / 3.) * tr).max(0.);
        Ok((
            spectral(u, e.map(|x| 2. * MU * x + LAMBDA * tr)),
            pressure,
            yielded,
        ))
    }
    fn transfer(&self, p: &[Particle], stress: &[M]) -> Grid {
        let mut grid = Grid::with_capacity(p.len() * 3);
        for (i, p) in p.iter().enumerate() {
            // Density follows each parcel's current phase: snow and meltwater
            // can coexist in one transfer grid, with different rest volumes.
            let volume = p.mass / p.kind.law().density;
            for (key, w, grad, offset) in stencil(p.p) {
                let node = grid.entry(key).or_default();
                node.mass += w * p.mass;
                node.momentum = add(
                    node.momentum,
                    scale(add(p.v, mv(self.points[i].affine, offset)), w * p.mass),
                );
                node.force = sub(node.force, scale(mv(stress[i], grad), volume));
            }
        }
        grid
    }
    pub fn predict(&mut self, particles: &[Particle], dt: f32) -> Result<Vec<V>, String> {
        let mut p = particles.to_vec();
        let mut trial = self.clone();
        // Compacted snow hardens substantially. Respect its fastest elastic
        // wave rather than applying the loose-sand transport step to it.
        // This only subdivides transport; OGC's physical frame stays fixed.
        let substeps = if particles.iter().any(|p| p.kind == Kind::Snow) {
            let wave_speed = ((7500. + 2. * 3500.) * 2.5f32.exp() / 280.).sqrt();
            (dt * wave_speed / (0.4 * H)).ceil().max(2.) as usize
        } else { 2 };
        let h = dt / substeps as f32;
        for _ in 0..substeps {
            let stress = trial
                .points
                .iter_mut()
                .enumerate().map(|(i,s)| Self::project(s, [[0.; 3]; 3], h, p[i].kind).map(|r| r.0))
                .collect::<Result<Vec<_>, _>>()?;
            let mut grid = trial.transfer(&p, &stress);
            trial.grid_nodes = grid.len();
            for node in grid.values_mut() {
                if node.mass <= 1e-10 {
                    continue;
                }
                node.velocity = add(
                    scale(node.momentum, 1. / node.mass),
                    add(scale(node.force, h / node.mass), [0., -9.81 * h, 0.]),
                );
            }
            for (i, p) in p.iter_mut().enumerate() {
                let mut v = [0.; 3];
                let mut gradient = [[0.; 3]; 3];
                let mut affine = [[0.; 3]; 3];
                for (key, w, grad, offset) in stencil(p.p) {
                    let node = &grid[&key];
                    v = add(v, scale(node.velocity, w));
                    madd(&mut gradient, outer(node.velocity, grad), 1.);
                    madd(&mut affine, outer(node.velocity, offset), w * 4. / (H * H));
                }
                Self::project(&mut trial.points[i], gradient, h, p.kind)?;
                trial.points[i].affine = affine;
                p.v = v;
                p.p = add(p.p, scale(v, h));
                if p.p.iter().chain(&p.v).any(|x| !x.is_finite()) {
                    return Err("non-finite sand prediction".into());
                }
            }
        }
        self.grid_nodes = trial.grid_nodes;
        Ok(p.into_iter().map(|p| p.p).collect())
    }
    pub fn finish(&mut self, particles: &mut [Particle], old: &[V], dt: f32) -> Result<(), String> {
        // Reconstruct the velocity gradient from accepted OGC displacement,
        // not the unconstrained prediction (which would compress through a
        // stopped bucket). Rebuild the affine term from the same velocity.
        let mut grid = Grid::with_capacity(particles.len() * 3);
        for (i, p) in particles.iter().enumerate() {
            let v = scale(sub(p.p, old[i]), 1. / dt);
            for (key, w, _, _) in stencil(old[i]) {
                let n = grid.entry(key).or_default();
                n.mass += w * p.mass;
                n.momentum = add(n.momentum, scale(v, w * p.mass));
            }
        }
        for n in grid.values_mut() {
            if n.mass > 1e-10 {
                n.velocity = scale(n.momentum, 1. / n.mass);
            }
        }
        self.yielded = 0;
        for (i, s) in self.points.iter_mut().enumerate() {
            let mut gradient = [[0.; 3]; 3];
            let mut affine = [[0.; 3]; 3];
            for (key, w, grad, offset) in stencil(old[i]) {
                let v = grid[&key].velocity;
                madd(&mut gradient, outer(v, grad), 1.);
                madd(&mut affine, outer(v, offset), w * 4. / (H * H));
            }
            let old_plastic=s.plastic_volume;
            let (_, pressure, yielded) = Self::project(s, gradient, dt, particles[i].kind)?;
            if particles[i].kind==Kind::Snow {
                particles[i].radius *= (s.plastic_volume/old_plastic).cbrt();
            }

            self.peak_pressure = self.peak_pressure.max(pressure);
            self.yielded += usize::from(yielded);
            s.affine = affine;
        }
        Ok(())
    }
    pub(super) fn resize(&mut self,n:usize) { self.points.resize(n,State{b:I,affine:[[0.;3];3],plastic_shear:0.,dilation:0.,plastic_volume:1.}); }
    pub(super) fn remove_point(&mut self, i: usize) { self.points.remove(i); }
    pub(super) fn reset_point(&mut self,i:usize) {
        self.points[i].b=I;self.points[i].plastic_volume=1.;self.points[i].dilation=0.;self.points[i].affine=[[0.;3];3];
    }
    pub(super) fn minimum_plastic_volume(&self)->f32 { self.points.iter().map(|p|p.plastic_volume).fold(1.,f32::min) }
    pub fn report(&self) -> Value {

        json!({"model":"fine dry sand: APIC material parcels, pressure-dependent frictional plasticity, no tensile cohesion",
            "material_points":self.points.len(),"grid_spacing_m":H,"grid_nodes":self.grid_nodes,
            "yielding_points":self.yielded,"peak_pressure_pa":self.peak_pressure,
            "physical_grain_size_m":0.0005,"explicit_individual_grains":false,
            "scope":"bounded workshop deposit; computational parcels represent many physical grains"})
    }
}
