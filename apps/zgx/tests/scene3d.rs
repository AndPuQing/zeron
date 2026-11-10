use zgx::scene::{NodeId, NodeKind};
use zgx_script::{Plugin, ScriptHost, testing::TestApp};

fn frame_id(app: &mut TestApp) -> Option<u64> {
    let root = app.find("mesh-scene")?;
    let scene = app.ui.scene.borrow();
    let clip = *scene.children(root).first()?;
    let frame: NodeId = *scene.children(clip).first()?;
    match scene.kind(frame) {
        NodeKind::Image(image) if image.gpu_frame().is_some() => Some(image.id()),
        _ => None,
    }
}

#[test]
fn a_plugin_draws_and_picks_custom_triangles_without_extra_grants() {
    let dir = std::env::temp_dir().join(format!("zerun-plugin-mesh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("init.luau");
    std::fs::write(
        &entry,
        r##"
local picked = ui.state("picked", "none")
ui.contribute { target = "#slot", after = ui.column {
    ui.text { id = "picked", "{picked}" },
    ui.scene3d {
        id = "mesh-scene",
        w = 200,
        h = 200,
        orbit = true,
        camera = { position = { 0, 0, 4 }, fov = 40 },
        ui.mesh {
            geometry = {
                positions = { -0.5, -0.5, 0.5,  0.5, -0.5, 0.5,  0.5, 0.5, 0.5,  -0.5, 0.5, 0.5 },
                indices = { 1, 2, 3,  1, 3, 4 },
                face_colors = { 1, 0, 0,  0, 0, 1 },
            },
            material = { double_sided = true },
            on_click = function(hit) picked:set(string.format("%.2f", hit.z)) end,
        },
    },
} }
"##,
    )
    .unwrap();
    let mut app = TestApp::new(move |runtime| {
        let host = ScriptHost::inline(
            runtime,
            "mesh-fixture",
            r#"return ui.column { ui.text { id = "slot", "Plugin mesh" } }"#,
        );
        host.set_plugins(vec![Plugin {
            name: "mesh-viewer".into(),
            entry,
            grants: vec![],
        }]);
        host
    });
    app.frames(3);
    assert_eq!(app.error(), None);
    let frame = frame_id(&mut app).expect("the plugin's custom mesh rendered a GPU frame");
    app.frames(10);
    assert_eq!(
        frame_id(&mut app),
        Some(frame),
        "an idle scene keeps its frame"
    );
    app.click("mesh-scene");
    assert_eq!(
        app.text("picked"),
        "0.50",
        "the click reached the plugin's mesh"
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
