// Cardinal browser face and acknowledged-drop feedback regressions.

use super::super::{piano_roll, plugin_surface};
use super::*;
use serde_json::{Value, json};
fn entry() -> Value {
    json!({"name":"Effect","format":"VST3","index":1,"instrument":false})
}
fn layout() -> Layout {
    Layout {
        screen: [0., 0., 1000., 700.],
        clip: [10., 50., 390., 200.],
        content: [200., 50., 200., 200.],
        categories: [
            [10., 50., 180., 43.2],
            [10., 93.2, 180., 43.2],
            [10., 136.4, 180., 43.2],
            [10., 179.6, 180., 43.2],
        ],
        back:[0.;4], search:[0.;4], imports:[[0.;4];2], query:String::new(),search_focused:false,
        rows: vec![Row {
            text: "Effect".into(),
            rect: [200., 50., 200., 43.2],
            item: Some(Item::Plugin(entry())),
            header: false,
        }],
        scale: 1.,
        minimum: 0.,
    }
}
fn rect(v: &Value) -> [f64; 4] {
    serde_json::from_value(v.clone()).unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}")
}
fn expect_rect(actual: &Value, expected: [f64; 4]) {
    for (a, b) in rect(actual).into_iter().zip(expected) {
        close(a, b);
    }
}
#[test]
fn hover_and_selection_fill_row_faces_and_keep_item_text_stable() {
    let l = layout();
    let item = Item::Plugin(entry());
    let groups = l.presentation_with_hover(2, Some(&item), Some([300., 70.]));
    let faces = groups[1]["browser_faces"].as_array().unwrap();
    assert_eq!(faces.len(), 2);
    expect_rect(&faces[0]["rect"], [207.2, 53.6, 185.6, 36.]);
    assert_eq!(faces[0]["alpha"], 0.28);
    assert_eq!(faces[0]["radius"], 9.6);
    assert_eq!(faces[1]["alpha"], 0.76);
    assert_eq!(faces[0]["rect"], faces[1]["rect"]);
    assert_eq!(groups[1]["labels"][0]["color"], json!([59, 52, 46]));
    assert!(groups[1]["labels"][0]["selected"].is_null());
    assert!(plugin_surface::valid(&json!({"groups":groups})));
}
#[test]
fn disclosure_hover_layers_under_selection_and_category_has_its_source_insets() {
    let l = layout();
    let item = Item::Plugin(entry());
    let groups = l.presentation_with_hover(2, Some(&item), Some([215., 70.]));
    let faces = groups[1]["browser_faces"].as_array().unwrap();
    assert_eq!(faces.len(), 3);
    expect_rect(&faces[1]["rect"], [207.2, 58.4, 26.4, 26.4]);
    assert_eq!(faces[1]["radius"], 6.);
    assert_eq!(faces[2]["alpha"], 0.76);
    let groups = l.presentation_with_hover(0, None, Some([20., 65.]));
    let faces = groups[0]["browser_faces"].as_array().unwrap();
    assert_eq!(faces.len(), 2);
    expect_rect(&faces[0]["rect"], [12., 53., 176., 37.2]);
    assert_eq!(faces[0]["alpha"], 0.28);
    assert_eq!(faces[1]["alpha"], 0.76);
}
#[test]
fn fractional_scroll_clips_actual_row_before_insetting_and_hides_partial_text() {
    let mut l = layout();
    l.rows[0].rect[1] = 30.;
    let item = Item::Plugin(entry());
    let groups = l.presentation_with_hover(2, Some(&item), Some([300., 55.]));
    let faces = groups[1]["browser_faces"].as_array().unwrap();
    assert_eq!(faces.len(), 2);
    expect_rect(&faces[0]["rect"], [207.2, 53.6, 185.6, 16.]);
    assert!(groups[1]["labels"].as_array().unwrap().is_empty());
    let groups = l.presentation_with_hover(2, None, Some([-1., 55.]));
    assert!(groups[1]["browser_faces"].as_array().unwrap().is_empty());
}
#[test]
fn proposed_drop_does_not_sparkle_until_host_accepts_it() {
    let l = layout();
    let mut ui = Ui::default();
    let audio = json!({"id":"a","kind":"audio"});
    ui.update(
        &piano_roll::Input {
            pointer: Some([300., 70.]),
            left: true,
            ..Default::default()
        },
        &l,
        1,
        [0.; 4],
        None,
        false,
    );
    let command = ui.update(
        &piano_roll::Input {
            pointer: Some([500., 300.]),
            ..Default::default()
        },
        &l,
        1,
        [0.; 4],
        Some(&audio),
        false,
    );
    assert_eq!(command, Some(json!({"op":"add","track":"a","index":1})));
    assert!(ui.drop_feedback(l.screen).is_null());
    ui.acknowledge_drop(Some([500., 300.]));
    let feedback = ui.drop_feedback(l.screen);
    let labels = feedback["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 8);
    assert_eq!(
        labels
            .iter()
            .map(|l| l["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["*", "+", "*", "+", "*", "+", "*", "+"]
    );
    assert_eq!(labels[0]["size"], 22.);
    expect_rect(&labels[0]["rect"], [485., 278., 44., 44.]);
    assert!(plugin_surface::valid(&json!({"groups":[feedback]})));
    ui.advance_feedback(0.275);
    let feedback = ui.drop_feedback(l.screen);
    assert_eq!(feedback["labels"][0]["size"], 16.);
    expect_rect(&feedback["labels"][0]["rect"], [512., 284., 32., 32.]);
    ui.advance_feedback(0.275);
    assert!(ui.drop_feedback(l.screen).is_null());
}
#[test]
fn sparkle_restart_cancel_and_invalid_points_are_bounded() {
    let mut ui = Ui::default();
    let screen = [0., 0., 1000., 700.];
    ui.acknowledge_drop(Some([500., 300.]));
    ui.advance_feedback(0.4);
    ui.acknowledge_drop(Some([200., 100.]));
    assert_eq!(ui.drop_feedback(screen)["labels"][0]["size"], 22.);
    ui.advance_feedback(-1.);
    ui.advance_feedback(f64::NAN);
    assert_eq!(ui.drop_feedback(screen)["labels"][0]["size"], 22.);
    ui.cancel();
    assert!(ui.drop_feedback(screen).is_null());
    ui.acknowledge_drop(Some([f64::NAN, 0.]));
    assert!(ui.drop_feedback(screen).is_null());
}
#[test]
fn face_payload_rejects_out_of_range_alpha_radius_and_color() {
    let l = layout();
    let good = json!({"groups":l.presentation_with_hover(2,None,Some([300.,70.]))});
    assert!(plugin_surface::valid(&good));
    for (key, value) in [
        ("alpha", json!(1.1)),
        ("radius", json!(-1.)),
        ("color", json!([0, 0, 256])),
        ("rect", json!([0, 0, -1, 5])),
    ] {
        let mut bad = good.clone();
        bad["groups"][1]["browser_faces"][0][key] = value;
        assert!(!plugin_surface::valid(&bad), "{key}");
    }
}

#[test]
fn category_wheel_does_not_scroll_the_plugin_contents_column() {
    let mut l = layout();
    l.minimum = -500.;
    let mut ui = Ui::default();
    ui.scroll = -100.;
    let mut input = piano_roll::Input {
        pointer: Some([20., 65.]),
        wheel: [0., -1.],
        ..Default::default()
    };
    ui.update(&input, &l, 1, [0.; 4], None, false);
    assert_eq!(ui.scroll, -100.);
    input.pointer = Some([300., 70.]);
    ui.update(&input, &l, 1, [0.; 4], None, false);
    close(ui.scroll, -143.2);
}

#[test]
fn audio_drag_targets_audio_lanes_and_cancel_does_not_import() {
    let mut l=layout();
    let entry=json!({"name":"Tone.wav","format":"WAV","path":"/tmp/Tone.wav"});
    l.rows[0].item=Some(Item::Audio(entry.clone()));
    let mut ui=Ui::default();
    let press=piano_roll::Input{pointer:Some([300.,70.]),left:true,..Default::default()};
    let release=piano_roll::Input{pointer:Some([500.,300.]),..Default::default()};
    assert!(ui.update(&press,&l,1,[0.;4],None,false).is_none());assert!(ui.dragging_audio());
    let command=ui.update(&release,&l,1,[0.;4],Some(&json!({"id":"a","kind":"audio"})),false).unwrap();
    assert_eq!(command,json!({"op":"import_audio","track":"a","path":"/tmp/Tone.wav"}));
    ui.update(&press,&l,1,[0.;4],None,false);
    assert!(ui.update(&release,&l,1,[0.;4],Some(&json!({"id":"m","kind":"midi"})),false).is_none());
    ui.update(&press,&l,1,[0.;4],None,false);
    let cancel=piano_roll::Input{escape:true,..press.clone()};
    ui.update(&cancel,&l,1,[0.;4],None,false);
    assert!(ui.update(&release,&l,1,[0.;4],Some(&json!({"id":"a","kind":"audio"})),false).is_none());
    ui.update(&press,&l,1,[0.;4],None,false);
    assert!(ui.update(&release,&l,2,[0.;4],Some(&json!({"id":"a","kind":"audio"})),false).is_none());
}

#[test]fn audio_click_previews_but_drag_only_imports(){
 let mut l=layout();let audio=json!({"name":"tone.wav","format":"WAV","path":"/missing-test-tone.wav"});l.rows[0].item=Some(Item::Audio(audio));
 let mut ui=Ui::default();let canvas=[0.,0.,1000.,700.];let p=|x,y,left|piano_roll::Input{pointer:Some([x,y]),left,..Default::default()};
 ui.update(&p(280.,70.,true),&l,1,canvas,None,false);
 assert_eq!(ui.update(&p(280.,70.,false),&l,1,canvas,None,false).unwrap()["op"],"preview_audio");
 ui.update(&p(280.,70.,true),&l,1,canvas,None,false);ui.update(&p(600.,300.,true),&l,1,canvas,None,false);
 assert_eq!(ui.update(&p(600.,300.,false),&l,1,canvas,Some(&json!({"id":"","kind":"audio"})),false).unwrap()["op"],"import_audio");
}

#[test]
fn explorer_root_back_search_and_import_controls_share_full_width() {
    let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};
    let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(2.);
    let motion=motion.snapshot();let options=Options::default();
    let entries=vec![json!({"name":"My Song.wav","format":"WAV","path":"/tmp/song.wav"})];
    let root=layout_filtered(view,&motion,options,&entries,4,0.,"").unwrap();
    assert!(root.rows.is_empty());assert_eq!(root.categories[1][2],root.content[2]);
    let filtered=layout_filtered(view,&motion,options,&entries,4,0.,"sOnG").unwrap();
    assert_eq!(filtered.rows.len(),1);assert!(matches!(filtered.rows[0].item,Some(Item::Audio(_))));
    let mut ui=Ui::default();let mut input=Input::default();
    input.pointer=Some([root.categories[3][0]+5.,root.categories[3][1]+5.]);input.left=true;
    ui.category=4;ui.update(&input,&root,1,[0.;4],None,false);assert_eq!(ui.category,3);
    input.left=false;ui.update(&input,&root,1,[0.;4],None,false);
    let audio=layout_filtered(view,&motion,options,&entries,3,0.,"").unwrap();
    input.left=true;input.pointer=Some([audio.imports[0][0]+5.,audio.imports[0][1]+5.]);
    assert!(ui.update(&input,&audio,1,[0.;4],None,false).is_none());
    input.left=false;assert_eq!(ui.update(&input,&audio,1,[0.;4],None,false),Some(json!({"op":"choose_audio"})));
    let p=[audio.search[0]+5.,audio.search[1]+5.];
    assert!(ui.search_input(&json!({"pointer":p,"left":true,"browser_keys":[[83,0],[79,0],[78,0],[71,0]]}),&audio,false));
    assert_eq!(ui.query,"song");
    ui.search_input(&json!({"pointer":p,"left":false,"browser_keys":[[259,0],[256,0]]}),&audio,false);
    assert!(ui.query.is_empty());
}
