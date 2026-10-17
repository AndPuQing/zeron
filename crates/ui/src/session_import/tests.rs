use super::*;
use crate::popover::Loadable;

fn model() -> ImportModel {
    ImportModel::new(
        vec![
            ImportDevice {
                id: "local".into(),
                name: "This device".into(),
                current_project: Some("/projects/fieldnotes".into()),
            },
            ImportDevice {
                id: "remote".into(),
                name: "Server".into(),
                current_project: None,
            },
        ],
        "local",
    )
}

fn session(id: &str, eligibility: ImportEligibility) -> ExternalSession {
    ExternalSession {
        source_ref: id.into(),
        title: id.into(),
        provider: SessionProvider::Codex,
        project: "fieldnotes".into(),
        cwd: "/projects/fieldnotes".into(),
        updated_at: Utc::now(),
        eligibility,
    }
}

fn request_id(event: SessionImportEvent) -> u64 {
    match event {
        SessionImportEvent::Discover { request_id, .. }
        | SessionImportEvent::Import { request_id, .. } => request_id,
        _ => panic!("expected a request"),
    }
}

use chrono::Utc;

#[test]
fn switching_device_ignores_old_discovery() {
    let mut model = model();
    let old_list = request_id(model.discover(false).unwrap());
    model.device_index = 1;
    let new_list = request_id(model.discover(false).unwrap());
    assert!(!model.receive_discovery(
        old_list,
        Ok(SessionDiscovery {
            sessions: vec![session("wrong-device", ImportEligibility::Available)],
            ..Default::default()
        })
    ));
    assert!(model.receive_discovery(new_list, Ok(SessionDiscovery::default())));
    assert!(model.sessions.is_empty());
    assert!(model.active.is_none());
}

#[test]
fn selection_excludes_unavailable_sessions_and_survives_filters() {
    let mut model = model();
    model.sessions = vec![
        session("ready", ImportEligibility::Available),
        session(
            "managed",
            ImportEligibility::AlreadyManaged {
                chat_id: "chat".into(),
            },
        ),
        session(
            "missing",
            ImportEligibility::Unavailable {
                reason: "Directory missing".into(),
            },
        ),
        session("running", ImportEligibility::Running),
    ];
    assert!(model.selected.is_empty());
    for id in ["ready", "managed", "missing", "running", "unknown"] {
        model.toggle_selection(id);
    }
    assert_eq!(model.selected.len(), 1);
    assert!(model.selected.contains("ready"));
    assert!(model.visible_sessions("no match").is_empty());
    assert_eq!(
        model.selected.len(),
        1,
        "filtering preserves explicit selections"
    );
    let visible = |model: &ImportModel| {
        model
            .visible_sessions("")
            .iter()
            .map(|row| row.source_ref.clone())
            .collect::<Vec<_>>()
    };
    model.status = ImportStatusFilter::Imported;
    assert_eq!(visible(&model), ["managed"]);
    model.status = ImportStatusFilter::NotImported;
    assert_eq!(visible(&model), ["ready", "missing", "running"]);
    assert!(model.selected.contains("ready"));
}

#[test]
fn imported_rows_move_out_of_the_not_imported_filter() {
    let mut model = model();
    model.sessions = vec![session("one", ImportEligibility::Available)];
    model.status = ImportStatusFilter::NotImported;
    model.toggle_selection("one");
    let batch = request_id(model.begin_import(false).unwrap());
    assert!(model.update_item(
        batch,
        "one",
        ImportItemState::Finished(ImportOutcome::Imported {
            chat_id: "chat".into()
        })
    ));
    assert!(model.visible_sessions("").is_empty());
    model.status = ImportStatusFilter::Imported;
    assert_eq!(model.visible_sessions("").len(), 1);
}

#[test]
fn selection_respects_the_engine_batch_limit_and_allows_replacing_a_selection() {
    let mut model = model();
    let limit = zeron_proto::MAX_EXTERNAL_SESSION_IMPORT_BATCH;
    model.sessions = (0..=limit)
        .map(|i| session(&i.to_string(), ImportEligibility::Available))
        .collect();
    for i in 0..=limit {
        model.toggle_selection(&i.to_string());
    }
    assert_eq!(model.selected.len(), limit);
    assert!(!model.selected.contains(&limit.to_string()));
    model.toggle_selection("0");
    model.toggle_selection(&limit.to_string());
    assert_eq!(model.selected.len(), limit);
    assert!(model.selected.contains(&limit.to_string()));
    let SessionImportEvent::Import { source_refs, .. } = model.begin_import(false).unwrap() else {
        panic!("expected import");
    };
    assert_eq!(source_refs.len(), limit);
    assert!(!source_refs.contains(&"0".to_string()));
}

#[test]
fn cancel_preserves_completed_imports_and_retry_only_requests_remaining_items() {
    let mut model = model();
    model.sessions = ["saved", "failed", "pending"]
        .into_iter()
        .map(|id| session(id, ImportEligibility::Available))
        .collect();
    model.selected = model
        .sessions
        .iter()
        .map(|row| row.source_ref.clone())
        .collect();
    let batch = request_id(model.begin_import(false).unwrap());
    assert!(model.update_item(
        batch,
        "saved",
        ImportItemState::Finished(ImportOutcome::Imported {
            chat_id: "chat-saved".into()
        })
    ));
    assert!(model.update_item(
        batch,
        "failed",
        ImportItemState::Finished(ImportOutcome::Failed {
            reason: "Source changed".into(),
            retryable: true
        })
    ));
    assert!(model.cancel().is_some());
    assert!(model.cancel().is_none(), "cancel is idempotent");
    assert!(model.finish_import(batch, true));
    assert_eq!(model.batch.as_ref().unwrap().imported(), 1);
    assert_eq!(
        model.batch.as_ref().unwrap().states["pending"],
        ImportItemState::Cancelled
    );
    let event = model.begin_import(true).unwrap();
    let SessionImportEvent::Import {
        request_id: retry,
        source_refs,
        ..
    } = event
    else {
        panic!("expected import");
    };
    assert_eq!(source_refs, ["failed", "pending"]);
    assert!(!model.update_item(
        batch,
        "pending",
        ImportItemState::Finished(ImportOutcome::Imported {
            chat_id: "stale".into()
        })
    ));
    assert!(!model.update_item(retry, "saved", ImportItemState::Importing));
    assert_eq!(model.batch.as_ref().unwrap().imported(), 1);
}

#[test]
fn paging_deduplicates_sessions_and_removes_selection_if_eligibility_changes() {
    let mut model = model();
    let first = request_id(model.discover(false).unwrap());
    model.receive_discovery(
        first,
        Ok(SessionDiscovery {
            sessions: vec![session("one", ImportEligibility::Available)],
            next_cursor: Some("page-two".into()),
            source_errors: vec![],
        }),
    );
    model.toggle_selection("one");
    let second = request_id(model.discover(true).unwrap());
    model.receive_discovery(
        second,
        Ok(SessionDiscovery {
            sessions: vec![
                session(
                    "one",
                    ImportEligibility::AlreadyManaged {
                        chat_id: "existing".into(),
                    },
                ),
                session("two", ImportEligibility::Available),
            ],
            ..Default::default()
        }),
    );
    assert_eq!(model.sessions.len(), 2);
    assert!(model.selected.is_empty());
}

#[test]
fn project_filter_respects_directory_boundaries_and_remote_windows_paths() {
    assert!(path_within(
        "/projects/fieldnotes/branch",
        "/projects/fieldnotes"
    ));
    assert!(!path_within(
        "/projects/fieldnotes-old",
        "/projects/fieldnotes"
    ));
    assert!(path_within(
        r"C:\Projects\Fieldnotes\branch",
        r"c:\projects\fieldnotes"
    ));
    assert!(!path_within(
        r"C:\Projects\Fieldnotes-old",
        r"c:\projects\fieldnotes"
    ));
}

#[test]
fn filter_row_hides_choices_the_scan_has_nothing_to_filter() {
    let mut single = model();
    single.sessions = vec![session("codex", ImportEligibility::Available)];
    assert!(!needs_provider_filter(&single));
    assert!(!needs_status_filter(&single));

    let mut mixed = model();
    let mut claude = session("claude", ImportEligibility::Available);
    claude.provider = SessionProvider::ClaudeCode;
    mixed.sessions = vec![session("codex", ImportEligibility::Available), claude];
    assert!(needs_provider_filter(&mixed));
    assert!(
        !needs_status_filter(&mixed),
        "nothing imported yet, so nothing to filter by status"
    );

    let mut imported = model();
    imported.sessions = vec![session(
        "managed",
        ImportEligibility::AlreadyManaged {
            chat_id: "chat".into(),
        },
    )];
    assert!(needs_status_filter(&imported));

    // A filter that is currently applied stays visible even when the scan no
    // longer justifies it, so it can always be cleared again.
    let mut applied = model();
    applied.sessions = vec![session("codex", ImportEligibility::Available)];
    applied.provider = Some(SessionProvider::ClaudeCode);
    applied.status = ImportStatusFilter::Imported;
    assert!(needs_provider_filter(&applied));
    assert!(needs_status_filter(&applied));
}

#[test]
fn mixed_project_import_preserves_the_choosers_group_order() {
    let mut model = model();
    let mut other = session("other-project", ImportEligibility::Available);
    other.cwd = "/projects/other".into();
    other.project = "other".into();
    model.sessions = vec![
        session("first", ImportEligibility::Available),
        other,
        session("second", ImportEligibility::Available),
    ];
    model.current_project_only = false;
    model.selected = model
        .sessions
        .iter()
        .map(|row| row.source_ref.clone())
        .collect();
    assert_eq!(
        model
            .visible_sessions("")
            .iter()
            .map(|row| row.source_ref.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "other-project"]
    );
    let SessionImportEvent::Import { source_refs, .. } = model.begin_import(false).unwrap() else {
        panic!("expected import");
    };
    assert_eq!(source_refs, ["first", "second", "other-project"]);
}

#[gpui::test]
fn native_row_and_checkbox_click_toggle_selection_once(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
        dialog.model.sessions = vec![session("ready", ImportEligibility::Available)];
        dialog.model.discovery = Loadable::Ready(());
        dialog
    });
    let selection = cx.debug_bounds("import-select-0").unwrap();
    cx.simulate_click(selection.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert!(view.model.selected.contains("ready"));
        assert_eq!(view.model.active.as_deref(), Some("ready"));
    });
    let row = cx.debug_bounds("import-row-0").unwrap();
    cx.simulate_click(
        gpui::point(row.right() - px(30.0), row.center().y),
        gpui::Modifiers::default(),
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.model.active.as_deref(), Some("ready"));
        assert!(view.model.selected.is_empty());
    });
    cx.simulate_keystrokes("down");
    view.read_with(cx, |view, _| {
        assert_eq!(view.model.active.as_deref(), Some("ready"));
        assert!(
            view.model.selected.is_empty(),
            "navigation only moves focus"
        );
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |view, _| assert!(view.model.selected.contains("ready")));
    cx.simulate_keystrokes("space");
    view.read_with(cx, |view, _| assert!(view.model.selected.is_empty()));
}

/// A dialog with two providers and one already imported session: enough for
/// both pickers to be offered.
fn filtered_dialog(cx: &mut gpui::Context<SessionImportDialog>) -> SessionImportDialog {
    let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
    let mut claude = session("claude", ImportEligibility::Available);
    claude.provider = SessionProvider::ClaudeCode;
    dialog.model.sessions = vec![
        session("ready", ImportEligibility::Available),
        claude,
        session(
            "managed",
            ImportEligibility::AlreadyManaged {
                chat_id: "chat".into(),
            },
        ),
    ];
    dialog.model.discovery = Loadable::Ready(());
    dialog
}

#[gpui::test]
fn native_agent_filter_picks_a_provider_from_its_menu(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| filtered_dialog(cx));
    let trigger = cx.debug_bounds("import-agent-filter-trigger").unwrap();
    cx.simulate_click(trigger.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert_eq!(view.filter_menu.get(), Some(&FilterMenu::Agent))
    });
    let option = cx.debug_bounds("import-agent-option-2").unwrap();
    cx.simulate_click(option.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert_eq!(view.model.provider, Some(SessionProvider::ClaudeCode));
        assert!(view.filter_menu.as_open().is_none());
        assert_eq!(
            view.model
                .visible_sessions("")
                .iter()
                .map(|row| row.source_ref.as_str())
                .collect::<Vec<_>>(),
            ["claude"]
        );
    });
}

#[gpui::test]
fn native_status_filter_picks_an_eligibility_from_its_menu(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| filtered_dialog(cx));
    let trigger = cx.debug_bounds("import-status-filter-trigger").unwrap();
    cx.simulate_click(trigger.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert_eq!(view.filter_menu.get(), Some(&FilterMenu::Status))
    });
    let option = cx.debug_bounds("import-status-option-1").unwrap();
    cx.simulate_click(option.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert_eq!(view.model.status, ImportStatusFilter::NotImported);
        assert!(
            view.model
                .visible_sessions("")
                .iter()
                .all(|row| !matches!(
                    row.eligibility,
                    ImportEligibility::AlreadyManaged { .. }
                ))
        );
    });
}

#[gpui::test]
fn native_group_header_toggles_every_importable_session_in_its_project(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
        dialog.model.sessions = vec![
            session("ready", ImportEligibility::Available),
            session("also-ready", ImportEligibility::Available),
            session(
                "managed",
                ImportEligibility::AlreadyManaged {
                    chat_id: "chat".into(),
                },
            ),
        ];
        dialog.model.discovery = Loadable::Ready(());
        dialog
    });
    let header = cx.debug_bounds("import-group-0").unwrap();
    cx.simulate_click(header.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| {
        assert!(view.model.selected.contains("ready"));
        assert!(view.model.selected.contains("also-ready"));
        assert!(
            !view.model.selected.contains("managed"),
            "an already imported session is not selectable"
        );
    });
    let header = cx.debug_bounds("import-group-0").unwrap();
    cx.simulate_click(header.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| assert!(view.model.selected.is_empty()));
}

#[gpui::test]
fn native_import_action_requires_selection_and_keyboard_stays_in_dialog(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
        dialog.model.sessions = vec![session("ready", ImportEligibility::Available)];
        dialog.model.discovery = Loadable::Ready(());
        dialog
    });
    let submit = cx.debug_bounds("import-submit").unwrap();
    cx.simulate_click(submit.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| assert!(view.model.batch.is_none()));
    let selection = cx.debug_bounds("import-select-0").unwrap();
    cx.simulate_click(selection.center(), gpui::Modifiers::default());
    let submit = cx.debug_bounds("import-submit").unwrap();
    cx.simulate_click(submit.center(), gpui::Modifiers::default());
    view.read_with(cx, |view, _| assert!(view.is_importing()));
    for _ in 0..12 {
        cx.simulate_keystrokes("tab");
        cx.update(|window, cx| assert!(view.read(cx).focus.contains_focused(window, cx)));
    }
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |view, _| {
        assert!(view.model.batch.as_ref().unwrap().cancelling)
    });
}

#[gpui::test]
fn native_narrow_layout_keeps_footer_visible_in_light_and_dark(cx: &mut gpui::TestAppContext) {
    for theme in [Theme::dark(), Theme::light()] {
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(theme);
        });
        let (_view, cx) = cx.add_window_view(|_, cx| {
            let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
            dialog.model.sessions = vec![session("ready", ImportEligibility::Available)];
            dialog.model.discovery = Loadable::Ready(());
            dialog
        });
        cx.simulate_resize(gpui::size(px(640.0), px(600.0)));
        let dialog = cx.debug_bounds("session-import-dialog").unwrap();
        let submit = cx.debug_bounds("import-submit").unwrap();
        assert!(dialog.right() <= px(640.0));
        assert!(submit.bottom() <= dialog.bottom());
        assert!(submit.left() >= dialog.left());
        let list = cx.debug_bounds("import-session-list").unwrap();
        assert!(list.left() >= dialog.left());
        assert!(list.right() <= dialog.right());
        assert!(list.size.width >= dialog.size.width - px(4.0));
    }
}

/// The dialog mounts as a priority-2 modal layer, so its device dropdown must
/// use a higher deferred layer or it paints underneath the card. Paint order is
/// not observable from the in-process test API; this covers the interaction
/// path (open the dropdown, pick a device) that the layering bug broke in the
/// running app.
struct ImportModalHarness {
    dialog: Entity<SessionImportDialog>,
}

impl Render for ImportModalHarness {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(popover::modal(
            "import-modal-test",
            window.viewport_size(),
            self.dialog.clone().into_any_element(),
        ))
    }
}

#[gpui::test]
fn device_menu_selects_a_device_inside_the_modal(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_global(Theme::default());
        crate::motion::set_reduced_motion(cx, true);
        crate::composer::init(cx, Default::default());
    });
    let (view, cx) = cx.add_window_view(|_, cx| ImportModalHarness {
        dialog: cx.new(|cx| {
            let mut dialog = SessionImportDialog::new(model().devices, "local", cx);
            dialog.model.sessions = vec![session("ready", ImportEligibility::Available)];
            dialog.model.discovery = Loadable::Ready(());
            dialog
        }),
    });
    cx.simulate_resize(gpui::size(px(720.0), px(640.0)));
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
    assert!(
        cx.debug_bounds("session-import-dialog").is_some(),
        "the dialog must render inside the modal wrapper"
    );

    let trigger = cx.debug_bounds("import-source-device").unwrap();
    cx.simulate_click(trigger.center(), gpui::Modifiers::default());
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
    view.read_with(cx, |harness, cx| {
        assert!(
            harness.dialog.read(cx).device_menu.get().is_some(),
            "clicking the trigger must open the device menu"
        );
    });
    let remote = cx.debug_bounds("import-device-1").unwrap();
    cx.simulate_click(remote.center(), gpui::Modifiers::default());

    view.read_with(cx, |harness, cx| {
        assert_eq!(
            harness.dialog.read(cx).model.device_index,
            1,
            "clicking a row in the device dropdown must reach the menu"
        );
    });
}
