//! Continuous free surfaces. The finite density sampling grid below is only a
//! visual isosurface extractor; it never stores terrain or settled water blocks.
use super::{bodies, hydrology, math::*, ocean, soft_body};
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vertex {
    pub position: V,
    pub normal: V,
    pub geometric: V,
    pub axis: V,
    pub uv: V2,
    pub flow: V2,
    pub depth: f64,
    pub thickness: f64,
    pub class: u32,
    pub flags: u32,
}
#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub truncated: bool,
    pub visibility: VisibilityStats,
}
#[derive(Default,Clone,Copy,Debug,PartialEq,serde::Serialize)]
pub(crate) struct VisibilityStats {pub grid_cells:usize,pub grid_cells_skipped:usize,pub grid_samples:usize,pub authored_triangles_skipped:usize}
impl Mesh {
    pub fn remove_material_domains(&mut self,domains:&[crate::terrain::material_domain::Domain]) {
        for domain in domains {
            let old=std::mem::take(self);let mut next=Mesh::default();
            for ids in old.indices.chunks_exact(3) {
                let mut inside:Vec<_>=ids.iter().map(|&i|old.vertices[i as usize]).collect();
                for axis in 0..3{for upper in [false,true]{
                    if inside.is_empty(){break;}
                    let distance=|v:Vertex|{let x=domain.local(v.position)[axis];if upper{domain.max[axis]-x}else{x-domain.min[axis]}};
                    let cut=|keep:bool|{let mut out=Vec::new();let mut a=*inside.last().unwrap();let mut da=distance(a);
                        for &b in &inside{let db=distance(b);let ka=if keep{da>=0.}else{da<0.};let kb=if keep{db>=0.}else{db<0.};
                            if ka!=kb{let t=da/(da-db);let mix=|a:V,b:V|add(a,mul(sub(b,a),t));let mut v=a;v.position=mix(a.position,b.position);v.normal=unit(mix(a.normal,b.normal));v.geometric=unit(mix(a.geometric,b.geometric));v.axis=unit(mix(a.axis,b.axis));v.uv=std::array::from_fn(|k|a.uv[k]+t*(b.uv[k]-a.uv[k]));v.flow=std::array::from_fn(|k|a.flow[k]+t*(b.flow[k]-a.flow[k]));v.depth=a.depth+t*(b.depth-a.depth);out.push(v);}if kb{out.push(b);}a=b;da=db;
                        }out};
                    let outside=cut(false);for k in 1..outside.len().saturating_sub(1){next.triangle([outside[0],outside[k],outside[k+1]]);}inside=cut(true);
                }}
            }
            next.truncated|=old.truncated;next.visibility=old.visibility;*self=next;
        }
    }
    pub fn material_water(&mut self,vertices:&[[f64;3]],frame:Frame) {
        for t in vertices.chunks_exact(3){let n=unit(cross(sub(t[1],t[0]),sub(t[2],t[0])));if len(n)<0.5{continue;}
            self.triangle(std::array::from_fn(|i|{let p=t[i];let uv=frame.local(p);Vertex{position:p,normal:n,geometric:n,axis:n,uv:[uv[0],uv[2]],flow:[0.;2],depth:0.10,thickness:0.08,class:4,flags:0}}));
        }
    }
    fn triangle(&mut self, mut p: [Vertex; 3]) {
        if self.vertices.len() + 3 > 196608 {
            self.truncated = true;
            return;
        }
        let normal = cross(
            sub(p[1].position, p[0].position),
            sub(p[2].position, p[0].position),
        );
        if len(normal) < 1e-12 {
            return;
        }
        if dot(normal, add(add(p[0].normal, p[1].normal), p[2].normal)) < 0. {
            p.swap(1, 2);
        }
        let first = self.vertices.len() as u32;
        self.vertices.extend(p);
        self.indices.extend([first, first + 1, first + 2]);
    }
}
pub(crate) fn chart() -> Frame {
    Frame::radial([0., crate::terrain::RADIUS, 0.], [0., 0., -1.])
}
pub(crate) fn free_surfaces(
    focus: V,
    _forward: V,
    bodies: &bodies::State,
    mean: f64,
    ocean: &ocean::State,
    terrain: &hydrology::State,
) -> (Frame, Mesh) {
    free_surfaces_visible(focus,_forward,bodies,mean,ocean,terrain,None)
}
pub(crate) fn free_surfaces_visible(focus:V,forward:V,bodies:&bodies::State,mean:f64,ocean:&ocean::State,terrain:&hydrology::State,view:Option<&super::visibility::View>)->(Frame,Mesh){
    static PARALLEL:std::sync::OnceLock<bool>=std::sync::OnceLock::new();
    let parallel=*PARALLEL.get_or_init(||!cfg!(test)&&std::env::var("WORMWOOD_WATER_SAMPLE_PARALLEL").as_deref()!=Ok("0")&&std::thread::available_parallelism().map_or(false,|n|n.get()>=4));
    free_surfaces_impl_visible(focus,forward,bodies,mean,ocean,terrain,parallel,view)
}
fn free_surfaces_impl(focus:V,_forward:V,bodies:&bodies::State,mean:f64,ocean:&ocean::State,terrain:&hydrology::State,parallel:bool)->(Frame,Mesh){
    free_surfaces_impl_visible(focus,_forward,bodies,mean,ocean,terrain,parallel,None)
}
fn free_surfaces_impl_visible(focus:V,_forward:V,bodies:&bodies::State,mean:f64,ocean:&ocean::State,terrain:&hydrology::State,parallel:bool,view:Option<&super::visibility::View>)->(Frame,Mesh){
    let radial = unit(focus);
    let origin = mul(radial, crate::terrain::RADIUS);
    let frame = Frame::radial(origin, [0., 0., -1.]);
    let global = chart();
    let chart_origin = global.local(frame.origin);
    let mut mesh = Mesh::default();
    // Preserve the central 1.5 m samples and add coarser outer coverage.
    let side = 128usize;
    let coordinate = |i: usize| {
        let x = i as f64 - side as f64 * 0.5;
        x.signum() * (x.abs().min(32.) * 1.5 + (x.abs()-32.).max(0.) * 6.)
    };
    let mut cells=vec![true;side*side];let mut needed=vec![view.is_none();(side+1)*(side+1)];
    if let Some(view)=view {
        let heights=super::visibility::height_bounds(len(focus)-crate::terrain::RADIUS < -18.,bodies,mean,ocean);
        for z in 0..side {for x in 0..side {
            let visible=heights.is_some_and(|heights|{let(a,b,r)=super::visibility::cell_capsule(frame,[coordinate(x),coordinate(x+1)],[coordinate(z),coordinate(z+1)],heights);view.capsule(a,b,r)});
            cells[z*side+x]=visible;
            if visible {let a=z*(side+1)+x;for i in [a,a+1,a+side+1,a+side+2]{needed[i]=true;}}
        }}
    }
    mesh.visibility=VisibilityStats{grid_cells:side*side,grid_cells_skipped:cells.iter().filter(|&&v|!v).count(),grid_samples:needed.iter().filter(|&&v|v).count(),authored_triangles_skipped:0};
    let sample=|x:usize,z:usize|{
            if !needed[z*(side+1)+x]{return None;}
            let p = frame.world([
                coordinate(x),
                0.,
                coordinate(z),
            ]);
            let water = if len(focus)-crate::terrain::RADIUS < -18. {
                bodies.depths_surface_boundary(p)
            } else {
                bodies.natural_candidate(p, mean, ocean, terrain).or_else(|| {
                    if !bodies.generated || coordinate(x).abs().max(coordinate(z).abs()) <= 48. {
                        return None;
                    }
                    // Outside the finite accepted mesh, show only generated regional
                    // drainage over its analytic bed. This is presentation, not a
                    // collision/occupancy fallback. Keep local unknown coverage open.
                    let up = unit(p);
                    let bed = crate::terrain::height(up);
                    if bodies.ocean && crate::terrain::regions::ocean(up) {
                        let uv=global.local(p);
                        let (shore,direction)=crate::terrain::regions::sea_shore(up);
                        return Some(super::flow::WetPoint {frame,point:frame.local(p),uv:[uv[0],uv[2]],surface_height:ocean.surface([uv[0],uv[2]],mean,bed,shore),mean_height:mean,bed_height:bed,shore_distance:shore,shore_gradient:direction.map(|v| -v),wave_amplitude:ocean.settings.amplitude,shore_direction:direction,downstream_direction:[0.;2],kind:super::flow::Kind::Ocean});
                    }
                    let w = crate::terrain::drainage::water_candidate(up)?;
                    let world = [w.direction[0], 0., w.direction[1]];
                    Some(super::flow::WetPoint {
                        frame, point: frame.local(p), uv: [0.;2],
                        surface_height: w.level, mean_height: w.level, bed_height: bed,
                        shore_distance: w.shore_distance, shore_gradient: [0.;2],
                        wave_amplitude: 0., shore_direction: [0.;2],
                        downstream_direction: [dot(world,frame.right),dot(world,frame.forward)],
                        kind: match w.kind {
                            crate::terrain::drainage::Kind::River => super::flow::Kind::River,
                            crate::terrain::drainage::Kind::Lake => super::flow::Kind::Lake,
                        },
                    })
                })
            };
            let vertex = water.map(|w| {
                let up = unit(p);
                let position = mul(up, crate::terrain::RADIUS + w.surface_height);
                let uv = global.local(position);
                let normal = if w.kind == super::flow::Kind::Ocean {
                    let n = ocean.normal(
                        [uv[0], uv[2]],
                        w.mean_height,
                        w.bed_height,
                        w.shore_distance,
                        w.shore_gradient,
                    );
                    unit(add(
                        up,
                        add(
                            mul(global.right, n[0] / n[1].max(1e-6)),
                            mul(global.forward, n[2] / n[1].max(1e-6)),
                        ),
                    ))
                } else {
                    up
                };
                let signed_depth = if w.kind == super::flow::Kind::River {
                    (w.surface_height - w.bed_height).min(w.shore_distance)
                } else {
                    w.surface_height - w.bed_height
                };
                (
                    Vertex {
                        position,
                        normal,
                        geometric: normal,
                        axis: up,
                        uv: [uv[0] - chart_origin[0], uv[2] - chart_origin[2]],
                        flow: w.downstream_direction,
                        depth: (w.surface_height - w.bed_height).max(0.),
                        thickness: (w.surface_height - w.bed_height).max(0.),
                        class: match w.kind {
                            super::flow::Kind::River => 3,
                            super::flow::Kind::Lake => 2,
                            _ => 1,
                        },
                        flags: 2,
                    },
                    signed_depth,
                )
            });
            vertex
    };
    let points=sample_grid(side,parallel,&sample);
    for z in 0..side {
        for x in 0..side {
            if !cells[z*side+x]{continue;}
            let a = z * (side + 1) + x;
            for t in [[a, a + 1, a + side + 2], [a, a + side + 2, a + side + 1]] {
                // Clip the continuous wet boundary through each triangle.
                // Unknown collision coverage remains an explicit hole.
                if let (Some(a), Some(b), Some(c)) = (points[t[0]], points[t[1]], points[t[2]]) {
                    clip_wet_triangle(&mut mesh,[a,b,c]);
                }
            }
        }
    }
    // Distant ocean is appearance over the shared analytic landform, not a
    // replacement for the finite collision/occupancy snapshot above.
    if bodies.ocean && bodies.generated {regional_ocean(&mut mesh,frame,mean,ocean);}
    // Join abrupt drops between adjacent underground river surfaces with a
    // falling sheet. Water occupancy remains governed by the river query.
    if len(focus)-crate::terrain::RADIUS < -18. {
        for z in 0..side { for x in 0..side {
            if !cells[z*side+x]{continue;}
            let a=z*(side+1)+x;
            for (b,c) in [(a+1,a+side+1),(a+side+1,a+1)] {
                let (Some((high,_)),Some((low,_)),Some((edge,_)))=(points[a],points[b],points[c]) else {continue;};
                let drop=len(high.position)-len(low.position);
                if !(2.0..=16.0).contains(&drop) {continue;}
                let side_vector=sub(edge.position,high.position);
                let width=len(side_vector);
                if width<0.01 {continue;}
                let up=unit(high.position);
                let normal=unit(cross(side_vector,up));
                let top0=high.position;
                let top1=add(top0,side_vector);
                let bottom0=sub(top0,mul(up,drop));
                let bottom1=sub(top1,mul(up,drop));
                let make=|position:V,uv:V2| Vertex {position,normal,geometric:normal,axis:up,uv,flow:[0.,-1.],depth:drop,thickness:0.4,class:3,flags:2};
                let v0=make(top0,high.uv);let v1=make(top1,edge.uv);
                let v2=make(bottom1,[edge.uv[0],edge.uv[1]+drop]);let v3=make(bottom0,[high.uv[0],high.uv[1]+drop]);
                mesh.triangle([v0,v1,v2]);mesh.triangle([v0,v2,v3]);
            }
        }}
    }
    // Shallow continuously flowing flume films follow the authored ride paths.
    for t in &bodies.sheets {
        if len(sub(t[0],focus))>500. {continue;}
        if view.is_some_and(|v|!v.triangle(*t)){mesh.visibility.authored_triangles_skipped+=1;continue;}
        let normal=unit(cross(sub(t[1],t[0]),sub(t[2],t[0])));let normal=if normal[1]<0. {mul(normal,-1.)}else{normal};
        mesh.triangle(t.map(|p|Vertex{position:p,normal,geometric:normal,axis:normal,uv:[p[0],p[2]],flow:[0.,1.],depth:0.08,thickness:0.06,class:3,flags:2}));
    }
    // Authored finite basins have a measured bed and exact rectangular edges.
    // Render them separately from regional drainage, including the grotto pool.
    for pool in &bodies.pools {
        let center=mul(add(pool.min,pool.max),0.5);
        if len(sub(center,focus))>500. {continue;}
        let sign=if pool.min[1]<0. {-1.}else{1.};
        let surface=if sign<0. {pool.min[1]}else{pool.max[1]};
        let nx=((pool.max[0]-pool.min[0])/2.).ceil() as usize;
        let nz=((pool.max[2]-pool.min[2])/2.).ceil() as usize;
        let center=[center[0],surface,center[2]];
        let radius=((pool.max[0]-pool.min[0]).powi(2)+(pool.max[2]-pool.min[2]).powi(2)).sqrt()*0.5;
        if view.is_some_and(|v|!v.sphere(center,radius)){mesh.visibility.authored_triangles_skipped+=2*nx*nz;continue;}
        let make=|x:usize,z:usize| {let p=[pool.min[0]+(pool.max[0]-pool.min[0])*x as f64/nx as f64,surface,pool.min[2]+(pool.max[2]-pool.min[2])*z as f64/nz as f64];
            Vertex{position:p,normal:[0.,sign,0.],geometric:[0.,sign,0.],axis:[0.,sign,0.],uv:[p[0],p[2]],flow:[0.;2],depth:pool.max[1]-pool.min[1],thickness:pool.max[1]-pool.min[1],class:2,flags:2}};
        for z in 0..nz {for x in 0..nx {
            let a=make(x,z);let b=make(x+1,z);let c=make(x+1,z+1);let d=make(x,z+1);
            for t in [[a,b,c],[a,c,d]]{if view.is_none_or(|v|v.triangle(t.map(|p|p.position))){mesh.triangle(t);}else{mesh.visibility.authored_triangles_skipped+=1;}}
        }}
    }
    (frame, mesh)
}
#[inline]
/// Square rings join the existing +/-240 m sample grid without overlap.
/// Authored seas share this bounded horizon mesh.
fn regional_ocean(mesh:&mut Mesh,frame:Frame,mean:f64,wave:&ocean::State) {
    if crate::terrain::nightworld::active(frame.origin) {return;}
    if crate::terrain::regions::ocean_distance(unit(frame.origin)) < -16_000. {return;}
    let chart=chart();let anchor=chart.local(frame.origin);
    let sample=|p:V| {
        let up=unit(p);let bed=crate::terrain::height(up);
        let (shore,direction)=crate::terrain::regions::sea_shore(up);
        let uv=chart.local(p);
        let h=wave.surface([uv[0],uv[2]],mean,bed,shore);
        let wn=wave.normal([uv[0],uv[2]],mean,bed,shore,direction.map(|v| -v));
        let normal=unit(add(up,add(mul(chart.right,wn[0]/wn[1].max(1e-6)),mul(chart.forward,wn[2]/wn[1].max(1e-6)))));
        let depth=h-bed;
        (Vertex{position:mul(up,crate::terrain::RADIUS+h),normal,geometric:normal,axis:up,uv:[uv[0]-anchor[0],uv[2]-anchor[2]],flow:[0.;2],depth:depth.max(0.),thickness:depth.max(0.),class:1,flags:2},if crate::terrain::regions::ocean(up){depth}else{-depth.abs()-1.})
    };
    let ring=|radius:f64| -> Vec<(Vertex,f64)> {
        (0..512).map(|i|{let side=i/128;let q=(i%128)as f64-64.;
            let t=q.signum()*(q.abs().min(32.)*1.5+(q.abs()-32.).max(0.)*6.)/240.;
            let (x,z)=match side {0=>(-1.,t),1=>(t,1.),2=>(1.,-t),_=>(-t,-1.)};
            sample(frame.world([x*radius,0.,z*radius]))
        }).collect()
    };
    let mut inner=ring(240.);
    for radius in [480.,960.,1920.,3840.,7680.,15360.] {
        let outer=ring(radius);
        for i in 0..512 {let j=(i+1)%512;
            clip_wet_triangle(mesh,[inner[i],inner[j],outer[j]]);
            clip_wet_triangle(mesh,[inner[i],outer[j],outer[i]]);
        }
        inner=outer;
    }
}

#[inline]
fn clip_wet_triangle(mesh:&mut Mesh,source:[(Vertex,f64);3]){
    if source.iter().all(|(_,d)|*d>=0.){mesh.triangle(source.map(|(v,_)|v));return;}
    if source.iter().all(|(_,d)|*d<0.){return;}
    // A triangle clipped by one linear wet boundary has at most four corners.
    let mut polygon=[source[0].0;4];let mut count=0;
    for i in 0..3 {let(a,da)=source[i];let(b,db)=source[(i+1)%3];
        if da>=0.{polygon[count]=a;count+=1;}
        if (da>=0.)!=(db>=0.){polygon[count]=interpolate(a,b,da/(da-db));count+=1;}
    }
    for i in 1..count.saturating_sub(1){mesh.triangle([polygon[0],polygon[i],polygon[i+1]]);}
}
fn interpolate(a: Vertex, b: Vertex, t: f64) -> Vertex {
    let scalar = |x: f64, y: f64| x + (y - x) * t;
    Vertex {
        position: mix(a.position, b.position, t),
        normal: unit(mix(a.normal, b.normal, t)),
        geometric: unit(mix(a.geometric, b.geometric, t)),
        axis: unit(mix(a.axis, b.axis, t)),
        uv: std::array::from_fn(|i| scalar(a.uv[i], b.uv[i])),
        flow: std::array::from_fn(|i| scalar(a.flow[i], b.flow[i])),
        depth: 0.,
        thickness: 0.,
        class: a.class,
        flags: a.flags,
    }
}
pub(crate) fn parcels(mesh: &mut Mesh, frame: Frame, water: &soft_body::State) {
    let selected: Vec<_> = water
        .parcels
        .iter()
        .filter(|p| p.covered && len(sub(p.position, frame.origin)) < 64.)
        .collect();
    if selected.is_empty() {
        return;
    }
    let range = water.settings.radius * 2.;
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in &selected {
        let q = frame.local(p.position);
        for i in 0..3 {
            low[i] = low[i].min(q[i] - range);
            high[i] = high[i].max(q[i] + range);
        }
    }
    let spacing = (water.settings.radius * 0.7)
        .max((0..3).map(|i| high[i] - low[i]).fold(0., f64::max) / 62.);
    let dims: [usize; 3] =
        std::array::from_fn(|i| (((high[i] - low[i]) / spacing).ceil() as usize + 1).clamp(2, 64));
    let index = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
    let count = dims.iter().product();
    let mut density = vec![0.; count];
    for p in &selected {
        let q = frame.local(p.position);
        let lo: [usize; 3] = std::array::from_fn(|i| {
            (((q[i] - range - low[i]) / spacing).floor().max(0.) as usize).min(dims[i] - 1)
        });
        let hi: [usize; 3] = std::array::from_fn(|i| {
            (((q[i] + range - low[i]) / spacing).ceil().max(0.) as usize).min(dims[i] - 1)
        });
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let r: V = [
                        low[0] + x as f64 * spacing,
                        low[1] + y as f64 * spacing,
                        low[2] + z as f64 * spacing,
                    ];
                    let d = len(sub(r, q)) / range;
                    if d < 1. {
                        density[index(x, y, z)] += (1. - d * d).powi(3);
                    }
                }
            }
        }
    }
    let gradient = |c: [usize; 3]| -> V {
        std::array::from_fn(|i| {
            let mut a = c;
            let mut b = c;
            a[i] = a[i].saturating_sub(1);
            b[i] = (b[i] + 1).min(dims[i] - 1);
            -(density[index(b[0], b[1], b[2])] - density[index(a[0], a[1], a[2])])
                / ((b[i] - a[i]).max(1) as f64 * spacing)
        })
    };
    let offsets = [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 1, 1],
    ];
    let tetra = [
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
        [0, 5, 1, 6],
    ];
    let edges = [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]];
    let level = 0.421875;
    let global = chart();
    let uv_origin = global.local(frame.origin);
    for z in 0..dims[2] - 1 {
        for y in 0..dims[1] - 1 {
            for x in 0..dims[0] - 1 {
                let coords = offsets.map(|d| [x + d[0], y + d[1], z + d[2]]);
                let values = coords.map(|c| density[index(c[0], c[1], c[2])]);
                if values.iter().all(|n| *n < level) || values.iter().all(|n| *n >= level) {
                    continue;
                }
                for tet in tetra {
                    let mut crossing = Vec::with_capacity(4);
                    for edge in edges {
                        let a = tet[edge[0]];
                        let b = tet[edge[1]];
                        if (values[a] < level) == (values[b] < level) {
                            continue;
                        }
                        let amount = (level - values[a]) / (values[b] - values[a]);
                        let local: V = std::array::from_fn(|i| {
                            low[i]
                                + (coords[a][i] as f64
                                    + (coords[b][i] as f64 - coords[a][i] as f64) * amount)
                                    * spacing
                        });
                        let position = frame.world(local);
                        let normal = safe_unit(
                            frame.vector(mix(gradient(coords[a]), gradient(coords[b]), amount)),
                            unit(position),
                        );
                        let uv = global.local(position);
                        crossing.push(Vertex {
                            position,
                            normal,
                            geometric: normal,
                            axis: normal,
                            uv: [uv[0] - uv_origin[0], uv[2] - uv_origin[2]],
                            flow: [0.; 2],
                            depth: water.settings.radius * 2.,
                            thickness: water.settings.radius * 2.,
                            class: 4,
                            flags: 0,
                        });
                    }
                    if crossing.len() == 3 {
                        mesh.triangle([crossing[0], crossing[1], crossing[2]]);
                    } else if crossing.len() == 4 {
                        let centre = mul(
                            crossing.iter().fold([0.; 3], |a, p| add(a, p.position)),
                            0.25,
                        );
                        let normal = unit(crossing.iter().fold([0.; 3], |a, p| add(a, p.normal)));
                        let axis = safe_unit(sub(crossing[0].position, centre), frame.right);
                        let side = cross(normal, axis);
                        crossing.sort_by(|a, b| {
                            let a = sub(a.position, centre);
                            let b = sub(b.position, centre);
                            dot(a, side)
                                .atan2(dot(a, axis))
                                .total_cmp(&dot(b, side).atan2(dot(b, axis)))
                        });
                        mesh.triangle([crossing[0], crossing[1], crossing[2]]);
                        mesh.triangle([crossing[0], crossing[2], crossing[3]]);
                    }
                }
            }
        }
    }
}

// Independent scalar samples retain their original row-major assembly order.
// Scoped workers borrow immutable inputs; native graphics and Host stay on their owners.
fn sample_grid<T:Send>(side:usize,parallel:bool,sample:&(impl Fn(usize,usize)->T+Sync))->Vec<T>{
    let rows=side+1;
    let chunk=|begin:usize,end:usize|{let mut out=Vec::with_capacity((end-begin)*rows);for z in begin..end{for x in 0..rows{out.push(sample(x,z));}}out};
    if !parallel{return chunk(0,rows);}
    std::thread::scope(|scope|{
        let mut jobs=Vec::new();
        for i in 0..3 {let a=rows*i/3;let b=rows*(i+1)/3;let build=&chunk;
            match std::thread::Builder::new().name("mirror-water-samples".into()).spawn_scoped(scope,move||{let _scope=crate::host::performance_telemetry::Scope::new(crate::host::performance_telemetry::Category::GameLogic);build(a,b)}){
                Ok(job)=>jobs.push((Some(job),None)),Err(_)=>jobs.push((None,Some(chunk(a,b))))
            }
        }
        let mut out=Vec::with_capacity(rows*rows);
        for (job,ready) in jobs {let mut values=if let Some(job)=job{job.join().unwrap_or_else(|p|std::panic::resume_unwind(p))}else{ready.unwrap()};out.append(&mut values);}out
    })
}
#[cfg(test)]mod parallel_water_tests {
 use super::*;
 #[test]fn parallel_water_sampling_preserves_every_vertex_and_triangle(){
  let r=crate::terrain::RADIUS;let bodies=bodies::State::parse(&serde_json::json!({"ocean":false,"generated":true,"rivers":[],"sources":[],"pools":[{"min":[-40.,r-70.,-300.],"max":[40.,r-64.,-240.]}]})).unwrap();
  for focus in [[0.,r+8.,0.],[20.,r-40.,-60.],[25.,r-64.,-272.]]{
   let a=free_surfaces_impl(focus,[0.,0.,-1.],&bodies,0.,&Default::default(),&Default::default(),false);
   let b=free_surfaces_impl(focus,[0.,0.,-1.],&bodies,0.,&Default::default(),&Default::default(),true);
   assert_eq!(a.0.origin.map(f64::to_bits),b.0.origin.map(f64::to_bits));assert_eq!(a.1,b.1);
  }
 }
}

#[cfg(test)]mod visibility_mesh_tests {
 use super::*;
 #[test]fn early_water_culling_preserves_every_potentially_visible_reference_triangle(){
  use std::collections::HashSet;
  let r=crate::terrain::RADIUS;
  let bodies=bodies::State::parse(&serde_json::json!({"ocean":false,"generated":true,"rivers":[],"sources":[],"pools":[{"min":[-40.,r-70.,-300.],"max":[40.,r-64.,-240.]}],"sheets":[[[0.,r+10.,-50.],[1.,r+10.,-50.],[0.,r+5.,-60.]]]})).unwrap();
  let signature=|t:&[Vertex]|{let mut s=Vec::new();for v in t {for a in [v.position,v.normal,v.geometric,v.axis]{s.extend(a.map(f64::to_bits));}s.extend(v.uv.map(f64::to_bits));s.extend(v.flow.map(f64::to_bits));s.extend([v.depth.to_bits(),v.thickness.to_bits(),v.class as u64,v.flags as u64]);}s};
  let mut skipped=0;
  for focus in [[0.,r+8.,0.],[20.,r-40.,-60.],[25.,r-64.,-272.]]{
   let(_,reference)=free_surfaces_impl(focus,[0.,0.,-1.],&bodies,0.,&Default::default(),&Default::default(),false);
   let up=unit(focus);
   for forward in [[0.,0.,-1.],[1.,0.,0.],[0.,0.,1.]]{for aspect in [0.5,1.6,3.]{
    let right=unit(cross(forward,up));let forward=unit(cross(up,right));
    let view=super::super::visibility::View::parse(&serde_json::json!({"position":focus,"right":right,"up":up,"forward":forward,"aspect":aspect})).unwrap();
    let(_,culled)=free_surfaces_impl_visible(focus,forward,&bodies,0.,&Default::default(),&Default::default(),false,Some(&view));
    skipped+=culled.visibility.grid_cells_skipped;
    let kept:HashSet<_>=culled.vertices.chunks_exact(3).map(&signature).collect();
    for t in reference.vertices.chunks_exact(3){if view.triangle([t[0].position,t[1].position,t[2].position]){assert!(kept.contains(&signature(t)),"visible water removed at {focus:?}, {forward:?}, aspect {aspect}");}}
    assert!(culled.vertices.len()<=reference.vertices.len());
   }}
  }
  assert!(skipped>100_000,"must actually avoid sampling hidden cells");
 }
}

#[cfg(test)]mod wet_clip_tests {
 use super::*;
 #[test]fn fixed_wet_clip_matches_reference_at_shores_and_zero_distance(){
  let vertex=|i:usize|Vertex{position:[[0.,0.,-1.],[1.,0.,-1.],[0.,0.,1.]][i],normal:[0.,1.,0.],geometric:[0.,1.,0.],axis:[0.,1.,0.],uv:[i as f64,0.],flow:[0.1,0.2],depth:2.,thickness:2.,class:3,flags:2};
  for a in [-1.,-1e-14,0.,1e-14,1.] {for b in [-1.,-1e-14,0.,1e-14,1.] {for c in [-1.,-1e-14,0.,1e-14,1.] {
   let source=[(vertex(0),a),(vertex(1),b),(vertex(2),c)];let mut expected=Mesh::default();let mut polygon=Vec::with_capacity(4);
   for i in 0..3{let(a,da)=source[i];let(b,db)=source[(i+1)%3];if da>=0.{polygon.push(a);}if(da>=0.)!=(db>=0.){polygon.push(interpolate(a,b,da/(da-db)));}}
   for i in 1..polygon.len().saturating_sub(1){expected.triangle([polygon[0],polygon[i],polygon[i+1]]);}
   let mut actual=Mesh::default();clip_wet_triangle(&mut actual,source);assert_eq!(actual,expected);
  }}}
 }
}
