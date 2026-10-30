//! Temporary native capture driver. Production Shell/Composer; isolated Mock data.
use gpui::{AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use std::{path::PathBuf, sync::Arc, time::Duration};
use zeron_proto::{HarnessId, RunRequest, SandboxLevel};
use zeron_ui::*;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("warn").init();
    let output = PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&output)?;
    let temp = tempfile::tempdir()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();
    let registry = zeron_engine::HarnessRegistry::new();
    registry.register(Arc::new(zeron_harness::mock::MockHarness { script: vec![] }));
    let core = Arc::new(runtime.block_on(async {
        zeron_engine::EngineCore::assemble(
            &temp.path().join("engine"),
            Arc::new(registry),
            HarnessId::Mock,
            None,
        )
    })?);
    let project = temp.path().join("demo-project");
    std::fs::create_dir_all(&project)?;
    core.workspace.create_space(
        "demo-project",
        &core.device_id,
        project.to_str().unwrap(),
        Some("demo-project".into()),
        false,
    )?;
    core.workspace.create_chat(
        "background-preview",
        Some("demo-project"),
        Some(&core.device_id),
        None,
        None,
    )?;
    core.workspace
        .rename_chat("background-preview", "Run the development workspace")?;
    runtime.block_on(async {
        core.sessions
            .dispatch(
                "background-preview",
                HarnessId::Mock,
                RunRequest {
                    prompt: "Start the dev server, doc tests and API log watcher.".into(),
                    harness: Some(HarnessId::Mock),
                    model: None,
                    reasoning: None,
                    model_options: Default::default(),
                    cwd: project.to_string_lossy().into(),
                    sandbox: SandboxLevel::WorkspaceWrite,
                    auto_approve: true,
                    access: Default::default(),
                    plan: false,
                    resume: None,
                    require_native_resume: false,
                    attachments: vec![],
                    worktree: None,
                    mcp: None,
                    env: Default::default(),
                },
                None,
            )
            .await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if core.sessions.background_tasks("background-preview").len() == 3 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;
        anyhow::Ok(())
    })?;
    let data = temp.path().join("ui");
    std::fs::create_dir_all(&data)?;
    let ipc_port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let _ipc = runtime.block_on(zeron_engine::serve_ipc(ipc_port, &data, core.rpc_service()))?;
    let boot = EngineBootConfig {
        data_dir: data.clone(),
        ipc_port,
        edge_url: String::new(),
        edge_token: None,
        org_id: None,
        workos_client_id: None,
        default_harness: HarnessId::Mock,
    };
    let handle = runtime.block_on(state::EngineHandle::bootstrap(boot.clone()))?;
    let render_core = core.clone();
    gpui_platform::application().with_assets(icons::Assets).run(move |cx| {
        gpui_tokio::init(cx);
        gpui_base::init(cx);
        let mut settings = settings::UiSettings::default();
        settings.sidebar_width = 270.;
        settings.sidebar_organization = settings::SidebarOrganization::InOneList;
        settings.surface = zeron_theme::SurfacePreference::Opaque;
        settings::init(settings.clone(), data.clone(), cx);
        let fonts = typography::register_fonts(cx);
        typography::init(settings.ui_font_family.clone(), settings.ui_font_size,
            settings.terminal_font_family.clone(), settings.terminal_font_size,
            settings.code_font_family.clone(), settings.code_font_size, fonts, cx);
        theme_library::init(data.clone(), cx);
        appearance::init(appearance::AppearanceMode::Dark, settings.theme_selection,
            settings.accent, settings.surface, cx);
        history::init(settings.git_history_columns, settings.git_history_column_widths,
            settings.git_history_column_order, settings.git_history_author_display, cx);
        composer::init(cx, settings.composer_send_behavior);
        terminal::panel::init(cx);
        app_menus::init(cx);
        cx.set_reduce_motion(true);
        let state = cx.new(|_| {
            let mut s = state::AppState::new();
            s.fixture_attachment_engine(handle);
            s.connection = zeron_proto::view::ConnectionStatus::Ready;
            s.workspace_scope = Some(zeron_proto::WorkspaceScope::Local);
            s.local_device_id = Some(render_core.device_id.clone());
            s.devices = serde_json::from_value(serde_json::json!([{
                "id":render_core.device_id,"name":"Demo computer","platform":"linux","lastSeenAt":null
            }])).unwrap();
            s.chats = render_core.workspace.read_chats().unwrap();
            s.spaces = render_core.workspace.read_spaces().unwrap();
            s.apply_sessions(render_core.sessions.watch_sessions().borrow().clone());
            s.selected_chat = Some("background-preview".into());
            s.selected_space = Some("demo-project".into());
            s.auto_selected = true; s.chats_synced = true; s.spaces_synced = true;
            s
        });
        let window = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(gpui::point(px(40.),px(40.)),size(px(1200.),px(820.))))),
            window_background: theme::Theme::of(cx).window_background_appearance(),
            titlebar: Some(gpui::TitlebarOptions { title: Some("Zerun Background Preview".into()), ..Default::default() }),
            ..Default::default()
        }, |_,cx| cx.new(|cx| shell::Shell::new(state.clone(),boot,cx))).unwrap();
        state.update(cx,|s,cx| {
            s.receive_transcript_frame(zeron_doc::TranscriptFrame::Reset {
                reset: serde_json::from_value(serde_json::json!([
                    {"id":"prompt","role":"user","parts":[{"id":"text","kind":"text","text":"Start the dev server, doc tests and API log watcher."}],"createdAt":1793320000000_i64,"deviceId":render_core.device_id},
                    {"id":"reply","role":"assistant","parts":[{"id":"text","kind":"text","text":"All three background commands are running.\n\nThe dev server is listening on localhost:5173, the doc test watcher is ready, and the API log watcher is following new requests. You can inspect their output or stop one from the Background tray below."}],"createdAt":1793320001000_i64,"deviceId":render_core.device_id,"status":"complete"}
                ])).unwrap(),
            },cx).unwrap();
            cx.notify();
        });
        cx.activate(true);
        std::fs::write(output.join("ready.json"),serde_json::to_vec(&serde_json::json!({"pid":std::process::id(),"tasks":render_core.sessions.background_tasks("background-preview")})).unwrap()).unwrap();
        cx.spawn(async move |cx| {
            let mut mode = String::from("dark");
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if output.join("quit").exists() { break; }
                let sessions = render_core.sessions.watch_sessions().borrow().clone();
                state.update(cx,|s,cx| { if s.apply_sessions(sessions) { cx.notify(); } });
                if let Ok(next) = std::fs::read_to_string(output.join("mode"))
                    && next.trim() != mode
                {
                    mode = next.trim().to_owned();
                    cx.update(|cx| appearance::set_mode(if mode == "light" {appearance::AppearanceMode::Light} else {appearance::AppearanceMode::Dark},cx));
                }
            }
            let native: gpui::AnyWindowHandle = window.into();
            let _ = native.update(cx,|_,w,_|w.remove_window());
            cx.update(|cx|cx.quit());
        }).detach();
    });
    runtime.block_on(core.shutdown());
    Ok(())
}
