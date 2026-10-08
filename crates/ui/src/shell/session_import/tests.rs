use super::*;
use crate::session_import::ImportDevice;
use std::rc::Rc;

struct SidebarHost(Entity<Shell>);

impl Render for SidebarHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0.update(cx, |shell, cx| {
            div()
                .w(px(280.0))
                .h(px(800.0))
                .child(shell.render_chat_sidebar(&Theme::default(), cx))
        })
    }
}

#[gpui::test]
fn first_import_remains_discoverable_without_any_zeron_sessions(cx: &mut gpui::TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::app_menus::init(cx);
        crate::history::init(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            cx,
        );
    });
    let (host, cx) = cx.add_window_view(|_, cx| {
        SidebarHost(cx.new(|cx| {
            let state = cx.new(|_| {
                let mut state = AppState::new();
                state.workspace_scope = Some(WorkspaceScope::Local);
                state.local_device_id = Some("local".into());
                state
            });
            let mut shell = Shell::new(
                state,
                EngineBootConfig {
                    data_dir: data.path().into(),
                    ipc_port: 0,
                    edge_url: String::new(),
                    edge_token: None,
                    org_id: None,
                    workos_client_id: None,
                    default_harness: zeron_proto::HarnessId::Mock,
                },
                cx,
            );
            shell.settings.sidebar_organization = SidebarOrganization::InOneList;
            shell.configure_session_import(
                SessionImportController {
                    devices: vec![ImportDevice {
                        id: "local".into(),
                        name: "This device".into(),
                        current_project: None,
                    }],
                    handle: Rc::new(|_, _, _| {}),
                },
                cx,
            );
            shell
        }))
    });
    let shell = host.read_with(cx, |host, _| host.0.clone());
    let options = cx
        .debug_bounds("sessions-options")
        .expect("the import entry must exist before the first Zerun session");
    cx.simulate_click(options.center(), gpui::Modifiers::default());
    shell.read_with(cx, |shell, _| {
        assert!(shell.session_import.menu.is_open());
        assert!(
            shell.sessions_open,
            "opening the menu must not collapse Sessions"
        );
    });
    cx.simulate_click(options.center(), gpui::Modifiers::default());
    shell.read_with(cx, |shell, _| {
        assert!(
            !shell.session_import.menu.is_open(),
            "a second trigger click must dismiss the menu"
        )
    });
}
