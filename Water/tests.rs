//! Behavioral obligations for the continuous adaptation. Run in Rewrite's
//! serialized validation lane; none of these establish native GPU presentation.
use super::{floodgate, hydrology, math::*, slides, soft_body, types::Snapshot};
fn floor(revision: u64, height: f64) -> hydrology::State {
    let snapshot = Snapshot {
        origin: [0., crate::terrain::RADIUS, 0.],
        vertices: vec![
            [-20., height as f32, -20.],
            [20., height as f32, -20.],
            [20., height as f32, 20.],
            [-20., height as f32, 20.],
        ],
        triangles: vec![[0, 2, 1], [0, 3, 2]],
        revision,
        min: [-20., -5., -20.],
        max: [20., 20., 20.],
    };
    let mut state = hydrology::State::default();
    state.update(snapshot, &slides::Mesh::default(), 0);
    state
}
#[test]
fn water_index_reuses_static_faces_and_removes_moved_or_deleted_material() {
    let mut ground=floor(1,0.);
    let r=crate::terrain::RADIUS;
    let base=ground.snapshot.as_ref().unwrap().clone();
    let mut moved=base.clone();
    moved.vertices.extend([[-1.,2.,-1.],[1.,2.,-1.],[0.,2.,1.]]);
    moved.triangles.push([4,6,5]); moved.revision=2;
    ground.update(moved.clone(),&slides::Mesh::default(),0);
    let trace=|g:&hydrology::State,x:f64|g.ray([x,r+5.,0.],[0.,-1.,0.],10.).unwrap().point[1]-r;
    assert!((trace(&ground,0.)-2.).abs()<1e-6);
    for v in &mut moved.vertices[4..] {v[0]+=5.;}
    moved.revision=3;
    ground.update(moved,&slides::Mesh::default(),0);
    assert!(trace(&ground,0.).abs()<1e-6);
    assert!((trace(&ground,5.)-2.).abs()<1e-6);
    let mut removed=base.clone();removed.revision=4;
    ground.update(removed,&slides::Mesh::default(),0);
    assert!(trace(&ground,5.).abs()<1e-6);
    let mut raised=base;raised.revision=5;raised.origin[1]+=1.;
    ground.update(raised,&slides::Mesh::default(),0);
    assert!((trace(&ground,0.)-1.).abs()<1e-6);
}
#[test]
fn continuous_water_sweeps_thin_triangles_and_allows_retreat() {
    let ground = floor(1, 0.);
    let r = crate::terrain::RADIUS;
    let hit = ground
        .sweep([0., r + 2., 0.], [0., r - 2., 0.], 0.48)
        .expect("thin ground must stop a fast parcel");
    assert!((hit.point[1] - (r + 0.48)).abs() < 1e-5);
    assert!(dot(hit.normal, [0., 1., 0.]) > 0.999);
    assert!(
        ground.sweep(hit.point, [0., r + 1., 0.], 0.48).is_none(),
        "support must permit retreat"
    );
    assert!(
        ground
            .sweep(hit.point, add(hit.point, [1., 0., 0.]), 0.48)
            .is_none(),
        "support must permit tangential flow"
    );
}
#[test]
fn continuous_water_rests_wakes_and_preserves_its_material() {
    let mut ground = floor(1, 0.);
    let mut water = soft_body::State::default();
    let r = crate::terrain::RADIUS;
    let id = water
        .spawn([0., r + 0.48, 0.], [0.; 3], Some(1), None)
        .unwrap();
    for _ in 0..180 {
        water.step(1. / 60., &ground).unwrap();
    }
    assert_eq!(water.parcels.len(), 1);
    assert!(
        water.parcels[0].resting,
        "supported quiet material should rest continuously"
    );
    assert_eq!(water.emitted - water.removed, water.parcels.len() as u64);
    ground = floor(2, 0.2);
    water.step(1. / 60., &ground).unwrap();
    assert!(!water.parcels[0].resting);
    assert_eq!(water.parcels[0].id, id);
    assert!(
        water.parcels[0].position[1] >= r + 0.68 - 1e-5,
        "accepted terrain revision must wake and support the same parcel"
    );
    water.disturb(water.parcels[0].position, 1.5, [1., 0., 0.]);
    assert!(!water.parcels[0].resting);
    assert_eq!(water.emitted - water.removed, 1);
}
#[test]
fn continuous_water_unknown_coverage_never_becomes_a_floor_or_sink() {
    let ground = floor(1, 0.);
    let mut water = soft_body::State::default();
    let p = [0., crate::terrain::RADIUS + 30., 0.];
    water.spawn(p, [0., -20., 0.], None, None).unwrap();
    for _ in 0..3 {
        water.step(1. / 60., &ground).unwrap();
    }
    assert_eq!(water.parcels[0].position, p);
    assert!(water.coverage_needed);
    assert!(!water.parcels[0].resting);
    assert_eq!(water.parcels.len(), 1);
}
#[test]
fn continuous_water_pair_forces_do_not_create_internal_momentum() {
    let ground = floor(1, 0.);
    let mut water = soft_body::State::default();
    water.settings.gravity = 0.;
    water.settings.air_drag = 0.;
    water.settings.downhill = 0.;
    let r = crate::terrain::RADIUS;
    water
        .spawn([-0.6, r + 5., 0.], [0.1, 0., 0.], None, None)
        .unwrap();
    water
        .spawn([0.6, r + 5., 0.], [-0.1, 0., 0.], None, None)
        .unwrap();
    water.step(1. / 60., &ground).unwrap();
    let momentum = water
        .parcels
        .iter()
        .fold([0.; 3], |v, p| add(v, p.velocity));
    assert!(len(momentum) < 1e-10);
    assert_eq!(water.emitted - water.removed, 2);
}
#[test]
fn continuous_source_volume_conserves_ownership_without_settled_blocks() {
    let ground = floor(1, 0.);
    let mut water = soft_body::State::default();
    let source = super::bodies::Source {
        id: 7,
        frame: Frame::radial([0., crate::terrain::RADIUS, 0.], [0., 0., 1.]),
        min: [-2., 1., -2.],
        max: [2., 3., 2.],
        target: 4,
        velocity: [0.; 3],
    };
    for _ in 0..8 {
        water
            .fill_sources(std::slice::from_ref(&source), &ground)
            .unwrap();
    }
    assert_eq!(water.parcels.len(), 4);
    assert!(
        water
            .parcels
            .iter()
            .all(|p| p.source == Some(7) && ground.covered(p.position, water.settings.radius))
    );
    let ids: Vec<_> = water.parcels.iter().map(|p| p.id).collect();
    for _ in 0..180 {
        water.step(1. / 60., &ground).unwrap();
    }
    assert_eq!(ids, water.parcels.iter().map(|p| p.id).collect::<Vec<_>>());
    assert_eq!(water.emitted - water.removed, 4);
}
#[test]
fn floodgate_exit_recycles_same_material_and_disable_returns_only_owned_water() {
    let ground = floor(1, 0.);
    let r = crate::terrain::RADIUS;
    let mut water = soft_body::State::default();
    let mut gate = floodgate::State::default();
    let pose = |z| slides::Pose {
        position: [0., r + 3., z],
        right: [1., 0., 0.],
        up: [0., 1., 0.],
        forward: [0., 0., 1.],
        radius: 3.,
    };
    gate.course = vec![pose(0.), pose(15.)];
    gate.settings.target = 1;
    gate.step(&mut water, &ground).unwrap();
    assert_eq!(water.parcels.len(), 1);
    assert_eq!(gate.spawned, 1);
    let id = water.parcels[0].id;
    let exit = gate.pose(0.995).unwrap();
    water.parcels[0].position = add(exit.position, mul(exit.forward, 0.1));
    gate.step(&mut water, &ground).unwrap();
    assert_eq!(water.parcels[0].id, id);
    assert_eq!(gate.recycled, 1);
    assert_eq!(water.emitted - water.removed, 1);
    water
        .spawn([10., r + 3., 0.], [0.; 3], Some(99), None)
        .unwrap();
    gate.settings.enabled = false;
    gate.step(&mut water, &ground).unwrap();
    assert_eq!(water.parcels.len(), 1);
    assert_eq!(water.parcels[0].source, Some(99));
    assert_eq!(water.emitted - water.removed, 1);
}

#[test]
fn recycling_preserves_existing_material_when_source_is_blocked_or_unknown() {
    let r = crate::terrain::RADIUS;
    for unknown in [false, true] {
        // Both emitter rows (y=2 and y=2.88) must intersect the
        // triangle boundary with their 0.48m radius. A floor at 2.9
        // leaves the lower row clear on the other side of the triangle.
        let ground = floor(1, if unknown { 0. } else { 2.44 });
        let mut water = soft_body::State::default();
        let mut gate = floodgate::State::default();
        let pose = |x, z| slides::Pose {
            position: [x, r + 3., z],
            right: [1., 0., 0.],
            up: [0., 1., 0.],
            forward: [0., 0., 1.],
            radius: 3.,
        };
        gate.course = vec![pose(if unknown { 100. } else { 0. }, 0.), pose(0., 15.)];
        gate.settings.target = 1;
        let exit = gate.pose(0.995).unwrap();
        let position = add(exit.position, mul(exit.forward, 0.1));
        let velocity = [0., 0., 0.75];
        let id = water.spawn(position, velocity, None, Some(0)).unwrap();
        gate.step(&mut water, &ground).unwrap();
        assert_eq!(water.parcels.len(), 1);
        assert_eq!(water.parcels[0].id, id);
        assert_eq!(water.parcels[0].position, position);
        assert_eq!(water.parcels[0].velocity, velocity);
        assert_eq!(gate.recycled, 0);
        assert_eq!(gate.spawned, 0);
        assert_eq!(water.emitted - water.removed, 1);
        assert_eq!(water.coverage_needed, unknown);
        if unknown {
            assert!(!water.coverage_points.is_empty());
        }
    }
}
#[test]
fn rendered_trough_triangles_are_the_particle_collision_boundary() {
    let r = crate::terrain::RADIUS;
    let mut slide = slides::State::default();
    slide
        .place([0., r, 0.], [0., 1., 0.], [0., 0., 1.], true)
        .unwrap();
    slide
        .place([0., r, 4.], [0., 1., 0.], [0., 0., 1.], false)
        .unwrap();
    let mesh = slide.mesh().unwrap();
    assert!(!mesh.triangles.is_empty());
    let mut ground = floor(1, -3.);
    let snapshot = ground.snapshot.take().unwrap();
    ground.update(snapshot, &mesh, slide.revision);
    let centre = [0., r + slide.radius + slide.settings.lift, 2.];
    let hit = ground
        .sweep(centre, add(centre, [0., -2., 0.]), 0.05)
        .unwrap();
    assert!(
        hit.point[1] > r,
        "the real trough must stop the parcel above the ground floor"
    );
    assert!(hit.point[1] < centre[1]);
}
#[test]
fn tide_preserves_continuous_high_quarter_low_cycle() {
    let mut tide = super::tide::State::default();
    assert_eq!(tide.sample().height, 0.);
    tide.age = tide.settings.period * 0.25;
    assert!((tide.sample().height + 3.5).abs() < 1e-12);
    tide.age = tide.settings.period * 0.5;
    assert_eq!(tide.sample().height, -7.);
    tide.age = tide.settings.period * 0.75;
    assert!(tide.sample().rising);
}

#[test]
fn explicit_source_course_selects_its_chain_and_rejects_missing_or_cyclic_paths() {
    let r = crate::terrain::RADIUS;
    let mut slide = slides::State::default();
    let up = [0., 1., 0.];
    let forward = [0., 0., 1.];
    let first = slide.place([0., r, 0.], up, forward, true).unwrap();
    slide.place([0., r, 4.], up, forward, false).unwrap();
    let second = slide.place([10., r, 0.], up, forward, true).unwrap();
    let end = slide.place([10., r, 4.], up, forward, false).unwrap();
    let course = slide.course_from(Some(second)).unwrap();
    assert!(!course.is_empty());
    assert!(
        course
            .iter()
            .all(|pose| (pose.position[0] - 10.).abs() < 1e-8)
    );
    assert!(slide.course_from(Some(end)).is_err());
    assert!(slide.course_from(Some(u64::MAX)).is_err());
    assert!(
        slide
            .course_from(Some(first))
            .unwrap()
            .iter()
            .all(|pose| pose.position[0].abs() < 1e-8)
    );
    slide.connect(end, second).unwrap();
    assert!(slide.course_from(Some(second)).is_err());
}
