//! Ephemeral, device-addressed environment editor. Values never enter UI settings.
use crate::{
    settings::widgets::{self, ActionTone},
    state::{AppState, EngineHandle},
    theme::Theme,
};
use gpui::{
    AnyElement, Context, Entity, IntoElement, KeyDownEvent, Render, Subscription, Task, Window,
    div, prelude::*, px,
};
use gpui_base::input::{Input, InputState, Textarea, TextareaState};
use zeron_proto::{
    EnvironmentAction, EnvironmentChange, EnvironmentEntryMetadata, HarnessEnvironmentMetadata,
    HarnessId, PatchHarnessEnvironmentReply, RevealedEnvironmentValue,
};
use zeron_rpc::methods;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftAction {
    Keep,
    Set,
    Unset,
    Delete,
}
struct Row {
    id: u64,
    original: Option<EnvironmentEntryMetadata>,
    name: Option<Entity<InputState>>,
    value: Option<Entity<InputState>>,
    multiline: Option<Entity<TextareaState>>,
    action: DraftAction,
    sensitive: bool,
    revealed: Option<String>,
    error: Option<String>,
}
impl Row {
    fn name(&self, cx: &gpui::App) -> String {
        self.original
            .as_ref()
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| {
                self.name
                    .as_ref()
                    .map(|input| input.read(cx).value().to_string())
                    .unwrap_or_default()
            })
    }
    fn value(&self, cx: &gpui::App) -> String {
        self.multiline
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .or_else(|| {
                self.value
                    .as_ref()
                    .map(|input| input.read(cx).value().to_string())
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Reload,
    Save,
    Cancel,
    Add,
    Replace,
    Unset,
    Delete,
    Reveal,
    Hide,
    Sensitive,
    Multiline,
}

pub struct EnvironmentEditor {
    state: Entity<AppState>,
    target: Option<String>,
    harness: HarnessId,
    connection: Option<EngineHandle>,
    metadata: Option<HarnessEnvironmentMetadata>,
    rows: Vec<Row>,
    next_id: u64,
    generation: u64,
    loading: bool,
    saving: bool,
    uncertain: bool,
    error: Option<String>,
    task: Option<Task<()>>,
    reveal_task: Option<Task<()>>,
    _observe: Subscription,
}

impl EnvironmentEditor {
    pub fn new(
        state: Entity<AppState>,
        target: Option<String>,
        harness: HarnessId,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&state, |editor, _, cx| {
            let connection = editor.state.read(cx).engine().cloned();
            let unchanged = matches!((&editor.connection, &connection), (Some(a), Some(b)) if a.same_connection(b))
                || (editor.connection.is_none() && connection.is_none());
            if !unchanged {
                for row in &mut editor.rows {
                    row.revealed = None;
                }
                editor.generation = editor.generation.wrapping_add(1);
                editor.task = None;
                editor.reveal_task = None;
                editor.metadata = None;
                editor.uncertain |= editor.saving;
                editor.loading = false;
                editor.saving = false;
                editor.connection = connection;
                editor.load(true, cx);
            } else if editor.metadata.is_none() && !editor.loading && editor.supported(cx) {
                editor.load(true, cx);
            }
            cx.notify();
        });
        let mut editor = Self {
            connection: state.read(cx).engine().cloned(),
            state,
            target,
            harness,
            metadata: None,
            rows: vec![],
            next_id: 0,
            generation: 0,
            loading: false,
            saving: false,
            uncertain: false,
            error: None,
            task: None,
            reveal_task: None,
            _observe: observe,
        };
        editor.load(false, cx);
        editor
    }

    fn supported(&self, cx: &gpui::App) -> bool {
        let state = self.state.read(cx);
        self.target.as_deref().map_or_else(
            || {
                state.engine().is_some_and(|engine| {
                    engine
                        .engine_info()
                        .supports(zeron_proto::capabilities::HARNESS_ENVIRONMENT_V1)
                })
            },
            |target| {
                state.device_supports(target, zeron_proto::capabilities::HARNESS_ENVIRONMENT_V1)
            },
        )
    }

    fn current(&self, generation: u64, connection: &EngineHandle, cx: &gpui::App) -> bool {
        self.generation == generation
            && self
                .state
                .read(cx)
                .engine()
                .is_some_and(|current| current.same_connection(connection))
    }

    fn params(&self, mut params: serde_json::Value) -> serde_json::Value {
        params["harness"] = serde_json::json!(self.harness);
        if let Some(target) = &self.target {
            params["targetDeviceId"] = serde_json::json!(target);
        }
        params
    }

    fn install_metadata(&mut self, metadata: HarnessEnvironmentMetadata, preserve: bool) {
        if !preserve {
            self.rows.clear();
        } else {
            self.rows.retain(|row| row.action != DraftAction::Keep);
        }
        for row in &mut self.rows {
            row.revealed = None;
        }
        for entry in &metadata.entries {
            if self.rows.iter().any(|row| {
                row.original
                    .as_ref()
                    .is_some_and(|old| old.name == entry.name)
            }) {
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            self.rows.push(Row {
                id,
                original: Some(entry.clone()),
                name: None,
                value: None,
                multiline: None,
                action: DraftAction::Keep,
                sensitive: entry.sensitive,
                revealed: None,
                error: None,
            });
        }
        self.metadata = Some(metadata);
    }

    fn load(&mut self, preserve: bool, cx: &mut Context<Self>) {
        if self.loading || self.saving || !self.supported(cx) {
            return;
        }
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        self.reveal_task = None;
        for row in &mut self.rows {
            row.revealed = None;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let params = self.params(serde_json::json!({}));
        self.loading = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::GET_HARNESS_ENVIRONMENT, params)
                .await
                .and_then(|value| {
                    serde_json::from_value::<HarnessEnvironmentMetadata>(value).map_err(|_| {
                        zeron_rpc::RpcError::Failed("Invalid environment metadata reply".into())
                    })
                });
            this.update(cx, |editor, cx| {
                if !editor.current(generation, &engine, cx) {
                    return;
                }
                editor.loading = false;
                match result {
                    Ok(metadata) => {
                        editor.install_metadata(metadata, preserve);
                        editor.uncertain = false;
                    }
                    Err(error) => {
                        editor.error = Some(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn changes(&mut self, cx: &gpui::App) -> Result<Vec<EnvironmentChange>, ()> {
        let mut names = std::collections::HashSet::new();
        let mut changes = vec![];
        let mut valid = true;
        for row in &mut self.rows {
            row.error = None;
            let name = row.name(cx);
            if row.action == DraftAction::Keep {
                continue;
            }
            if let Err(error) = zeron_harness::environment::validate_name(&name) {
                row.error = Some(error);
                valid = false;
                continue;
            }
            let normalized = if self
                .metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.policy.case_sensitive)
            {
                name.to_ascii_uppercase()
            } else {
                name.clone()
            };
            if !names.insert(normalized) {
                row.error = Some("Duplicate variable name".into());
                valid = false;
                continue;
            }
            let change = match row.action {
                DraftAction::Set => {
                    let value = row.value(cx);
                    if value.contains('\0')
                        || value.len() > zeron_harness::environment::MAX_VALUE_BYTES
                    {
                        row.error = Some("Values must have no NUL and be at most 16 KiB".into());
                        valid = false;
                        continue;
                    }
                    EnvironmentChange::Set {
                        name,
                        value,
                        sensitive: row.sensitive,
                    }
                }
                DraftAction::Unset => EnvironmentChange::Unset { name },
                DraftAction::Delete => EnvironmentChange::Delete { name },
                DraftAction::Keep => unreachable!(),
            };
            changes.push(change);
        }
        if valid { Ok(changes) } else { Err(()) }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.saving || self.uncertain {
            return;
        }
        let Some(metadata) = &self.metadata else {
            return;
        };
        let revision = metadata.revision.clone();
        let Ok(changes) = self.changes(cx) else {
            cx.notify();
            return;
        };
        if changes.is_empty() {
            return;
        }
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.error = Some("Device unavailable; your draft is preserved".into());
            cx.notify();
            return;
        };
        let params =
            self.params(serde_json::json!({"expectedRevision":revision, "changes":changes}));
        let generation = self.generation;
        self.saving = true;
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = engine.client()
                .call(methods::PATCH_HARNESS_ENVIRONMENT, params).await
                .and_then(|value| serde_json::from_value::<PatchHarnessEnvironmentReply>(value)
                    .map_err(|_| zeron_rpc::RpcError::Failed("Invalid environment save reply".into())));
            this.update(cx, |editor, cx| {
                if !editor.current(generation, &engine, cx) {
                    return;
                }
                editor.saving = false;
                match result {
                    Ok(reply) => {
                        if !reply.conflict {
                            let target = editor.target.clone()
                                .unwrap_or_else(|| engine.engine_info().device_id.clone());
                            crate::pickers::bump_harness_environment(target, editor.harness, cx);
                        }
                        editor.install_metadata(reply.metadata, reply.conflict);
                        if reply.conflict {
                            editor.error = Some("Another editor changed these settings. Review your draft and save again.".into());
                        }
                    },
                    Err(error) => {
                        editor.error = Some(format!("{error}. Reload status before retrying; your draft is preserved."));
                        editor.uncertain = true;
                    },
                }
                cx.notify();
            }).ok();
        }));
        cx.notify();
    }

    fn reveal(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(metadata) = &self.metadata else {
            return;
        };
        let revision = metadata.revision.clone();
        let Some(row) = self
            .rows
            .iter()
            .find(|row| row.id == id && row.action == DraftAction::Keep)
        else {
            return;
        };
        let name = row.name(cx);
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        let params = self.params(serde_json::json!({"name":name, "expectedRevision":revision}));
        let generation = self.generation;
        self.reveal_task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::REVEAL_HARNESS_ENVIRONMENT_VALUE, params)
                .await
                .and_then(|value| {
                    serde_json::from_value::<RevealedEnvironmentValue>(value)
                        .map_err(|_| zeron_rpc::RpcError::Failed("Invalid reveal reply".into()))
                });
            this.update(cx, |editor, cx| {
                if !editor.current(generation, &engine, cx)
                    || editor
                        .metadata
                        .as_ref()
                        .is_none_or(|metadata| metadata.revision != revision)
                {
                    return;
                }
                if let Some(row) = editor
                    .rows
                    .iter_mut()
                    .find(|row| row.id == id && row.action == DraftAction::Keep)
                {
                    match result {
                        Ok(value) if value.revision == revision => row.revealed = Some(value.value),
                        Ok(_) => {}
                        Err(error) => row.error = Some(error.to_string()),
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn perform(&mut self, action: Action, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.loading {
            return;
        }
        match action {
            Action::Reload => self.load(true, cx),
            Action::Save => self.save(cx),
            Action::Cancel => {
                self.generation = self.generation.wrapping_add(1);
                self.reveal_task = None;
                self.error = None;
                if self.uncertain {
                    self.load(false, cx);
                } else if let Some(metadata) = self.metadata.clone() {
                    self.install_metadata(metadata, false);
                }
            }
            Action::Reveal => self.reveal(id, cx),
            Action::Hide => {
                self.reveal_task = None;
                self.generation = self.generation.wrapping_add(1);
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.revealed = None;
                }
            }
            Action::Add => {
                let id = self.next_id;
                self.next_id += 1;
                self.rows.push(Row {
                    id,
                    original: None,
                    name: Some(
                        cx.new(|cx| InputState::new(window, cx).placeholder("VARIABLE_NAME")),
                    ),
                    value: Some(cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder("Literal value (empty is allowed)")
                            .masked(true)
                    })),
                    multiline: None,
                    action: DraftAction::Set,
                    sensitive: true,
                    revealed: None,
                    error: None,
                });
            }
            _ => {
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.revealed = None;
                    match action {
                        Action::Replace => {
                            row.action = DraftAction::Set;
                            row.value = Some(cx.new(|cx| {
                                InputState::new(window, cx)
                                    .placeholder("Replacement value")
                                    .masked(true)
                            }));
                            row.multiline = None;
                        }
                        Action::Unset => row.action = DraftAction::Unset,
                        Action::Delete => {
                            if row.original.is_none() {
                                self.rows.retain(|row| row.id != id);
                            } else {
                                row.action = DraftAction::Delete;
                            }
                        }
                        Action::Hide => {}
                        Action::Sensitive => row.sensitive = !row.sensitive,
                        Action::Multiline => {
                            let value = row.value(cx);
                            row.multiline = Some(cx.new(|cx| {
                                TextareaState::new(window, cx)
                                    .default_value(value)
                                    .auto_grow(2, 5)
                            }));
                            row.value = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        cx.notify();
    }

    fn button(
        &self,
        theme: &Theme,
        key: &'static str,
        label: &'static str,
        action: Action,
        id: u64,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        widgets::action_button(theme, ActionTone::Quiet)
            .id((key, id))
            .role(gpui::Role::Button)
            .aria_label(label)
            .tab_index(0)
            .child(label)
            .on_click(
                cx.listener(move |editor, _, window, cx| editor.perform(action, id, window, cx)),
            )
            .on_key_down(
                cx.listener(move |editor, event: &KeyDownEvent, window, cx| {
                    if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        editor.perform(action, id, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
    }
}

impl Render for EnvironmentEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let readonly = self.loading || self.saving;
        for row in &self.rows {
            for input in [row.name.as_ref(), row.value.as_ref()]
                .into_iter()
                .flatten()
            {
                input.update(cx, |input, cx| input.set_readonly(readonly, cx));
            }
            if let Some(input) = &row.multiline {
                input.update(cx, |input, cx| input.set_readonly(readonly, cx));
            }
        }
        let theme = Theme::of(cx).for_settings_surface();
        let device = self.target.as_deref().unwrap_or("This device");
        let mut content = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(widgets::section_label(
                &theme,
                format!("Environment variables · {device}"),
            ));
        if !self.supported(cx) {
            return content
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .child("This device does not support environment settings."),
                )
                .into_any_element();
        }
        content = content.child(div().text_sm().text_color(theme.text_muted).child("Saved changes apply to new agent processes. Active tasks continue with their current settings. Credential overrides may take precedence over saved CLI accounts."));
        if let Some(metadata) = &self.metadata
            && !metadata.previous_environment_sessions.is_empty()
        {
            content = content.child(div().text_sm().text_color(theme.warning_muted).child(
                format!(
                    "Sessions using previous settings: {}",
                    metadata.previous_environment_sessions.join(", ")
                ),
            ));
        }
        if let Some(error) = &self.error {
            content = content.child(widgets::error_strip(&theme, error.clone()));
        }
        if self.loading {
            content = content.child("Loading environment settings…");
        }
        for row in &self.rows {
            let id = row.id;
            let name: AnyElement = match &row.name {
                Some(input) => div()
                    .min_w(px(160.0))
                    .flex_1()
                    .child(Input::new(input))
                    .into_any_element(),
                None => div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(row.name(cx))
                    .into_any_element(),
            };
            let mut card = div()
                .id(("environment-row", id))
                .flex()
                .flex_col()
                .gap(px(5.0))
                .p(px(8.0))
                .rounded(px(8.0))
                .bg(theme.input_glass_bg())
                .child(name);
            let actions = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(4.0));
            let actions = match row.action {
                DraftAction::Keep => {
                    let set = row
                        .original
                        .as_ref()
                        .is_some_and(|entry| entry.action == EnvironmentAction::Set);
                    card = card.child(div().text_sm().text_color(theme.text_muted).child(
                        row.revealed.clone().unwrap_or_else(|| {
                            if set {
                                "••••••••".into()
                            } else {
                                "Removed from child environment".into()
                            }
                        }),
                    ));
                    actions
                        .child(self.button(
                            &theme,
                            "environment-replace",
                            "Replace",
                            Action::Replace,
                            id,
                            cx,
                        ))
                        .when(set, |actions| {
                            actions.child(self.button(
                                &theme,
                                "environment-show",
                                if row.revealed.is_some() {
                                    "Hide value"
                                } else {
                                    "Show value"
                                },
                                if row.revealed.is_some() {
                                    Action::Hide
                                } else {
                                    Action::Reveal
                                },
                                id,
                                cx,
                            ))
                        })
                        .child(self.button(
                            &theme,
                            "environment-unset",
                            "Remove from child",
                            Action::Unset,
                            id,
                            cx,
                        ))
                        .child(self.button(
                            &theme,
                            "environment-delete",
                            "Restore inheritance",
                            Action::Delete,
                            id,
                            cx,
                        ))
                }
                DraftAction::Set => {
                    if let Some(input) = &row.multiline {
                        card = card.child(Textarea::new(input)).child(
                            div()
                                .text_sm()
                                .text_color(theme.text_muted)
                                .child("Multiline values are visible while editing."),
                        );
                    } else if let Some(input) = &row.value {
                        card = card.child(Input::new(input));
                    }
                    actions
                        .child(self.button(
                            &theme,
                            "environment-sensitive",
                            if row.sensitive {
                                "Sensitive: on"
                            } else {
                                "Sensitive: off"
                            },
                            Action::Sensitive,
                            id,
                            cx,
                        ))
                        .when(row.multiline.is_none(), |actions| {
                            actions.child(self.button(
                                &theme,
                                "environment-multiline",
                                "Edit multiline (visible)",
                                Action::Multiline,
                                id,
                                cx,
                            ))
                        })
                        .child(self.button(
                            &theme,
                            "environment-unset",
                            "Remove from child",
                            Action::Unset,
                            id,
                            cx,
                        ))
                        .child(self.button(
                            &theme,
                            "environment-delete",
                            if row.original.is_some() {
                                "Restore inheritance"
                            } else {
                                "Remove row"
                            },
                            Action::Delete,
                            id,
                            cx,
                        ))
                }
                DraftAction::Unset | DraftAction::Delete => {
                    card = card.child(div().text_sm().text_color(theme.text_muted).child(
                        if row.action == DraftAction::Unset {
                            "Will be removed from child environment"
                        } else {
                            "Will inherit the host default"
                        },
                    ));
                    actions
                        .child(self.button(
                            &theme,
                            "environment-replace",
                            "Set value",
                            Action::Replace,
                            id,
                            cx,
                        ))
                        .child(self.button(
                            &theme,
                            "environment-delete",
                            "Restore inheritance",
                            Action::Delete,
                            id,
                            cx,
                        ))
                }
            };
            card = card.child(actions);
            if let Some(error) = &row.error {
                card = card.child(
                    div()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error.clone()),
                );
            }
            content = content.child(card);
        }
        let buttons = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(4.0))
            .child(self.button(
                &theme,
                "environment-add",
                "Add variable",
                Action::Add,
                0,
                cx,
            ))
            .child(self.button(
                &theme,
                "environment-save",
                if self.saving { "Saving…" } else { "Save" },
                Action::Save,
                0,
                cx,
            ))
            .child(self.button(
                &theme,
                "environment-cancel",
                "Cancel draft",
                Action::Cancel,
                0,
                cx,
            ))
            .child(self.button(
                &theme,
                "environment-reload",
                "Reload status",
                Action::Reload,
                0,
                cx,
            ));
        content.child(buttons).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext, WindowHandle};
    use zeron_rpc::{ClientFrame, ServerFrame};
    type Wire = (
        tokio::sync::mpsc::Receiver<String>,
        tokio::sync::mpsc::Sender<String>,
    );
    fn connection() -> (EngineHandle, Wire) {
        let (out, requests) = tokio::sync::mpsc::channel(64);
        let (replies, inbound) = tokio::sync::mpsc::channel(64);
        (
            EngineHandle::from_test_client(zeron_rpc::RpcClient::new(out, inbound))
                .with_test_capabilities(vec![
                    zeron_proto::capabilities::HARNESS_ENVIRONMENT_V1.into(),
                ]),
            (requests, replies),
        )
    }
    fn settle(runtime: &tokio::runtime::Runtime, cx: &mut TestAppContext) {
        for _ in 0..8 {
            cx.run_until_parked();
            runtime.block_on(tokio::task::yield_now());
        }
    }
    fn request(wire: &mut Wire) -> ClientFrame {
        loop {
            let request: ClientFrame =
                serde_json::from_str(&wire.0.try_recv().expect("expected editor request")).unwrap();
            if !request.cancel {
                return request;
            }
        }
    }
    fn reply(wire: &Wire, id: u64, ok: Option<serde_json::Value>, error: Option<&str>) {
        wire.1
            .try_send(
                serde_json::to_string(&ServerFrame {
                    id,
                    ok,
                    err: error.map(str::to_owned),
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
    }
    fn metadata(revision: &str) -> HarnessEnvironmentMetadata {
        let mut metadata = zeron_harness::environment::EnvironmentSnapshot::default().metadata();
        metadata.revision = revision.into();
        metadata.entries = vec![EnvironmentEntryMetadata {
            name: "API_KEY".into(),
            action: EnvironmentAction::Set,
            sensitive: true,
        }];
        metadata
    }
    fn setup(
        cx: &mut TestAppContext,
        handle: EngineHandle,
    ) -> (Entity<AppState>, WindowHandle<EnvironmentEditor>) {
        let directory = tempfile::tempdir().unwrap();
        cx.update(|cx| {
            crate::settings::init(Default::default(), directory.path(), cx);
            gpui_base::init(cx);
            cx.set_global(Theme::default());
        });
        let state = cx.new(|_| AppState::new());
        state.update(cx, |state, _| state.set_test_engine(handle));
        let model = state.clone();
        let window =
            cx.add_window(move |_, cx| EnvironmentEditor::new(model, None, HarnessId::Grok, cx));
        (state, window)
    }
    #[gpui::test]
    fn editor_masks_values_preserves_literal_drafts_and_requires_reload_after_lost_ack(
        cx: &mut TestAppContext,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let (handle, mut wire) = connection();
        let (_state, window) = setup(cx, handle);
        settle(&runtime, cx);
        let get = request(&mut wire);
        assert_eq!(
            get.method.as_deref(),
            Some(methods::GET_HARNESS_ENVIRONMENT)
        );
        reply(
            &wire,
            get.id,
            Some(serde_json::to_value(metadata("one")).unwrap()),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| {
                assert_eq!(editor.rows.len(), 1);
                assert!(editor.rows[0].value.is_none() && editor.rows[0].revealed.is_none());
                editor.reveal(editor.rows[0].id, cx);
            })
            .unwrap();
        settle(&runtime, cx);
        let reveal = request(&mut wire);
        assert_eq!(
            reveal.method.as_deref(),
            Some(methods::REVEAL_HARNESS_ENVIRONMENT_VALUE)
        );
        reply(
            &wire,
            reveal.id,
            Some(serde_json::json!({"revision":"one", "value":"private-revealed"})),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, window, cx| {
                assert_eq!(editor.rows[0].revealed.as_deref(), Some("private-revealed"));
                editor.perform(Action::Hide, editor.rows[0].id, window, cx);
                assert!(editor.rows[0].revealed.is_none());
                editor.perform(Action::Replace, editor.rows[0].id, window, cx);
                assert!(
                    editor.rows[0]
                        .value
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .presentation()
                        .is_masked()
                );
                editor.perform(Action::Multiline, editor.rows[0].id, window, cx);
                editor.rows[0]
                    .multiline
                    .as_ref()
                    .unwrap()
                    .update(cx, |input, cx| {
                        input.set_value("  中文\n$HOME `literal`  ", window, cx)
                    });
                editor.save(cx);
            })
            .unwrap();
        settle(&runtime, cx);
        let save = request(&mut wire);
        assert_eq!(
            save.params["changes"][0]["value"],
            "  中文\n$HOME `literal`  "
        );
        reply(
            &wire,
            save.id,
            Some(serde_json::json!({"conflict":true, "metadata":metadata("two")})),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| {
                assert_eq!(editor.rows[0].value(cx), "  中文\n$HOME `literal`  ");
                assert_eq!(editor.metadata.as_ref().unwrap().revision, "two");
                editor.save(cx);
            })
            .unwrap();
        settle(&runtime, cx);
        let retry = request(&mut wire);
        reply(
            &wire,
            retry.id,
            None,
            Some("connection closed before acknowledgement"),
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| {
                assert!(editor.uncertain);
                editor.save(cx);
            })
            .unwrap();
        settle(&runtime, cx);
        assert!(
            wire.0.try_recv().is_err(),
            "must not automatically replay save"
        );
        window
            .update(cx, |editor, _, cx| editor.load(true, cx))
            .unwrap();
        settle(&runtime, cx);
        let reload = request(&mut wire);
        reply(
            &wire,
            reload.id,
            Some(serde_json::to_value(metadata("three")).unwrap()),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| {
                assert!(!editor.uncertain);
                editor.save(cx);
            })
            .unwrap();
        settle(&runtime, cx);
        let retry = request(&mut wire);
        assert_eq!(retry.params["expectedRevision"], "three");
        reply(
            &wire,
            retry.id,
            Some(serde_json::json!({"conflict":false, "metadata":metadata("four")})),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, _| {
                assert!(editor.rows.iter().all(|row| row.action == DraftAction::Keep
                    && row.value.is_none()
                    && row.multiline.is_none()
                    && row.revealed.is_none()));
            })
            .unwrap();
    }
    #[gpui::test]
    fn replacement_connection_fences_old_reveals_and_clears_plaintext(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let (handle, mut old) = connection();
        let (state, window) = setup(cx, handle.clone());
        settle(&runtime, cx);
        let get = request(&mut old);
        reply(
            &old,
            get.id,
            Some(serde_json::to_value(metadata("one")).unwrap()),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| editor.reveal(editor.rows[0].id, cx))
            .unwrap();
        settle(&runtime, cx);
        let pending = request(&mut old);
        let (replacement, mut fresh) = connection();
        state.update(cx, |state, cx| {
            state.set_test_engine(replacement.clone().with_test_capabilities(vec![]));
            cx.notify();
        });
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, _| {
                assert!(
                    editor.metadata.is_none(),
                    "connection replacement discards the old revision"
                );
            })
            .unwrap();
        assert!(
            fresh.0.try_recv().is_err(),
            "wait for capability negotiation"
        );
        state.update(cx, |state, cx| {
            state.set_test_engine(replacement);
            cx.notify();
        });
        settle(&runtime, cx);
        reply(
            &old,
            pending.id,
            Some(serde_json::json!({"revision":"one","value":"stale-secret"})),
            None,
        );
        let get = request(&mut fresh);
        reply(
            &fresh,
            get.id,
            Some(serde_json::to_value(metadata("new-device")).unwrap()),
            None,
        );
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, cx| {
                assert_eq!(editor.metadata.as_ref().unwrap().revision, "new-device");
                assert!(editor.rows.iter().all(|row| row.revealed.is_none()));
                assert!(!editor.current(0, &handle, cx));
            })
            .unwrap();
    }
}
