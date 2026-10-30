//! Host-resolved configuration paths; only local files can open through the OS.
use crate::{
    settings::widgets,
    state::{AppState, EngineHandle},
    theme::Theme,
};
use gpui::{
    ClipboardItem, Context, Entity, IntoElement, Render, Subscription, Task, Window, div,
    prelude::*, px,
};
use zeron_proto::{HarnessConfigFiles, HarnessId, capabilities};

#[derive(Clone)]
enum FileAction {
    Refresh,
    Copy(String),
    Open(std::path::PathBuf),
}

pub struct ConfigFiles {
    state: Entity<AppState>,
    target: Option<String>,
    harness: HarnessId,
    connection: Option<EngineHandle>,
    data: Option<HarnessConfigFiles>,
    error: Option<String>,
    loading: bool,
    generation: u64,
    task: Option<Task<()>>,
    _observe: Subscription,
}

impl ConfigFiles {
    pub fn new(
        state: Entity<AppState>,
        target: Option<String>,
        harness: HarnessId,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&state, |this, _, cx| {
            let connection = this.state.read(cx).engine().cloned();
            let same = matches!((&this.connection, &connection), (Some(a), Some(b)) if a.same_connection(b))
                || (this.connection.is_none() && connection.is_none());
            if !same {
                this.generation += 1;
                this.task = None; this.data = None; this.error = None; this.loading = false;
                this.connection = connection;
            }
            if this.data.is_none() && this.error.is_none() && !this.loading { this.load(cx); }
            cx.notify();
        });
        let mut this = Self {
            connection: state.read(cx).engine().cloned(),
            state,
            target,
            harness,
            data: None,
            error: None,
            loading: false,
            generation: 0,
            task: None,
            _observe: observe,
        };
        this.load(cx);
        this
    }
    fn activate(&mut self, action: FileAction, cx: &mut Context<Self>) {
        match action {
            FileAction::Refresh => self.load(cx),
            FileAction::Copy(path) => cx.write_to_clipboard(ClipboardItem::new_string(path)),
            FileAction::Open(path) if self.target.is_none() && self.supported(cx) => {
                if path.is_file() {
                    cx.open_with_system(&path);
                } else {
                    self.error = Some(
                        "This file no longer exists. Refresh the list to locate it again.".into(),
                    );
                    cx.notify();
                }
            }
            FileAction::Open(_) => {}
        }
    }
    fn button(
        &self,
        theme: &Theme,
        id: impl Into<gpui::ElementId>,
        label: &'static str,
        action: FileAction,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let keyboard = action.clone();
        widgets::ghost_action(theme)
            .flex_none()
            .id(id)
            .tab_index(0)
            .role(gpui::Role::Button)
            .aria_label(label)
            .focus_visible(|style| style.border_2().border_color(theme.accent))
            .on_click(cx.listener(move |this, _, _, cx| this.activate(action.clone(), cx)))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.activate(keyboard.clone(), cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
    }

    fn supported(&self, cx: &gpui::App) -> bool {
        if matches!(self.harness, HarnessId::Muse | HarnessId::Mock)
            || self.target.as_deref() == Some(zeron_proto::CLOUD_ACCOUNTS_DEVICE)
        {
            return false;
        }
        let state = self.state.read(cx);
        state.engine().is_some_and(|engine| {
            engine
                .engine_info()
                .supports(capabilities::HARNESS_CONFIG_FILES_V1)
        }) && self.target.as_deref().is_none_or(|target| {
            state.device_supports(target, capabilities::HARNESS_CONFIG_FILES_V1)
        })
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.supported(cx) {
            return;
        }
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        self.data = None;
        self.error = None;
        self.loading = true;
        let mut params = serde_json::json!({"harness": self.harness});
        if let Some(target) = &self.target {
            params["targetDeviceId"] = target.clone().into();
        }
        self.task = Some(cx.spawn(async move |this, cx| {
            let reply = crate::attachments::call_with_timeout(
                &engine,
                cx.background_executor(),
                zeron_rpc::methods::LIST_HARNESS_CONFIG_FILES,
                params,
                std::time::Duration::from_secs(20),
            )
            .await;
            let result = reply
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()));
            this.update(cx, |this, cx| {
                if generation != this.generation
                    || !this
                        .state
                        .read(cx)
                        .engine()
                        .is_some_and(|current| current.same_connection(&engine))
                {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(data) => this.data = Some(data),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for ConfigFiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).for_settings_surface();
        let supported = self.supported(cx);
        let local = self.target.is_none();
        let note = if self.target.as_deref() == Some(zeron_proto::CLOUD_ACCOUNTS_DEVICE) {
            "Cloud manages its own provider configuration."
        } else if matches!(self.harness, HarnessId::Muse | HarnessId::Mock) {
            "Configuration files are not available for this provider."
        } else if !supported {
            "Update the execution host and this app to locate configuration files."
        } else if local {
            "User configuration on this computer. Existing files open in your default app."
        } else {
            "These paths belong to the selected remote computer. Open them on that computer."
        };
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(widgets::row_title(&theme, "Configuration files"))
                    .when(supported && !self.loading, |row| {
                        row.child(self.button(
                            &theme,
                            "config-files-refresh",
                            "Refresh",
                            FileAction::Refresh,
                            cx,
                        ))
                    }),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(note),
            );
        if self.loading {
            section = section.child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child("Locating files…"),
            );
        }
        if let Some(error) = &self.error {
            section = section.child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        if let Some(data) = &self.data {
            if let Some(note) = &data.note {
                section = section.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text_muted)
                        .child(note.clone()),
                );
            }
            for (index, file) in data.files.iter().enumerate() {
                let copy = file.path.clone();
                let open = std::path::PathBuf::from(&file.path);
                section = section.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().text_size(px(12.0)).child(format!(
                                    "{}{}",
                                    file.label,
                                    if file.exists { "" } else { " · Not created" }
                                )))
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_muted)
                                        .truncate()
                                        .child(file.path.clone()),
                                ),
                        )
                        .child(self.button(
                            &theme,
                            ("config-copy", index),
                            "Copy path",
                            FileAction::Copy(copy),
                            cx,
                        ))
                        .when(local && file.exists, |row| {
                            row.child(self.button(
                                &theme,
                                ("config-open", index),
                                "Open",
                                FileAction::Open(open),
                                cx,
                            ))
                        }),
                );
            }
        }
        section.into_any_element()
    }
}
