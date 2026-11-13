use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

fn material() -> Material {
    Material {
        base: [0.5, 0.3, 0.1],
        roughness: 0.4,
        emission: [0.; 3],
        metallic: 0.,
        unlit: true,
        double_sided: true,
    }
}
fn vertex(x: f64, y: f64) -> Vertex {
    Vertex {
        position: [x, y, 0.],
        normal: [0., 0., 1.],
        uv: [x + 0.5, y + 0.5],
    }
}
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Triangle> {
    let a = vertex(x0, y0);
    let b = vertex(x1, y0);
    let c = vertex(x1, y1);
    let d = vertex(x0, y1);
    vec![
        Triangle {
            vertices: [a, b, c],
            material: 0,
        },
        Triangle {
            vertices: [a, c, d],
            material: 0,
        },
    ]
}
fn kit(resize: Resize) -> Kit {
    let mut packet = Vec::new();
    let model = Model::pack(
        RawMesh {
            triangles: rectangle(-0.5, -0.125, 0.5, 0.125),
            materials: vec![material()],
        },
        &mut packet,
        resize,
        [-0.4, -0.1, 0.3, 0.1],
        0.,
        true,
    )
    .unwrap();
    Kit {
        id: "test.plane".into(),
        name: "Plane".into(),
        shader_source: None,
        animated: false,
        background: false,
        models: vec![model],
        panels: [Some(0); 4],
        packet,
        panel_shader: false,
    }
}
fn view() -> Viewport {
    Viewport {
        width: 1280.,
        height: 720.,
        canvas_scale: 1.,
    }
}
fn motion() -> Snapshot {
    Snapshot {
        extensions: [1.; 4],
        open: [true; 4],
        opening_order: [3, 4, 1, 2],
        ..Snapshot::default()
    }
}

#[test]
fn user_style_zero_depth_panel_retains_authored_triangles_and_material_flags() {
    let kit = kit(Resize::Stretch);
    let model = &kit.models[0];
    assert_eq!(model.min[2], 0.);
    assert_eq!(model.max[2], 0.);
    assert_eq!(kit.packet[model.end - BASE][3], 0.);
    assert_eq!(kit.packet[model.end - BASE + 1][3], 1.);
    assert_eq!(kit.packet[model.end - BASE + 2][3], 1.);
    assert!(kit.packet[model.root - BASE][2] < 0.);
    assert!(kit.packet[model.root - BASE + 1][2] > 0.);
    let mut rows = vec![[0.; 4]; BASE + ROWS];
    kit.write_geometry(&mut rows).unwrap();
    kit.apply(&mut rows, view(), &motion(), Options::default(), 100.)
        .unwrap();
    assert_eq!(rows[HEADER], [1., 0., 0., 0.]);
    assert_eq!(rows[HEADER + 1][0], BASE as f32);
    assert_eq!(rows[32][0], 0.);
    assert_eq!(rows[33][3], 1.);
    assert!(rows[5][3] <= -0.02);
    assert!(rows.iter().flatten().all(|v| v.is_finite()));
}

#[test]
fn user_style_fit_surface_and_hit_share_the_same_offcenter_content_transform() {
    let stretch = kit(Resize::Stretch);
    let contain = kit(Resize::Contain);
    let v = view();
    let m = motion();
    let o = Options::default();
    let slot = panel_layout::layout(v, &m, o).unwrap()[2];
    let fitted = stretch.surface(2, v, &m, o).unwrap().unwrap();
    assert!((fitted.visual_rect.width - slot.width).abs() < 1e-8);
    assert!((fitted.visual_rect.height - slot.height).abs() < 1e-8);
    assert!((fitted.scale[0] - fitted.scale[1]).abs() > 1e-3);
    assert!(
        (fitted.origin[0]
            - ((slot.center()[0] - v.width * 0.5) * 1.3 / v.height - 0.05 * fitted.scale[0]))
            .abs()
            < 1e-10
    );
    assert!((fitted.size[0] - 0.7 * fitted.scale[0]).abs() < 1e-10);
    let fitted = contain.surface(2, v, &m, o).unwrap().unwrap();
    assert_eq!(fitted.scale[0], fitted.scale[1]);
    assert_eq!(fitted.scale[1], fitted.scale[2]);
    assert!(fitted.visual_rect.height < slot.height);
    assert!(
        contain
            .hit_panel_point(2, v, &m, o, fitted.visual_rect.center())
            .unwrap()
    );
    assert!(
        !contain
            .hit_panel_point(2, v, &m, o, [slot.center()[0], slot.y + 1.])
            .unwrap()
    );
}

#[test]
fn user_style_snap_uses_target_open_state_instead_of_intermediate_slide() {
    let kit = kit(Resize::Snap);
    let mut m = motion();
    m.extensions = [0.24, 0.44, 0.54, 0.66];
    let intermediate = kit
        .surface(1, view(), &m, Options::default())
        .unwrap()
        .unwrap();
    let finished = kit
        .surface(1, view(), &motion(), Options::default())
        .unwrap()
        .unwrap();
    assert_eq!(intermediate.visual_rect, finished.visual_rect);
    assert_eq!(intermediate.size, finished.size);
    m.open[1] = false;
    let closed = kit
        .surface(1, view(), &m, Options::default())
        .unwrap()
        .unwrap();
    assert!(closed.visual_rect.y >= view().height);
}

#[test]
fn user_style_canvas_tracks_snap_models_during_motion_and_after_swaps() {
    let kit = kit(Resize::Snap);
    let mut intermediate = motion();
    intermediate.extensions = [0.02, 0.24, 0.08, 0.56];
    let values = |rect: super::super::piano_roll::Rect| [rect.left, rect.top, rect.width, rect.height];
    for swapped in [false, true] {
        let options = Options { swap_sides: swapped, swap_horizontal: swapped, ..Options::default() };
        let resolved = panel_layout::visual_bounds_with_style(view(), &intermediate, options, Some(&kit)).unwrap();
        let target = panel_layout::visual_bounds_with_style(view(), &motion(), options, Some(&kit)).unwrap();
        assert_eq!(resolved, target);
        let canvas = super::super::piano_roll_scene::canvas_from_bounds(view(), resolved, options);
        let target_canvas = super::super::piano_roll_scene::canvas_from_bounds(view(), target, options);
        assert_eq!(values(canvas), values(target_canvas));
        let raw = panel_layout::layout(view(), &intermediate, options).unwrap();
        let raw_canvas = super::super::piano_roll_scene::canvas_from_bounds(view(), raw, options);
        assert_ne!(values(canvas), values(raw_canvas));
    }
}

#[test]
fn user_style_contain_canvas_keeps_eight_pixel_margins_from_visible_models() {
    let kit = kit(Resize::Contain);
    let options = Options::default();
    let raw = panel_layout::layout(view(), &motion(), options).unwrap();
    let resolved = panel_layout::visual_bounds_with_style(view(), &motion(), options, Some(&kit)).unwrap();
    let canvas = super::super::piano_roll_scene::canvas_from_bounds(view(), resolved, options);
    let raw_canvas = super::super::piano_roll_scene::canvas_from_bounds(view(), raw, options);
    assert!(canvas.top < raw_canvas.top);
    assert!(canvas.height > raw_canvas.height);
    assert!((canvas.left - (resolved[2].x + resolved[2].width) - 8.).abs() < 1e-8);
    assert!((canvas.top - (resolved[0].y + resolved[0].height) - 8.).abs() < 1e-8);
    assert!((resolved[3].x - canvas.left - canvas.width - 8.).abs() < 1e-8);
    assert!((resolved[1].y - canvas.top - canvas.height - 8.).abs() < 1e-8);
    assert_eq!(panel_layout::visual_bounds_with_style(view(), &motion(), options, None).unwrap(), raw);
}

#[test]
fn user_style_snap_face_can_close_before_raw_opening_extension_reaches_gesture_threshold() {
    let kit = kit(Resize::Snap);
    let mut intermediate = motion();
    intermediate.extensions = [0.02; 4];
    let options = Options::default();
    let point = kit.surface(0,view(),&intermediate,options).unwrap().unwrap().visual_rect.center();
    assert_eq!(panel_layout::gesture_with_style(view(), &intermediate, options, point, [0.,-80.], 1, Some(&kit)),
        Some((super::super::panel_motion::Edge::Top, panel_layout::GestureAction::Close)));
    assert_eq!(panel_layout::gesture_with_style(view(), &intermediate, options, point, [0.,-80.], 1, None), None);
}

#[test]
fn user_style_mesh_coverage_preserves_concave_holes() {
    let mut kit = kit(Resize::Stretch);
    let mut triangles = rectangle(-0.5, -0.5, -0.2, 0.5);
    triangles.extend(rectangle(0.2, -0.5, 0.5, 0.5));
    triangles.extend(rectangle(-0.2, -0.5, 0.2, -0.2));
    triangles.extend(rectangle(-0.2, 0.2, 0.2, 0.5));
    kit.packet.clear();
    kit.models[0] = Model::pack(
        RawMesh {
            triangles,
            materials: vec![material()],
        },
        &mut kit.packet,
        Resize::Stretch,
        [-0.4, -0.4, 0.4, 0.4],
        0.,
        false,
    )
    .unwrap();
    let surface = kit
        .surface(0, view(), &motion(), Options::default())
        .unwrap()
        .unwrap();
    assert!(
        !kit.hit_panel_point(
            0,
            view(),
            &motion(),
            Options::default(),
            surface.visual_rect.center()
        )
        .unwrap()
    );
    assert!(
        kit.hit_panel_point(
            0,
            view(),
            &motion(),
            Options::default(),
            [
                surface.visual_rect.x + surface.visual_rect.width * 0.1,
                surface.visual_rect.center()[1]
            ]
        )
        .unwrap()
    );
}

#[test]
fn user_style_bvh_escape_and_triangle_addresses_are_bounded() {
    let mut triangles = Vec::new();
    for i in 0..100 {
        triangles.extend(rectangle(i as f64 * 0.01, 0., i as f64 * 0.01 + 0.009, 0.1));
    }
    let mut packet = Vec::new();
    let model = Model::pack(
        RawMesh {
            triangles,
            materials: vec![material()],
        },
        &mut packet,
        Resize::Stretch,
        [0., 0., 0.99, 0.1],
        0.,
        false,
    )
    .unwrap();
    let mut leaves = 0;
    let mut covered = vec![false; 200];
    for at in (model.root..model.end).step_by(3) {
        let escape = packet[at - BASE][3] as usize;
        let first = packet[at - BASE + 1][3] as usize;
        let count = packet[at - BASE + 2][0] as usize;
        assert!(escape > at && escape <= model.end);
        assert_eq!((escape - model.root) % 3, 0);
        assert!(count <= 4);
        if count > 0 {
            leaves += 1;
            assert!(first >= model.end);
            assert_eq!((first - model.end) % 8, 0);
            for index in (first - model.end) / 8..(first - model.end) / 8 + count {
                assert!(!covered[index]);
                covered[index] = true;
            }
        }
    }
    assert!(leaves > 1 && covered.into_iter().all(|v| v));
}

fn glb_document() -> (Value, Vec<u8>) {
    let positions: [[f32; 3]; 3] = [[-0.5, -0.125, 0.], [0.5, -0.125, 0.], [0., 0.125, 0.]];
    let binary: Vec<u8> = positions
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
    (
        json!({"asset":{"version":"2.0"},"buffers":[{"byteLength":binary.len()}],
        "bufferViews":[{"buffer":0,"byteLength":binary.len()}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0}),
        binary,
    )
}
fn encode_glb(document: Value, mut binary: Vec<u8>) -> Vec<u8> {
    let mut json = serde_json::to_vec(&document).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let mut out = Vec::new();
    for word in [
        0x46546c67u32,
        2,
        (12 + 8 + json.len() + 8 + binary.len()) as u32,
        json.len() as u32,
        0x4e4f534a,
    ] {
        out.extend(word.to_le_bytes());
    }
    out.extend(json);
    out.extend((binary.len() as u32).to_le_bytes());
    out.extend(0x004e4942u32.to_le_bytes());
    out.extend(binary);
    out
}

#[test]
fn user_style_glb_accepts_nonindexed_thin_triangles_and_synthesizes_face_normals() {
    let (doc, bin) = glb_document();
    let raw = glb::decode(&encode_glb(doc, bin), false).unwrap();
    assert_eq!(raw.triangles.len(), 1);
    assert_eq!(raw.triangles[0].vertices[0].normal, [0., 0., 1.]);
    assert_eq!(raw.triangles[0].vertices[0].uv, [0.; 2]);
}

#[test]
fn user_style_glb_rejects_data_loss_transforms_uris_and_invalid_ranges() {
    let (doc, bin) = glb_document();
    let mut cases = Vec::new();
    let mut changed = doc.clone();
    changed["nodes"][0]["translation"] = json!([1., 0., 0.]);
    cases.push(changed);
    let mut changed = doc.clone();
    changed["buffers"][0]["uri"] = json!("outside.bin");
    cases.push(changed);
    let mut changed = doc.clone();
    changed["animations"] = json!([{}]);
    cases.push(changed);
    let mut changed = doc.clone();
    changed["accessors"][0]["count"] = json!(18446744073709551615u64);
    cases.push(changed);
    let mut changed = doc.clone();
    changed["accessors"][0]["byteOffset"] = json!(4);
    cases.push(changed);
    let mut changed = doc.clone();
    changed["nodes"][0]["children"] = json!([0]);
    cases.push(changed);
    let mut changed = doc.clone();
    changed["textures"] = json!([{}]);
    cases.push(changed);
    let mut changed = doc;
    changed["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = json!(0);
    cases.push(changed);
    for doc in cases {
        assert!(glb::decode(&encode_glb(doc, bin.clone()), false).is_err());
    }
    let (doc, mut bin) = glb_document();
    bin[0..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(glb::decode(&encode_glb(doc, bin), false).is_err());
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "wormwood-user-style-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_style_package_load_rejects_traversal_symlink_escape_and_unknown_roles() {
    let temporary = Temporary::new();
    let (doc, bin) = glb_document();
    std::fs::write(temporary.0.join("panel.glb"), encode_glb(doc, bin)).unwrap();
    let mut manifest = json!({"format":"mirror.workstation.style","version":1,"id":"test.valid","name":"Valid",
        "models":{"plane":{"file":"panel.glb","axes":"panel","content_rect_m":[-0.3,-0.1,0.3,0.1]}},"panels":{"top":{"model":"plane"}}});
    let path = temporary.0.join("style.json");
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let loaded = Kit::load(&path).unwrap();
    assert_eq!(loaded.panels, [Some(0), None, None, None]);
    assert!(package_file(&temporary.0, "../panel.glb").is_err());
    assert!(package_file(&temporary.0, "/tmp/panel.glb").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(std::env::temp_dir(), temporary.0.join("escape")).unwrap();
        assert!(package_file(&temporary.0, "escape").is_err());
    }
    manifest["panels"]["typo"] = json!({"model":"plane"});
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(Kit::load(&path).is_err());
}

#[test]
fn user_style_authored_examples_import_actual_meshes_and_flat_ui_remains_a_plane() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("Art/AudioWorkstation/StyleExamples");
    for name in ["carved-metal", "wood", "flat-ui"] {
        let kit = Kit::load(&root.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(kit.models.len(), 1);
        assert!(kit.panels.iter().all(|v| *v == Some(0)));
        assert!(kit.shader_source.is_some());
        let model = &kit.models[0];
        assert!(model.triangles.len() <= 2000);
        if name == "flat-ui" {
            assert_eq!(model.triangles.len(), 2);
            assert_eq!(model.resize, Resize::Snap);
            assert_eq!(model.min[2], model.max[2]);
            assert_eq!(kit.packet[model.end - BASE + 1][3], 1.);
        } else {
            assert!(model.max[2] > model.min[2]);
            assert!(model.triangles.len() > 2);
        }
        for (width, height) in [(1280., 720.), (390., 844.)] {
            let view = Viewport {
                width,
                height,
                canvas_scale: 1.,
            };
            for index in 0..4 {
                let surface = kit
                    .surface(index, view, &motion(), Options::default())
                    .unwrap()
                    .unwrap();
                assert!(surface.size.into_iter().all(|v| v > 0. && v.is_finite()));
                assert_eq!(surface.origin[2], 0.);
            }
        }
    }
}
