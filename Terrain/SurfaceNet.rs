//! Experimental surface-net extraction for a continuous scalar field.
//! Sampling is temporary mesh construction, not stored world occupancy.
#[derive(Debug)]
pub(crate) struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

pub(crate) fn extract(
    origin: [f64; 3], cells: [usize; 3], step: f64,
    sample: impl Fn([f64; 3]) -> f64,
) -> Result<Mesh, String> {
    if cells.iter().any(|&n| n == 0 || n > 128) || !step.is_finite() || step <= 0. || origin.iter().any(|v| !v.is_finite()) {
        return Err("invalid surface-net bounds".into());
    }
    let [nx, ny, nz] = cells;
    let point_index = |x: usize, y: usize, z: usize| x + (nx + 1) * (y + (ny + 1) * z);
    let cell_index = |x: usize, y: usize, z: usize| x + nx * (y + ny * z);
    let mut values = Vec::with_capacity((nx + 1) * (ny + 1) * (nz + 1));
    for z in 0..=nz { for y in 0..=ny { for x in 0..=nx {
        let p = [origin[0] + x as f64 * step, origin[1] + y as f64 * step, origin[2] + z as f64 * step];
        let value = sample(p);
        if !value.is_finite() { return Err("nonfinite surface-net sample".into()); }
        values.push(value);
    } } }
    let offsets = [[0,0,0],[1,0,0],[0,1,0],[1,1,0],[0,0,1],[1,0,1],[0,1,1],[1,1,1]];
    let edges = [[0,1],[2,3],[4,5],[6,7],[0,2],[1,3],[4,6],[5,7],[0,4],[1,5],[2,6],[3,7]];
    let mut mesh = Mesh { vertices: Vec::new(), triangles: Vec::new() };
    let mut cell_vertex = vec![None; nx * ny * nz];
    for z in 0..nz { for y in 0..ny { for x in 0..nx {
        let v = offsets.map(|o| values[point_index(x+o[0], y+o[1], z+o[2])]);
        let mut sum = [0.; 3]; let mut count = 0;
        for [a,b] in edges {
            if (v[a] < 0.) == (v[b] < 0.) { continue; }
            let t = v[a] / (v[a] - v[b]);
            for k in 0..3 {
                let base = [x,y,z][k] as f64;
                sum[k] += origin[k] + (base + offsets[a][k] as f64 + t * (offsets[b][k] as f64 - offsets[a][k] as f64)) * step;
            }
            count += 1;
        }
        if count > 0 {
            cell_vertex[cell_index(x,y,z)] = Some(mesh.vertices.len() as u32);
            mesh.vertices.push(sum.map(|v| v / count as f64));
        }
    } } }
    let mut quad = |coords: [[usize;3];4], flip: bool| {
        let q = coords.map(|c| cell_vertex[cell_index(c[0], c[1], c[2])]);
        if let [Some(a),Some(b),Some(c),Some(d)] = q {
            if flip { mesh.triangles.extend([[a,c,b],[a,d,c]]); }
            else { mesh.triangles.extend([[a,b,c],[a,c,d]]); }
        }
    };
    // Each sign-changing grid edge connects the four adjacent cell vertices.
    for z in 1..nz { for y in 1..ny { for x in 0..nx {
        let a=values[point_index(x,y,z)]; let b=values[point_index(x+1,y,z)];
        if (a<0.) != (b<0.) { quad([[x,y-1,z-1],[x,y,z-1],[x,y,z],[x,y-1,z]], a>=0.); }
    } } }
    for z in 1..nz { for y in 0..ny { for x in 1..nx {
        let a=values[point_index(x,y,z)]; let b=values[point_index(x,y+1,z)];
        if (a<0.) != (b<0.) { quad([[x-1,y,z-1],[x-1,y,z],[x,y,z],[x,y,z-1]], a>=0.); }
    } } }
    for z in 0..nz { for y in 1..ny { for x in 1..nx {
        let a=values[point_index(x,y,z)]; let b=values[point_index(x,y,z+1)];
        if (a<0.) != (b<0.) { quad([[x-1,y-1,z],[x,y-1,z],[x,y,z],[x-1,y,z]], a>=0.); }
    } } }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_a_closed_sphere_without_unmatched_edges() {
        let mesh=extract([-4.;3],[16;3],0.5,|p| p.iter().map(|v|v*v).sum::<f64>()-4.).unwrap();
        assert!(!mesh.triangles.is_empty());
        let mut edges=std::collections::BTreeMap::new();
        for t in &mesh.triangles { for [a,b] in [[t[0],t[1]],[t[1],t[2]],[t[2],t[0]]] {
            *edges.entry((a.min(b),a.max(b))).or_insert(0usize)+=1;
        } }
        assert!(edges.values().all(|&n|n==2));
        assert!(mesh.vertices.iter().all(|p| (p.iter().map(|x|x*x).sum::<f64>().sqrt()-2.).abs()<0.3));
    }
}
