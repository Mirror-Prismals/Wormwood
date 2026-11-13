use super::Host;
use crate::{
    audio::*,
    base_system::layer_five::crafting_system::{self, Item, Kind},
};
use serde_json::{Value, json};
fn host(name: &str) -> Host {
    Host::load_paths(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        &std::env::temp_dir().join(format!("mirror-crafting-{name}-{}", std::process::id())),
    )
    .unwrap()
}
fn run(h: &mut Host, hook: &str, v: Value) -> Value {
    h.stack.push((0, format!("CraftingSystem.{hook}")));
    let r = crafting_system::dispatch(h, hook, &v);
    h.stack.pop();
    r.unwrap()
}
fn command(p: V, f: bool, click: bool) -> Value {
    json!({"view":{"position":add(p,[0.,1.,0.]),"forward":[0.,-1.,0.],"up":[0.,0.,1.],"right":[1.,0.,0.]},"window":{"flight":{"focused":true,"phone":{"interact":f},"select":click,"workstation":{"editor":{"shift":true}}}}})
}
#[test]
fn crafting_cave_jars_are_supported_and_models_render() {
    let mut h = host("jars");
    let status = run(&mut h, "Configure", Value::Null);
    assert!(
        !h.crafting.items.is_empty(),
        "must actually spawn cave jars: {status}"
    );
    let data: Value =
        serde_json::from_slice(&std::fs::read(h.root().join("Procedures/caves.json")).unwrap())
            .unwrap();
    let field = crate::terrain::underground::Field::parse(&data)
        .unwrap()
        .unwrap();
    for i in &h.crafting.items {
        assert_eq!(i.kind, Kind::Jar);
        assert!(
            field.distance(
                add(i.position, mul(i.up, 0.3)),
                crate::terrain::height(i.up)
            ) > 0.24,
            "jar {:?} clearance {}",
            i.position,
            field.distance(
                add(i.position, mul(i.up, 0.3)),
                crate::terrain::height(i.up)
            )
        );
        assert!(
            field.distance(
                sub(i.position, mul(i.up, 0.025)),
                crate::terrain::height(i.up)
            ) < 0.
        );
    }
    let p = h.crafting.items[0].position;
    let view = json!({"position":add(p,[0.,0.7,2.]),"yaw":0.,"pitch":-0.2,"aspect":1.6});
    let mesh = crate::base_system::layer_four::crafting_render_system::frame(&h, &view).unwrap();
    assert!(!mesh.is_empty());
    assert!(mesh.iter().flatten().all(|v| v.is_finite()));
    run(&mut h, "Close", Value::Null);
    assert!(h.crafting.items.is_empty());
}
#[test]
fn crafting_actual_recipe_requires_tree_counts_time_and_makes_individual_planks() {
    let mut h = host("recipe");
    h.invoke("CaveSystem.Configure", Value::Null).unwrap();
    let p = [0., crate::terrain::RADIUS + 100., 0.];
    h.crafting.items.push(Item {
        id: 100,
        kind: Kind::Blueprint,
        position: p,
        up: [0., 1., 0.],
        right: [1., 0., 0.],
        job: None,
    });
    let result = run(&mut h, "Input", command(p, false, true));
    assert_eq!(result["consumed_select"], true);
    assert!(h.crafting.items[0].job.is_none());
    run(&mut h, "Input", command(p, false, false));
    h.physics.seed_crafting_tree(p, true);
    run(&mut h, "Input", command(p, false, true));
    assert!(h.crafting.items[0].job.is_some());
    for _ in 0..240 {
        run(
            &mut h,
            "Frame",
            json!({"view":{"position":add(p,[20.,0.,0.])},"dt":0.25}),
        );
    }
    assert!((h.crafting.items[0].job.as_ref().unwrap().elapsed - 0.6).abs() < 1e-9);
    for _ in 0..237 {
        run(&mut h, "Frame", json!({"view":{"position":p},"dt":0.25}));
    }
    assert_eq!(h.crafting.items.len(), 1);
    run(&mut h, "Frame", json!({"view":{"position":p},"dt":0.25}));
    assert_eq!(h.crafting.items.len(), 6);
    assert!(h.crafting.items.iter().all(|i| i.kind == Kind::Plank));
    let ids: std::collections::HashSet<_> = h.crafting.items.iter().map(|i| i.id).collect();
    assert_eq!(ids.len(), 6);
    h.stack.push((0, "CraftingSystem.Frame".into()));
    assert!(
        h.invoke("PhysicsSystem.GameCraftTree", json!({"position":p}))
            .unwrap()
            .is_null()
    );
    h.stack.pop();
    run(&mut h, "Input", command(p, false, false));
    let pickup = run(&mut h, "Input", command(p, true, false));
    assert_eq!(pickup["held"], true);
    let id = h.crafting.held.unwrap();
    assert_eq!(h.crafting.items.iter().filter(|i| i.id == id).count(), 1);
    assert_eq!(
        run(&mut h, "Grip", command(p, false, false)["view"].clone())["active"],
        true
    );
    run(&mut h, "Close", Value::Null);
    assert_eq!(h.audicles.count(), 0);
}
#[test]
fn crafting_f_opens_cave_jar_places_plan_and_close_cancels_job() {
    let mut h = host("f-input");
    h.invoke("CaveSystem.Configure", Value::Null).unwrap();
    run(&mut h, "Configure", Value::Null);
    let p = h.crafting.items[0].position;
    assert_eq!(run(&mut h, "Input", command(p, true, false))["held"], true);
    let id = h.crafting.held.unwrap();
    assert_eq!(
        h.crafting.items.iter().find(|i| i.id == id).unwrap().kind,
        Kind::Blueprint
    );
    // A held F never instantly places the newly acquired plan.
    assert_eq!(run(&mut h, "Input", command(p, true, false))["held"], true);
    run(&mut h, "Input", command(p, false, false));
    assert_eq!(run(&mut h, "Input", command(p, true, false))["held"], false);
    let i = h.crafting.items.iter().find(|i| i.id == id).unwrap();
    assert!(length(sub(i.position, p)) < 0.15);
    // A blocked click cannot become a fresh click when focus resumes.
    let mut blocked = command(p, false, true);
    blocked["blocked"] = json!(true);
    run(&mut h, "Input", blocked);
    h.physics.seed_crafting_tree(p, true);
    run(&mut h, "Input", command(p, false, true));
    assert!(h.crafting.items.iter().all(|i| i.job.is_none()));
    run(&mut h, "Input", command(p, false, false));
    run(&mut h, "Input", command(p, false, true));
    assert_eq!(
        h.crafting.items.iter().filter(|i| i.job.is_some()).count(),
        1
    );
    run(&mut h, "Input", command(p, false, true));
    assert_eq!(h.audicles.count(), 1);
    run(&mut h, "Close", Value::Null);
    assert_eq!(h.audicles.count(), 0);
}
