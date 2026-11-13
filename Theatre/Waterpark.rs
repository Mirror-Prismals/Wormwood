//! Blender-authored flumes and landscaped grotto; all positions use venue metres.
use super::*;
use crate::audio::cross;
#[derive(Clone)]
struct Face { points:[V;3], min:V, max:V }
pub(crate) struct Park {
    pub offset:V,
    faces:Vec<Face>,
    pub rides:Vec<Vec<V>>,
    pub pools:Vec<(V,V)>,
    river:Vec<(V,V)>,
    features:Vec<(V,V,f64)>,
    bins:std::collections::HashMap<[i32;3],Vec<usize>>,
    large:Vec<usize>,
}

impl Park {
    /// Static supplementary architecture uses the same swept capsule solver
    /// and triangle broadphase as the waterpark, without fabricating rides.
    pub(crate) fn collision_mesh(triangles:Vec<[V;3]>)->Self {
        let faces:Vec<_>=triangles.into_iter().map(|points| {
            let min=std::array::from_fn(|i|points.iter().map(|p|p[i]).fold(f64::INFINITY,f64::min));
            let max=std::array::from_fn(|i|points.iter().map(|p|p[i]).fold(f64::NEG_INFINITY,f64::max));
            Face{points,min,max}
        }).collect();
        let mut bins=std::collections::HashMap::<[i32;3],Vec<usize>>::new();let mut large=Vec::new();
        for (index,face) in faces.iter().enumerate(){
            let a=face.min.map(|x|(x/4.).floor() as i32);let b=face.max.map(|x|(x/4.).floor() as i32);
            if (0..3).map(|i|(b[i]-a[i]+1) as usize).product::<usize>()>512 {large.push(index);continue;}
            for x in a[0]..=b[0]{for y in a[1]..=b[1]{for z in a[2]..=b[2]{bins.entry([x,y,z]).or_default().push(index);}}}
        }
        Self{offset:[0.;3],faces,rides:Vec::new(),pools:Vec::new(),river:Vec::new(),features:Vec::new(),bins,large}
    }
    pub(crate) fn extend_collision(&mut self,triangles:Vec<[V;3]>) {
        let all=self.faces.iter().map(|f|f.points).chain(triangles).collect();
        let mesh=Self::collision_mesh(all);self.faces=mesh.faces;self.bins=mesh.bins;self.large=mesh.large;
    }
    pub fn water_sheets(&self,origin:V)->Vec<[V;3]> {self.flowing_sheets(origin,None)}
    pub fn flowing_sheets(&self,origin:V,flow:Option<&Flow>)->Vec<[V;3]> {
        let mut triangles=Vec::new();
        for (course,path) in self.rides.iter().enumerate() {for (i,pair) in path.windows(2).enumerate() {
            if flow.and_then(|f|f.cells.get(course)).and_then(|c|c.get(i)).is_some_and(|c|c.depth<0.004){continue;}
            let forward=unit(sub(pair[1],pair[0]));let right=unit(cross([0.,1.,0.],forward));let up=unit(cross(forward,right));
            let strip=|p:V,sign:f64|add(origin,add(p,add(mul(up,-1.46),mul(right,sign*0.42))));
            let a=strip(pair[0],-1.);let b=strip(pair[0],1.);let c=strip(pair[1],1.);let d=strip(pair[1],-1.);
            triangles.extend([[a,b,c],[a,c,d]]);
        }}
        for &(a,b,width) in &self.features {
            let right=mul(unit(cross(unit(sub(b,a)),[0.,0.,1.])),width*0.5);
            let a=add(a,origin);let b=add(b,origin);
            triangles.extend([[sub(a,right),add(a,right),add(b,right)],[sub(a,right),add(b,right),sub(b,right)]]);
        }
        triangles
    }
    fn nearby(&self,min:V,max:V)->Vec<&Face> {
        let a=min.map(|x|(x/4.).floor() as i32);let b=max.map(|x|(x/4.).floor() as i32);
        // Long teleport queries retain a bounded full-face broadphase.
        if (0..3).map(|i|(b[i]-a[i]+1).max(0) as usize).product::<usize>()>4096 {
            return self.faces.iter().filter(|f|(0..3).all(|i|f.max[i]>=min[i]&&f.min[i]<=max[i])).collect();
        }
        let mut indices=self.large.clone();
        for x in a[0]..=b[0]{for y in a[1]..=b[1]{for z in a[2]..=b[2]{if let Some(rows)=self.bins.get(&[x,y,z]){indices.extend(rows);}}}}
        indices.sort_unstable();indices.dedup();
        indices.into_iter().map(|i|&self.faces[i]).filter(|f|(0..3).all(|i|f.max[i]>=min[i]&&f.min[i]<=max[i])).collect()
    }
    pub fn collision_vertices(&self,focus:V,origin:V)->Vec<V> {
        self.collision_vertices_radius(focus,origin,24.)
    }
    pub(crate) fn collision_vertices_radius(&self,focus:V,origin:V,radius:f64)->Vec<V>{
        self.nearby(focus.map(|x|x-radius),focus.map(|x|x+radius)).into_iter().flat_map(|f|f.points.map(|p|add(p,origin))).collect()
    }
    pub fn resolve(&self,previous:V,desired:V,eye:f64)->(V,Vec<V>) {
        let mut p=previous;let delta=sub(desired,previous);let steps=(length(delta)/0.10).ceil().clamp(1.,128.) as usize;let mut normals=Vec::new();
        // Nearby triangles only. Bounds include the swept capsule and step rise.
        let min:V=std::array::from_fn(|i|previous[i].min(desired[i])-if i==1 {eye+0.5}else{0.6});
        let max:V=std::array::from_fn(|i|previous[i].max(desired[i])+0.6);
        let candidates:Vec<_>=self.nearby(min,max);
        if candidates.is_empty(){return(desired,normals);}
        for _ in 0..steps {
            let old=p;p=add(p,mul(delta,1./steps as f64));
            // Triangle-based stair support permits 18cm risers without turning
            // the decorative treads into a hidden ramp or a teleport.
            let mut support=None::<f64>;
            for f in &candidates {
                let [a,b,c]=f.points;let n=cross(sub(b,a),sub(c,a));
                if n[1].abs()<0.5*length(n){continue;}
                let y=a[1]-(n[0]*(p[0]-a[0])+n[2]*(p[2]-a[2]))/n[1];
                if y>old[1]-eye+0.205 || y<p[1]-eye-0.10 {continue;}
                let q=[p[0],y,p[2]];
                if length(sub(crate::water::hydrology::closest(q,f.points),q))<0.30 {support=Some(support.map_or(y,|s|s.max(y)));}
            }
            if let Some(y)=support {if p[1]-eye<y+0.015 {p[1]=y+eye;push_normal(&mut normals,[0.,1.,0.]);}}
            for _ in 0..3 {
                let mut moved=false;
                for f in &candidates {for dy in [-eye+0.28,-eye*0.5,-0.20] {
                    let center=add(p,[0.,dy,0.]);
                    if (0..3).any(|i|center[i]<f.min[i]-0.29 || center[i]>f.max[i]+0.29){continue;}
                    let q=crate::water::hydrology::closest(center,f.points);let d=sub(center,q);let distance=length(d);
                    if distance>1e-7 && distance<0.275 {
                        let n=mul(d,1./distance);p=add(p,mul(n,0.275-distance));push_normal(&mut normals,n);moved=true;
                    }
                }}
                if !moved {break;}
            }
        }
        (p,normals)
    }
    /// A seated body moves in free 3D. Only contact with the authored triangles
    /// removes inward velocity; no nearest-path projection or minimum speed.
    pub(crate) fn tube_region(&self,position:V)->bool {
        let p=sub(position,self.offset);
        ((-134. ..=-76.).contains(&p[0]) || (63. ..=127.).contains(&p[0])) && (-60. ..=35.).contains(&p[2]) && (0. ..=30.).contains(&p[1])
    }
    pub fn surf(&self,position:V,mut velocity:V,steering:V,dt:f64,water:V,wet:bool)->Option<(V,V,bool)> {
        let local=sub(position,self.offset);
        if !(-134. ..=127.).contains(&local[0]) || !(-95. ..=60.).contains(&local[1]) || !(-82. ..=45.).contains(&local[2]) || !dt.is_finite() || !(0. ..=0.1).contains(&dt){return None;}
        let tube=self.tube_region(position);let radius=if tube{0.87}else{0.29};let eye=0.60;let mut center=add(position,[0.,-eye+radius,0.]);
        let reach=length(velocity)*dt+1.5;
        let candidates:Vec<_>=self.nearby(center.map(|x|x-reach),center.map(|x|x+reach));
        let count=((length(velocity)*dt/0.07).ceil() as usize).max((dt*240.).ceil() as usize).clamp(1,128);
        let h=dt/count as f64;let mut grounded=false;
        for _ in 0..count {
            velocity=add(velocity,mul(add([0.,-9.81,0.],steering),h));
            if tube {
                for (a,b) in &self.pools {
                    let foot=center[1]-radius;
                    if center[0]>a[0]&&center[0]<b[0]&&center[2]>a[2]&&center[2]<b[2]&&foot<b[1]+0.2&&foot>a[1]-0.5 {
                        velocity[1]+=(9.81+(b[1]-0.15-foot)*30.-velocity[1]*5.).clamp(-40.,45.)*h;
                    }
                }
            }
            // Water drag exchanges momentum; it cannot impose a rail or speed floor.
            if wet {velocity=add(velocity,mul(sub(water,velocity),1.-(-1.2*h).exp()));}
            center=add(center,mul(velocity,h));
            let mut contact=false;
            for _ in 0..4 {
                let mut hit=false;
                for f in &candidates {
                    if (0..3).any(|i|center[i]<f.min[i]-radius || center[i]>f.max[i]+radius){continue;}
                    let q=crate::water::hydrology::closest(center,f.points);let delta=sub(center,q);let d=length(delta);
                    if d>1e-9 && d<radius {
                        let n=mul(delta,1./d);center=add(center,mul(n,radius-d+1e-6));
                        let inward=dot(velocity,n).min(0.);velocity=sub(velocity,mul(n,inward));
                        grounded|=n[1]>0.35;hit=true;contact=true;
                    }
                }
                if !hit{break;}
            }
            if contact {
                let friction=if wet{0.015}else{0.22};let speed=length(velocity);
                velocity=mul(velocity,(1.-friction*9.81*h/speed.max(1e-9)).max(0.));
            }
            let speed=length(velocity);if speed>45. {velocity=mul(velocity,45./speed);}
        }
        Some((add(center,[0.,eye-radius,0.]),velocity,grounded))
    }
    /// Sample a wet film, never a rider position constraint. Above the rounded
    /// bed or outside the flume the sample is dry, even near a centreline.
    pub fn film_sample(&self,flow:&Flow,point:V)->(bool,V) {
        let mut best=0.65;let mut result=(false,[0.;3]);
        for (course,path) in self.rides.iter().enumerate() {for (i,pair) in path.windows(2).enumerate() {
            let f=unit(sub(pair[1],pair[0]));let right=unit(cross([0.,1.,0.],f));let up=unit(cross(f,right));
            let along=dot(sub(point,pair[0]),f).clamp(0.,length(sub(pair[1],pair[0])));
            let bed=add(add(pair[0],mul(f,along)),mul(up,-1.48));let d=length(sub(point,bed));
            if d<best {best=d;let cell=flow.cells.get(course).and_then(|c|c.get(i));
                if let Some(cell)=cell {result=(cell.depth>0.004,mul(f,cell.speed));}
            }
        }}
        for &(a,b,_) in &self.features {
            let d=sub(b,a);let t=(dot(sub(point,a),d)/dot(d,d).max(1e-9)).clamp(0.,1.);
            if length(sub(point,add(a,mul(d,t))))<0.5 {result=(true,mul(unit(d),5.));}
        }
        for (i,(a,b)) in self.river.iter().enumerate() {
            if (0..3).all(|k|point[k]>=a[k]&&point[k]<=b[k]) {
                return (true,match i {0=>[0.65,0.,0.],1=>[-0.65,0.,0.],2=>[0.,0.,-0.65],_=>[0.,0.,0.65]});
            }
        }
        if !result.0 && self.pools.iter().any(|(a,b)|(0..3).all(|i|point[i]>=a[i]&&point[i]<=b[i])) {result=(true,[0.;3]);}
        result
    }
    pub fn sources(&self,origin:V)->Vec<crate::water::bodies::Source> {
        // Independent bounded upward jets use the existing conserved parcel
        // solver, including collision and two-way disturbance by the rider.
        [-32.,-27.,-22.,-17.,-12.].iter().enumerate().map(|(i,x)|crate::water::bodies::Source {
            id:9000+i as u64,frame:crate::water::math::Frame{origin:add(origin,add(self.offset,[*x,0.65,14.])),right:[1.,0.,0.],up:[0.,1.,0.],forward:[0.,0.,1.]},
            min:[-0.55,-0.55,-0.55],max:[0.55,0.55,0.55],target:12,velocity:[0.,8.,0.],
        }).collect()
    }
}
#[derive(Clone,Copy,Default)]
struct Film { depth:f64,speed:f64 }
#[derive(Default)]
pub(crate) struct Flow { cells:Vec<Vec<Film>>,pub gate_open:Vec<bool>,initialized:bool }
impl Flow {
    /// Finite-volume water film: gravity/friction update discharge velocity;
    /// upwind flux conserves water between cells. Closing a gate drains the run.
    pub fn step(&mut self,park:&Park,dt:f64) {
        if !dt.is_finite() || !(0. ..=0.1).contains(&dt){return;}
        if !self.initialized {
            self.cells=park.rides.iter().map(|p|vec![Film{depth:0.025,speed:1.};p.len()-1]).collect();
            self.gate_open=vec![true;park.rides.len()];self.initialized=true;
        }
        let steps=(dt*240.).ceil().max(1.) as usize;let h=dt/steps as f64;
        for (k,path) in park.rides.iter().enumerate() {
            let cells=&mut self.cells[k];let mut flux=vec![0.;cells.len()+1];let mut depths=vec![0.;cells.len()];let mut old=cells.clone();
            for _ in 0..steps {
            flux[0]=if self.gate_open[k] {0.10}else{0.};
            for (d,c) in depths.iter_mut().zip(cells.iter()){*d=c.depth;}
            for (i,pair) in path.windows(2).enumerate() {
                let dx=length(sub(pair[1],pair[0]));
                let upstream=if i==0{depths[i]}else{depths[i-1]};
                let downstream=depths.get(i+1).copied().unwrap_or(0.);
                let pressure=9.81*(upstream-downstream)/(2.*dx);
                let slope=-unit(sub(pair[1],pair[0]))[1];let c=&mut cells[i];
                c.speed=(c.speed+h*(9.81*slope+pressure-0.014*c.speed*c.speed)).clamp(0.,28.);
                flux[i+1]=(c.depth*c.speed).min(c.depth*length(sub(pair[1],pair[0]))/h.max(1e-9));
            }
            old.copy_from_slice(cells);
            for i in 0..cells.len(){
                let dx=length(sub(path[i+1],path[i]));let incoming=if i==0{1.}else{old[i-1].speed};
                let depth=(old[i].depth+h*(flux[i]-flux[i+1])/dx).max(0.);
                let momentum=(old[i].depth*old[i].speed+h*(flux[i]*incoming-flux[i+1]*old[i].speed)/dx).max(0.);
                cells[i]=Film{depth,speed:if depth>1e-9{(momentum/depth).clamp(0.,28.)}else{0.}};
            }
        }}
    }

}
/// Keep the cave floor above the untouched continuous ground. The hill's crown
/// is the park's ground level; its cavern and daylight stair are below it.
/// Deterministic common datum for the venue and its geological excavations.
pub(crate) fn theatre_origin()->V {super::foundation_origin()}
pub(crate) fn site_offset(origin:V)->V {
 let v=crate::modding::call_selected("prismals","Prismals.ParkOffset",&json!(origin)).expect("selected Prismals park placement");vector(&v).expect("valid mod placement")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn venue()->Venue {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));let mut v=super::super::load(root).unwrap();
        v.add_approach(-1.).unwrap();v.add_waterpark(root,[0.,14.,-235.],-1.,[0.,crate::terrain::RADIUS+crate::terrain::height([0.,1.,0.])+0.5,0.]).unwrap();v
    }
    #[test]
    fn waterpark_funnel_tube_and_lazy_river() {
        let v=venue();let park=v.park.as_ref().unwrap();let mut flow=Flow::default();
        flow.step(park,1./60.);assert_eq!(flow.gate_open.len(),3);assert_eq!(park.river.len(),4);assert_eq!(park.pools.len(),8);
        for (a,b) in &park.river {
            let point=mul(add(*a,*b),0.5);let (wet,current)=park.film_sample(&flow,point);
            assert!(wet&&length(current)>0.5);
        }
        let basin=park.river[0];let mut p=add(mul(add(basin.0,basin.1),0.5),[0.,0.6,0.]);let mut velocity=[0.;3];
        for _ in 0..1200 {let (_,current)=park.film_sample(&flow,add(p,[0.,-0.45,0.]));let next=park.surf(p,velocity,[0.;3],1./120.,current,true).unwrap();p=next.0;velocity=next.1;}
        assert!(p[1]>basin.1[1]+0.25&&p[1]<basin.1[1]+0.7,"tube does not float {p:?}");
        let course=&park.rides[2];assert!(course[0][1]-course.last().unwrap()[1]>20.);
        let mut p=add(course[0],[0.,-0.9,0.]);let mut velocity=mul(unit(sub(course[1],course[0])),2.);let mut lowest=p[1];let mut arrived=false;let basin=*park.pools.last().unwrap();
        for _ in 0..24000 {let (wet,current)=park.film_sample(&flow,add(p,[0.,-0.45,0.]));let next=park.surf(p,velocity,[0.;3],1./120.,current,wet).unwrap();p=next.0;velocity=next.1;lowest=lowest.min(p[1]);if p[0]>basin.0[0]&&p[0]<basin.1[0]&&p[2]>basin.0[2]&&p[2]<basin.1[2]&&p[1]<basin.1[1]+1.{arrived=true;break;}}
        println!("funnel descent lowest {} local, end {:?}",lowest-park.offset[1],sub(p,park.offset));
        assert!(arrived,"funnel did not reach its receiving basin: {p:?}");
    }
    #[test]
    fn waterpark_surf_is_free_and_gate_water_drains() {
        let v=venue();let p=v.park.as_ref().unwrap();let mut flow=Flow::default();
        for _ in 0..1200{flow.step(p,1./60.);}
        for k in 0..2 {assert!(flow.cells[k].iter().any(|c|c.speed>5.));}
        let before: f64=flow.cells[0].iter().map(|c|c.depth).sum();flow.gate_open[0]=false;
        for _ in 0..6000{flow.step(p,1./60.);}
        assert!(flow.cells[0].iter().map(|c|c.depth).sum::<f64>()<before*0.2);
        let start=add(p.offset,[0.,55.,35.]);
        let (a,va,_)=p.surf(start,[3.,0.,0.],[0.;3],0.05,[0.;3],false).unwrap();
        let (b,_,_)=p.surf(start,[-3.,0.,0.],[0.;3],0.05,[0.;3],false).unwrap();
        assert!(a[0]>b[0]+0.29 && va[1]<0.); // No snapping, imposed forward travel, or speed floor.
        for path in p.rides.iter().take(2) {assert!(path[0][1]-path.last().unwrap()[1]>130.);}
    }
    #[test]
    fn waterpark_physical_descents_reach_deep_receiving_water() {
        let v=venue();let park=v.park.as_ref().unwrap();let mut flow=Flow::default();
        for _ in 0..1800{flow.step(park,1./60.);}
        for (k,path) in park.rides.iter().take(2).enumerate() {
            let mut pos=add(path[0],[0.,-0.9,0.]);let mut vel=mul(unit(sub(path[1],path[0])),2.);
            let mut arrived=false;
            for frame in 0..18000 {
                let (wet,current)=park.film_sample(&flow,add(pos,[0.,-0.45,0.]));
                let (next,v,_)=park.surf(pos,vel,[0.;3],1./120.,current,wet).unwrap();pos=next;vel=v;
                if pos[1]<park.offset[1]-87. && park.pools.iter().take(2).any(|(a,b)|pos[0]>a[0]&&pos[0]<b[0]&&pos[2]>a[2]&&pos[2]<b[2]) {arrived=true;break;}
                if frame%2400==0{eprintln!("slide {k} frame {frame}: {:?}, speed {}",sub(pos,park.offset),length(vel));}
                assert!(pos[1]>park.offset[1]-94.,"escaped course {k}: {:?}",sub(pos,park.offset));
            }
            assert!(arrived,"slide {k} stalled or escaped: {:?}, velocity {vel:?}",sub(pos,park.offset));
        }
    }
    #[test]
    fn waterpark_excavation_clears_slides_and_mine_and_reaches_depths() {
        let profile:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let field=crate::terrain::underground::Field::parse(&profile).unwrap().unwrap();
        let origin=theatre_origin();let offset=site_offset(origin);
        eprintln!("park radial datum {}",origin[1]+offset[1]-crate::terrain::RADIUS);
        let convert=|p:V|add(origin,add(offset,p));
        for row in profile["waterpark_excavation"]["tunnels"].as_array().unwrap() {
            let a=vector(&row["start"]).unwrap();let b=vector(&row["end"]).unwrap();
            for t in [0.,0.5,1.] {let p=convert(add(a,mul(sub(b,a),t)));assert!(field.distance(p,crate::terrain::height(unit(p)))>0.5,"blocked excavation at {:?}",crate::terrain::underground::local(p));}
        }
        let bottom=convert([0.,-88.,-53.]);let depth=crate::terrain::underground::local(bottom)[1];
        assert!(depth< -40. && depth>crate::terrain::BEDROCK_ELEVATION+4.,"cavern depth {depth}");
    }
    fn walk(v:&Venue,mut p:V,d:V,n:usize)->V {for _ in 0..n {p=v.resolve_body(p,add(p,d),2.).0;}p}
    #[test]
    fn waterpark_tower_stairs_and_cave_return_are_walkable_both_ways() {
        let v=venue();let o=v.park.as_ref().unwrap().offset;
        let mut p=add(o,[-13.,2.,18.]);
        for level in 0..12 {
            let dir=if level%2==0 {1.}else{-1.};let end=if level%2==0 {25.56}else{18.};
            let target=o[2]+end+dir*0.9;let count=((target-p[2]).abs()/0.06).ceil() as usize;
            p=walk(&v,p,[0.,-0.035,dir*0.06],count);
            assert!(p[1]>o[1]+(level+1) as f64*4.2+1.7,"tower level {level}: {p:?}");
            if level<11 {let dx=if level%2==0 {-0.06}else{0.06};p=walk(&v,p,[dx,-0.035,0.],67);}
        }
        assert!(p[1]>o[1]+52.);
        let mut p=add(o,[43.,-84.,-26.]);
        for level in 0..24 {
            let dir=if level%2==0 {1.}else{-1.};let end=if level%2==0 {-19.55}else{-26.};
            let count=((o[2]+end+dir*1.0-p[2]).abs()/0.06).ceil() as usize;
            p=walk(&v,p,[0.,-0.035,dir*0.06],count);
            assert!(p[1]>o[1]-86.+(level+1) as f64*(86./24.)+1.6,"mine flight {level}: {p:?}");
            if level<23{p=walk(&v,p,[if level%2==0{0.06}else{-0.06},-0.035,0.],84);}
        }
        assert!(p[1]>o[1]+1.6);

    }
    #[test]
    fn waterpark_road_clears_terrain_and_connects_the_plaza() {
        let v=venue();let p=walk(&v,[0.,1.,-94.],[0.,-0.04,-0.06],1640);
        assert!(p[2]< -191. && p[1]>15.5,"road end {p:?}");
        let ground_origin=crate::terrain::RADIUS+crate::terrain::height([0.,1.,0.])+0.5;
        let offset=site_offset([0.,ground_origin,0.]);
        for x in (-50..=50).step_by(10) {for z in (-300..=-195).step_by(10) {
            let n=unit([x as f64,crate::terrain::RADIUS,z as f64]);let y=n[1]*(crate::terrain::RADIUS+crate::terrain::height(n));
            assert!(offset[1]+ground_origin-12.55>y);
        }}
    }
    #[test]
    fn waterpark_pools_have_exact_native_surfaces_and_known_occupancy() {
        let v=venue();let p=v.park.as_ref().unwrap();let origin=[0.,crate::terrain::RADIUS,0.];
        let data=json!({"ocean":false,"generated":false,"rivers":[],"sources":[],"pools":p.pools.iter().map(|(min,max)|json!({"min":add(*min,origin),"max":add(*max,origin)})).collect::<Vec<_>>()});
        let bodies=crate::water::bodies::State::parse(&data).unwrap();let wave=crate::water::ocean::State::default();let h=crate::water::hydrology::State::default();
        for (min,max) in &p.pools {
            let center=add(mul(add(*min,*max),0.5),origin);
            assert!(bodies.query(center,0.,&wave,&h).is_some());
            assert!(bodies.query(add(center,[100.,0.,0.]),0.,&wave,&h).is_none());
            let (_,mesh)=crate::water::surface_mesh::free_surfaces(center,[0.,0.,-1.],&bodies,0.,&wave,&h);
            assert!(!mesh.truncated);assert!(mesh.vertices.iter().any(|v|(v.position[1]-(max[1]+origin[1])).abs()<1e-6));
            assert!(bodies.query([center[0],max[1]+origin[1]+0.2,center[2]],0.,&wave,&h).is_none());
        }
    }
}

impl Park {
 pub(crate) fn from_mod_packet(v:&Value)->Result<Self,String>{
  let triangles:Vec<[V;3]>=serde_json::from_value(v["collision"].clone()).map_err(|e|e.to_string())?;
  if triangles.len()>250000||triangles.iter().flatten().flatten().any(|x|!x.is_finite()){return Err("invalid mod collision mesh".into());}
  let mut p=Self::collision_mesh(triangles);p.offset=vector(&v["offset"])?;
  p.rides=serde_json::from_value(v["rides"].clone()).map_err(|e|e.to_string())?;
  p.pools=serde_json::from_value(v["pools"].clone()).map_err(|e|e.to_string())?;
  p.river=serde_json::from_value(v["river"].clone()).map_err(|e|e.to_string())?;
  p.features=serde_json::from_value(v["features"].clone()).map_err(|e|e.to_string())?;
  if p.rides.len()>256||p.pools.len()>256||p.river.len()>256||p.features.len()>1024||p.rides.iter().flatten().flatten().any(|x|!x.is_finite()){return Err("invalid mod flow network".into());}Ok(p)
 }
}
