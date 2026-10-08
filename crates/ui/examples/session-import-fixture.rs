//! Native UI review for session import, with synthetic data and no engine.
use gpui::{AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeron_ui::*;

fn main() -> anyhow::Result<()> {
    let mut scene = "list".to_owned();
    let mut light = false;
    let mut opaque = false;
    let mut narrow = false;
    let mut capture_directory = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => light = true,
            "--opaque" => opaque = true,
            "--narrow" => narrow = true,
            "--capture" => {
                capture_directory =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--capture requires an output directory")
                    })?))
            }
            "entry" | "fresh-entry" | "list" | "selected" | "results" | "partial" | "empty"
            | "source-error" | "scan-error" | "scanning" => scene = arg,
            _ => anyhow::bail!(
                "Unknown option: {arg}. Scenes: entry, fresh-entry, list, selected, results, partial, empty, source-error, scan-error, scanning; flags: --light, --opaque, --narrow, --capture DIRECTORY"
            ),
        }
    }
    let capture_path = capture_directory
        .map(|directory| -> anyhow::Result<_> {
            std::fs::create_dir_all(&directory)?;
            let path = directory.join(format!(
                "{scene}-{}-{}-{}.png",
                if light { "light" } else { "dark" },
                if opaque { "opaque" } else { "frosted" },
                if narrow { "narrow" } else { "wide" }
            ));
            anyhow::ensure!(!path.exists(), "Capture already exists: {}", path.display());
            Ok(path)
        })
        .transpose()?;
    let capture_failed = Arc::new(AtomicBool::new(false));
    let capture_failed_in_app = capture_failed.clone();
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let temp = tempfile::tempdir()?;
    let data = temp.path().to_path_buf();
    gpui_platform::application().with_assets(icons::Assets).run(move |cx| {
        gpui_tokio::init(cx);
        gpui_base::init(cx);
        let mut settings = settings::UiSettings::default();
        settings.surface = if opaque { zeron_theme::SurfacePreference::Opaque } else { zeron_theme::SurfacePreference::Frosted };
        settings.save(&data).unwrap();
        settings::init(settings.clone(), data.clone(), cx);
        let fonts = typography::register_fonts(cx);
        typography::init(settings.ui_font_family.clone(), settings.ui_font_size, settings.terminal_font_family.clone(), settings.terminal_font_size, settings.code_font_family.clone(), settings.code_font_size, fonts, cx);
        theme_library::init(data.clone(), cx);
        appearance::init(if light { appearance::AppearanceMode::Light } else { appearance::AppearanceMode::Dark }, settings.theme_selection, settings.accent, settings.surface, cx);
        history::init(settings.git_history_columns, settings.git_history_column_widths, settings.git_history_column_order, settings.git_history_author_display, cx);
        composer::init(cx, settings.composer_send_behavior);
        terminal::panel::init(cx);
        app_menus::init(cx);
        // Snapshot exports use settled controls even when the host skips
        // animation frames for a background fixture window.
        if capture_path.is_some() { cx.set_reduce_motion(true); }
        let state = cx.new(|_| {
            let mut state = state::AppState::new();
            state.connection = zeron_proto::view::ConnectionStatus::Ready;
            state.workspace_scope = Some(zeron_proto::WorkspaceScope::Local);
            state.local_device_id = Some("local".into());
            state.auto_selected = true;
            state.chats_synced = true;
            state.spaces_synced = true;
            state.selected_space = Some("project".into());
            state.devices = serde_json::from_value(serde_json::json!([
                {"id":"local","name":"This device","platform":std::env::consts::OS,"lastSeenAt":null},
                {"id":"remote","name":"Build server","platform":"linux","lastSeenAt":chrono::Utc::now()}
            ])).unwrap();
            state.spaces = serde_json::from_value(serde_json::json!([
                {"id":"project","deviceId":"local","path":"/projects/fieldnotes","createdAt":"2026-10-01T00:00:00Z"},
                {"id":"remote-project","deviceId":"remote","path":"/srv/projects/fieldnotes","createdAt":"2026-10-01T00:00:00Z"}
            ])).unwrap();
            state.chats = serde_json::from_value(serde_json::json!([
                {"id":"existing-session","deviceId":"local","spaceId":"project","title":"Add deployment status","archived":false,"createdAt":"2026-10-01T00:00:00Z",
                "config":{"harness":"claude-code","model":"claude-sonnet-4-6","reasoning":null,"sandbox":"workspace-write"}},
                {"id":"remote-existing-session","deviceId":"remote","spaceId":"remote-project","title":"Add deployment status","archived":true,"createdAt":"2026-10-01T00:00:00Z"},
                {"id":"current-session","deviceId":"local","spaceId":"project","title":"Review the workspace","archived":false,"createdAt":"2026-10-01T00:00:00Z"}
            ])).unwrap();
            if scene == "fresh-entry" { state.chats.clear(); }
            state
        });
        let boot = EngineBootConfig { data_dir: data, ipc_port: 0, edge_url: String::new(), edge_token: None, org_id: None, workos_client_id: None, default_harness: HarnessId::Codex };
        let window = cx.open_window(WindowOptions {
            window_background: theme::Theme::of(cx).window_background_appearance(),
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(gpui::point(px(12.0), px(30.0)), size(px(if narrow { 760.0 } else { 1200.0 }), px(850.0))))),
            titlebar: Some(gpui::TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(gpui::point(px(14.0), px(14.0))) }),
            app_owns_titlebar_drag: true,
            ..Default::default()
        }, |window, cx| cx.new(|cx| {
            let mut shell = shell::Shell::new(state.clone(), boot, cx);
            shell.fixture_session_import(&scene, window, cx);
            shell
        })).unwrap();
        state.update(cx, |_, cx| cx.notify());
        cx.activate(true);
        if let Some(path) = capture_path {
            cx.spawn(async move |cx| {
                let result: anyhow::Result<()> = async {
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                        let ready = window.update(cx, |shell, window, cx| shell.fixture_session_import_capture_ready(&scene, window, cx))?;
                        if ready { break; }
                        anyhow::ensure!(Instant::now() < deadline, "Scene {scene} did not become ready");
                    }
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                    // Drawing updates the root view; do not borrow Shell through
                    // the typed window handle while asking GPUI to draw it.
                    let capture_window: gpui::AnyWindowHandle = window.into();
                    capture_window.update(cx, |_, window, cx| {
                        window.draw(cx).clear();
                        window.render_to_image()?.save(&path)?;
                        Ok::<(), anyhow::Error>(())
                    })??;
                    eprintln!("Saved {}", path.display());
                    Ok(())
                }.await;
                if let Err(error) = result {
                    capture_failed_in_app.store(true, Ordering::Relaxed);
                    eprintln!("Session import capture failed: {error:#}");
                }
                cx.update(|cx| cx.quit());
            }).detach();
        }
    });
    anyhow::ensure!(
        !capture_failed.load(Ordering::Relaxed),
        "Session import capture failed"
    );
    Ok(())
}
