//! Ephemeral, device-addressed environment editor. Values never enter UI settings.
use crate::{
    popover,
    settings::widgets::{self, ActionTone},
    state::{AppState, EngineHandle},
    theme::Theme,
};
use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, KeyDownEvent, Render, Subscription, Task,
    Window, div, prelude::*, px,
};
use gpui_base::input::{Input, InputEvent, InputState};
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
    name_subscription: Option<Subscription>,
    value: Option<Entity<InputState>>,
    value_subscription: Option<Subscription>,
    action: DraftAction,
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
        self.value
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy)]
enum Action {
    Reload,
    Save,
    Cancel,
    Add,
    Edit,
    Unset,
    Delete,
    Undo,
    Reveal,
    Hide,
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
    editing: Option<u64>,
    row_menu: popover::Popup<u64>,
    page_menu: popover::Popup<()>,
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
            editing: None,
            row_menu: popover::Popup::default(),
            page_menu: popover::Popup::default(),
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
            self.editing = None;
        } else {
            self.rows.retain(|row| row.action != DraftAction::Keep);
        }
        for row in &mut self.rows {
            row.revealed = None;
            if let Some(original) = &mut row.original
                && let Some(current) = metadata
                    .entries
                    .iter()
                    .find(|entry| entry.name == original.name)
            {
                // A conflict/reload gives drafts a new undo baseline. The
                // draft inputs and action stay local, but undo must restore
                // the metadata that is current after the refresh.
                *original = current.clone();
            }
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
                name_subscription: None,
                value: None,
                value_subscription: None,
                action: DraftAction::Keep,
                revealed: None,
                error: None,
            });
        }
        self.metadata = Some(metadata);
    }

    fn invalidate_catalog(&self, engine: &EngineHandle, cx: &mut Context<Self>) {
        let target = self
            .target
            .clone()
            .unwrap_or_else(|| engine.engine_info().device_id.clone());
        crate::pickers::bump_harness_environment(target, self.harness, cx);
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
                        if editor
                            .metadata
                            .as_ref()
                            .is_some_and(|previous| previous.revision != metadata.revision)
                        {
                            editor.invalidate_catalog(&engine, cx);
                        }
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
                    EnvironmentChange::Set { name, value }
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
            self.editing = self
                .rows
                .iter()
                .find(|row| row.error.is_some())
                .map(|row| row.id);
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
                        let revision_changed = editor.metadata.as_ref()
                            .is_some_and(|previous| previous.revision != reply.metadata.revision);
                        if !reply.conflict || revision_changed {
                            editor.invalidate_catalog(&engine, cx);
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

    fn dirty(&self) -> bool {
        self.rows.iter().any(|row| row.action != DraftAction::Keep)
    }

    fn close_row_menu(&mut self, cx: &mut Context<Self>) {
        if self.row_menu.begin_close() {
            popover::reap_popup(cx, |editor: &mut Self| &mut editor.row_menu);
        }
        cx.notify();
    }

    fn close_page_menu(&mut self, cx: &mut Context<Self>) {
        if self.page_menu.begin_close() {
            popover::reap_popup(cx, |editor: &mut Self| &mut editor.page_menu);
        }
        cx.notify();
    }

    fn mutation_disabled(&self) -> bool {
        self.loading || self.saving || self.uncertain
    }

    fn subscribe_input(&mut self, id: u64, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let subscription = cx.subscribe(input, move |editor, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                editor.input_changed(id, &input, cx);
            }
        });
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            if row.name.as_ref().is_some_and(|active| active == input) {
                row.name_subscription = Some(subscription);
            } else if row.value.as_ref().is_some_and(|active| active == input) {
                row.value_subscription = Some(subscription);
            }
        }
    }

    fn input_changed(&mut self, id: u64, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        if !row.name.as_ref().is_some_and(|active| active == input)
            && !row.value.as_ref().is_some_and(|active| active == input)
        {
            return;
        }
        if row.original.is_some() && row.action != DraftAction::Set {
            row.action = DraftAction::Set;
            row.revealed = None;
            row.error = None;
        }
        cx.notify();
    }

    fn undo(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return;
        };
        let name = self.rows[index].name(cx);
        let current = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.entries.iter().find(|entry| entry.name == name))
            .cloned();
        if let Some(entry) = current {
            let row = &mut self.rows[index];
            row.original = Some(entry.clone());
            row.name = None;
            row.name_subscription = None;
            row.value = None;
            row.value_subscription = None;
            row.action = DraftAction::Keep;
            row.revealed = None;
            row.error = None;
        } else {
            // Delete drafts and brand-new rows have no current metadata entry
            // to restore. Dropping the draft is the only lossless undo.
            self.rows.remove(index);
        }
        if self.editing == Some(id) {
            self.editing = None;
        }
    }

    fn begin_edit(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing == Some(id) {
            // Closing the local form never changes the draft representation.
            self.editing = None;
            return;
        }
        let needs_replacement = self
            .rows
            .iter()
            .find(|row| row.id == id)
            .is_some_and(|row| row.action != DraftAction::Set);
        if needs_replacement {
            let value = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Replacement value")
                    .masked(true)
            });
            if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                row.value_subscription = None;
                row.value = Some(value);
                row.revealed = None;
                row.error = None;
            }
            if let Some(input) = self
                .rows
                .iter()
                .find(|row| row.id == id)
                .and_then(|row| row.value.as_ref())
                .cloned()
            {
                self.subscribe_input(id, &input, cx);
            }
        }
        self.editing = Some(id);
    }

    fn perform(&mut self, action: Action, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || self.saving {
            return;
        }
        self.close_row_menu(cx);
        self.close_page_menu(cx);
        if self.uncertain && !matches!(action, Action::Reload | Action::Cancel) {
            return;
        }
        match action {
            Action::Reload => self.load(true, cx),
            Action::Save => self.save(cx),
            Action::Cancel => {
                self.generation = self.generation.wrapping_add(1);
                self.reveal_task = None;
                self.editing = None;
                self.error = None;
                if self.uncertain {
                    self.load(false, cx);
                } else if let Some(metadata) = self.metadata.clone() {
                    self.install_metadata(metadata, false);
                }
            }
            Action::Edit => self.begin_edit(id, window, cx),
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
                let name = cx.new(|cx| InputState::new(window, cx).placeholder("VARIABLE_NAME"));
                let value = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Literal value (empty is allowed)")
                        .masked(true)
                });
                self.rows.push(Row {
                    id,
                    original: None,
                    name: Some(name.clone()),
                    name_subscription: None,
                    value: Some(value.clone()),
                    value_subscription: None,
                    action: DraftAction::Set,
                    revealed: None,
                    error: None,
                });
                self.subscribe_input(id, &name, cx);
                self.subscribe_input(id, &value, cx);
                self.editing = Some(id);
            }
            Action::Unset => {
                if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    if row.original.is_none() {
                        self.rows.retain(|row| row.id != id);
                    } else {
                        row.value_subscription = None;
                        row.action = DraftAction::Unset;
                        row.value = None;
                        row.revealed = None;
                        self.editing = None;
                    }
                }
            }
            Action::Delete => {
                if self
                    .rows
                    .iter()
                    .find(|row| row.id == id)
                    .is_some_and(|row| row.original.is_none())
                {
                    self.rows.retain(|row| row.id != id);
                } else if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
                    row.value_subscription = None;
                    row.action = DraftAction::Delete;
                    row.value = None;
                    row.revealed = None;
                    self.editing = None;
                }
            }
            Action::Undo => self.undo(id, cx),
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
        tone: ActionTone,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut button = widgets::action_button(theme, tone)
            .id((key, id))
            .role(gpui::Role::Button)
            .aria_label(label)
            .tab_index(if disabled { -1 } else { 0 })
            .when(disabled, |button| button.opacity(0.42))
            .child(label);
        if !disabled {
            button =
                button
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.perform(action, id, window, cx)
                    }))
                    .on_key_down(
                        cx.listener(move |editor, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                editor.close_row_menu(cx);
                                editor.close_page_menu(cx);
                                cx.stop_propagation();
                            } else if !event.is_held
                                && matches!(event.keystroke.key.as_str(), "enter" | "space")
                            {
                                editor.perform(action, id, window, cx);
                                cx.stop_propagation();
                            }
                        }),
                    );
        }
        button
    }

    fn menu_item(
        &self,
        theme: &Theme,
        key: &'static str,
        label: &'static str,
        action: Action,
        id: u64,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut item = popover::menu_row(theme, false, key)
            .id((key, id))
            .role(gpui::Role::MenuItem)
            .aria_label(label)
            .tab_index(if disabled { -1 } else { 0 })
            .when(disabled, |item| item.opacity(0.42))
            .child(label);
        if !disabled {
            item = item
                .on_click(cx.listener(move |editor, _, window, cx| {
                    editor.perform(action, id, window, cx);
                    cx.stop_propagation();
                }))
                .on_key_down(
                    cx.listener(move |editor, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            editor.close_row_menu(cx);
                            editor.close_page_menu(cx);
                            cx.stop_propagation();
                        } else if !event.is_held
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            editor.perform(action, id, window, cx);
                            cx.stop_propagation();
                        }
                    }),
                );
        }
        item
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
        }
        let theme = Theme::of(cx).for_settings_surface();
        let mutation_disabled = self.mutation_disabled();
        let mut content = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    let mut closed = false;
                    if editor.row_menu.is_open() {
                        editor.close_row_menu(cx);
                        closed = true;
                    }
                    if editor.page_menu.is_open() {
                        editor.close_page_menu(cx);
                        closed = true;
                    }
                    if closed {
                        cx.stop_propagation();
                    }
                }
            }));
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

        let page_menu_open = self.page_menu.get().is_some();
        let mut reload = widgets::action_button(&theme, ActionTone::Quiet)
            .id("environment-actions")
            .w(px(32.0))
            .px_0()
            .justify_center()
            .when(page_menu_open, |button| button.bg(theme.glass_hover()))
            .when(readonly, |button| button.opacity(0.42))
            .role(gpui::Role::Button)
            .aria_label("Environment actions")
            .aria_expanded(page_menu_open)
            .tab_index(if readonly { -1 } else { 0 })
            .tooltip(widgets::text_tooltip("Environment actions"))
            .child(
                crate::icons::icon(crate::icons::MORE_HORIZONTAL)
                    .size(px(16.0))
                    .text_color(theme.text_muted),
            );
        if !readonly {
            reload = reload
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|editor, _, _, _| editor.page_menu.note_trigger_press()),
                )
                .on_click(cx.listener(|editor, _, window, cx| {
                    cx.stop_propagation();
                    if editor.page_menu.take_press_was_open() {
                        editor.close_page_menu(cx);
                    } else {
                        editor.close_row_menu(cx);
                        editor.page_menu.open(());
                        cx.notify();
                    }
                    let _ = window;
                }))
                .on_key_down(cx.listener(|editor, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        editor.close_page_menu(cx);
                        cx.stop_propagation();
                    } else if !event.is_held
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        if editor.page_menu.is_open() {
                            editor.close_page_menu(cx);
                        } else {
                            editor.close_row_menu(cx);
                            editor.page_menu.open(());
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }
                }));
        }
        if page_menu_open {
            let popup = Theme::of(cx).for_popup();
            let menu = popover::popover_card(&popup)
                .w(px(190.0))
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|editor, _, _, cx| editor.close_page_menu(cx)))
                .child(self.menu_item(
                    &popup,
                    "environment-reload",
                    "Reload status",
                    Action::Reload,
                    0,
                    readonly,
                    cx,
                ))
                .into_any_element();
            reload = reload.relative().child(popover::anchored_menu_below_end(
                "environment-actions-menu",
                menu,
                self.page_menu.closing_since(),
            ));
        }

        let header = div()
            .flex()
            .flex_row()
            .items_start()
            .justify_between()
            .gap(px(16.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(widgets::section_label(&theme, "Environment variables"))
                    .child(
                        div()
                            .px(px(8.0))
                            .text_size(crate::typography::ui_rems(13.0))
                            .text_color(theme.text_muted)
                            .child("Applies to new sessions."),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .id("environment-precedence")
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .tooltip(widgets::text_tooltip(
                                "Credential overrides may take precedence over saved provider accounts.",
                            ))
                            .child(
                                crate::icons::icon(crate::icons::INFO_CIRCLE)
                                    .size(px(15.0))
                                    .text_color(theme.text_muted),
                            ),
                    )
                    .child(self.button(
                        &theme,
                        "environment-add",
                        "Add variable",
                        Action::Add,
                        0,
                        ActionTone::Outlined,
                        mutation_disabled,
                        cx,
                    ))
                    .child(reload),
            );
        content = content.child(header);
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
            content = content.child(
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child("Loading environment settings…"),
            );
        }

        let mut rows_card = widgets::section_card(&theme).mt_0();
        if self.rows.is_empty() && !self.loading {
            rows_card = rows_card.child(
                div()
                    .mx(px(16.0))
                    .py(px(16.0))
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child("No environment overrides."),
            );
        }
        for (index, row) in self.rows.iter().enumerate() {
            let id = row.id;
            let editing = self.editing == Some(id);
            let name = row.name(cx);
            let display_name = if name.is_empty() {
                "New variable".to_owned()
            } else {
                name.clone()
            };
            let set = row
                .original
                .as_ref()
                .is_some_and(|entry| entry.action == EnvironmentAction::Set);
            let summary = match row.action {
                DraftAction::Keep => {
                    if set {
                        row.revealed
                            .clone()
                            .unwrap_or_else(|| "Set · ••••••••".to_owned())
                    } else {
                        "Unset".to_owned()
                    }
                }
                DraftAction::Set => "Draft value · ••••••••".to_owned(),
                DraftAction::Unset => "Unset · draft".to_owned(),
                DraftAction::Delete => "Restore inheritance · draft".to_owned(),
            };

            let name_summary = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(crate::typography::ui_rems(13.0))
                        .line_height(crate::typography::ui_rems(17.0))
                        .font_family(theme.font_mono.clone())
                        .text_color(theme.text)
                        .child(display_name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .max_w(px(190.0))
                        .min_w_0()
                        .truncate()
                        .text_size(crate::typography::ui_rems(12.0))
                        .text_color(if row.action == DraftAction::Keep {
                            theme.text_muted
                        } else {
                            theme.warning_muted
                        })
                        .child(summary),
                );

            let edit_disabled = mutation_disabled;
            let edit = self.button(
                &theme,
                "environment-edit",
                if editing { "Done" } else { "Edit" },
                Action::Edit,
                id,
                ActionTone::Quiet,
                edit_disabled,
                cx,
            );

            let menu_open = self.row_menu.get() == Some(&id);
            let mut more = widgets::action_button(&theme, ActionTone::Quiet)
                .id(("environment-more", id))
                .w(px(28.0))
                .px_0()
                .justify_center()
                .when(menu_open, |button| button.bg(theme.glass_hover()))
                .when(mutation_disabled, |button| button.opacity(0.42))
                .role(gpui::Role::Button)
                .aria_label(format!("Actions for {display_name}"))
                .aria_expanded(menu_open)
                .tab_index(if mutation_disabled { -1 } else { 0 })
                .tooltip(widgets::text_tooltip("Variable actions"))
                .child(
                    crate::icons::icon(crate::icons::MORE_HORIZONTAL)
                        .size(px(16.0))
                        .text_color(theme.text_muted),
                );
            if !mutation_disabled {
                let trigger_id = id;
                more = more
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |editor, _, _, _| {
                            editor
                                .row_menu
                                .note_trigger_press_matching(|open| *open == trigger_id);
                        }),
                    )
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        cx.stop_propagation();
                        if editor.row_menu.take_press_was_open() {
                            editor.close_row_menu(cx);
                        } else {
                            editor.close_page_menu(cx);
                            editor.row_menu.open(trigger_id);
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(move |editor, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            editor.close_row_menu(cx);
                            cx.stop_propagation();
                        } else if !event.is_held
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            if editor.row_menu.is_open()
                                && editor.row_menu.get() == Some(&trigger_id)
                            {
                                editor.close_row_menu(cx);
                            } else {
                                editor.close_page_menu(cx);
                                editor.row_menu.open(trigger_id);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }
                    }));
            }

            if menu_open {
                let popup = Theme::of(cx).for_popup();
                let mut menu = popover::popover_card(&popup)
                    .w(px(222.0))
                    .flex()
                    .flex_col()
                    .on_mouse_down_out(cx.listener(|editor, _, _, cx| editor.close_row_menu(cx)));
                match row.action {
                    DraftAction::Keep => {
                        if set {
                            menu = menu.child(self.menu_item(
                                &popup,
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
                                mutation_disabled,
                                cx,
                            ));
                            menu = menu.child(self.menu_item(
                                &popup,
                                "environment-unset",
                                "Unset in child",
                                Action::Unset,
                                id,
                                mutation_disabled,
                                cx,
                            ));
                        } else {
                            menu = menu.child(self.menu_item(
                                &popup,
                                "environment-set",
                                "Set value",
                                Action::Edit,
                                id,
                                mutation_disabled,
                                cx,
                            ));
                        }
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-delete",
                            "Restore inheritance",
                            Action::Delete,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                    }
                    DraftAction::Set => {
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-undo",
                            "Undo changes",
                            Action::Undo,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-unset",
                            "Unset in child",
                            Action::Unset,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-delete",
                            if row.original.is_some() {
                                "Restore inheritance"
                            } else {
                                "Remove draft"
                            },
                            Action::Delete,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                    }
                    DraftAction::Unset | DraftAction::Delete => {
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-set",
                            "Set value",
                            Action::Edit,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                        menu = menu.child(self.menu_item(
                            &popup,
                            "environment-undo",
                            "Undo changes",
                            Action::Undo,
                            id,
                            mutation_disabled,
                            cx,
                        ));
                    }
                }
                more = more.relative().child(popover::anchored_menu_below_end(
                    format!("environment-row-menu-{id}"),
                    menu.into_any_element(),
                    self.row_menu.closing_since(),
                ));
            }

            let top = div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(name_summary)
                .child(edit)
                .child(more);
            let mut row_element = widgets::card_row(&theme, index == 0)
                .id(("environment-row", id))
                .min_h(px(44.0))
                .py(px(6.0))
                .flex_col()
                .items_stretch()
                .gap(px(8.0));
            if editing {
                let input_shell = |field: AnyElement, focus: gpui::FocusHandle| {
                    div()
                        .flex()
                        .w_full()
                        .min_h(px(32.0))
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.input_glass_bg())
                        .track_focus(&focus)
                        .focus_visible(|style| style.border_1().border_color(theme.accent))
                        .child(field)
                        .into_any_element()
                };
                let name_field: AnyElement = match &row.name {
                    Some(input) => input_shell(
                        Input::new(input).into_any_element(),
                        input.read(cx).focus_handle(cx),
                    ),
                    None => div()
                        .text_sm()
                        .text_color(theme.text)
                        .child(display_name.clone())
                        .into_any_element(),
                };
                let value_field: AnyElement = match &row.value {
                    Some(input) => input_shell(
                        Input::new(input).into_any_element(),
                        input.read(cx).focus_handle(cx),
                    ),
                    None => div().into_any_element(),
                };
                let form = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .border_t_1()
                    .border_color(widgets::row_divider(&theme))
                    .pt(px(10.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(widgets::field_label(&theme, "Name"))
                            .child(name_field),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(widgets::field_label(&theme, "Value"))
                            .child(value_field),
                    );
                row_element = row_element.child(top).child(form);
            } else {
                row_element = row_element.child(top);
            }
            if let Some(error) = &row.error {
                row_element = row_element.child(
                    div()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(error.clone()),
                );
            }
            rows_card = rows_card.child(row_element);
        }
        content = content.child(rows_card);
        if self.dirty() {
            let save_disabled =
                readonly || self.uncertain || self.metadata.is_none() || !self.dirty();
            let discard_disabled = readonly || !self.dirty();
            content = content.child(
                div()
                    .mt(px(2.0))
                    .pt(px(10.0))
                    .border_t_1()
                    .border_color(widgets::row_divider(&theme))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .child(if self.saving {
                                "Saving changes…"
                            } else if self.uncertain {
                                "Reload before saving again."
                            } else {
                                "Unsaved changes"
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.0))
                            .child(self.button(
                                &theme,
                                "environment-discard",
                                "Discard",
                                Action::Cancel,
                                0,
                                ActionTone::Quiet,
                                discard_disabled,
                                cx,
                            ))
                            .child(self.button(
                                &theme,
                                "environment-save",
                                if self.saving { "Saving…" } else { "Save" },
                                Action::Save,
                                0,
                                ActionTone::Solid,
                                save_disabled,
                                cx,
                            )),
                    ),
            );
        }
        content.into_any_element()
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
        let input = window
            .update(cx, |editor, window, cx| {
                assert_eq!(editor.rows[0].revealed.as_deref(), Some("private-revealed"));
                editor.perform(Action::Hide, editor.rows[0].id, window, cx);
                assert!(editor.rows[0].revealed.is_none());
                editor.perform(Action::Edit, editor.rows[0].id, window, cx);
                assert!(
                    editor.rows[0]
                        .value
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .presentation()
                        .is_masked()
                );
                editor.rows[0].value.clone().unwrap()
            })
            .unwrap();
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| {
                    input.replace_all("  中文 $HOME `literal`  ", window, cx)
                });
            })
            .unwrap();
        settle(&runtime, cx);
        window.update(cx, |editor, _, cx| editor.save(cx)).unwrap();
        settle(&runtime, cx);
        let save = request(&mut wire);
        assert_eq!(
            save.params["changes"][0]["value"],
            "  中文 $HOME `literal`  "
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
                assert_eq!(editor.rows[0].value(cx), "  中文 $HOME `literal`  ");
                assert_eq!(editor.metadata.as_ref().unwrap().revision, "two");
                assert_eq!(
                    cx.global::<crate::pickers::HarnessEnvironmentChanged>()
                        .generations
                        .get(&("local".into(), HarnessId::Grok)),
                    Some(&1),
                    "conflicts refresh catalogs to the observed committed revision"
                );
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
                assert_eq!(
                    cx.global::<crate::pickers::HarnessEnvironmentChanged>()
                        .generations
                        .get(&("local".into(), HarnessId::Grok)),
                    Some(&2),
                    "lost-ack recovery refreshes catalogs without replaying the save"
                );
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

    #[gpui::test]
    fn edit_done_does_not_stage_an_empty_replacement(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let (handle, mut wire) = connection();
        let (_state, window) = setup(cx, handle);
        settle(&runtime, cx);
        let get = request(&mut wire);
        reply(
            &wire,
            get.id,
            Some(serde_json::to_value(metadata("one")).unwrap()),
            None,
        );
        settle(&runtime, cx);

        window
            .update(cx, |editor, window, cx| {
                let id = editor.rows[0].id;
                editor.begin_edit(id, window, cx);
                assert!(matches!(editor.rows[0].action, DraftAction::Keep));
                assert!(!editor.dirty());
                editor.begin_edit(id, window, cx);
                assert!(matches!(editor.rows[0].action, DraftAction::Keep));
                assert!(!editor.dirty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn input_change_stages_replacement_for_save(cx: &mut TestAppContext) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let (handle, mut wire) = connection();
        let (_state, window) = setup(cx, handle);
        settle(&runtime, cx);
        let get = request(&mut wire);
        reply(
            &wire,
            get.id,
            Some(serde_json::to_value(metadata("one")).unwrap()),
            None,
        );
        settle(&runtime, cx);

        let input = window
            .update(cx, |editor, window, cx| {
                let id = editor.rows[0].id;
                editor.begin_edit(id, window, cx);
                editor.rows[0].value.clone().unwrap()
            })
            .unwrap();
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| {
                    input.replace_all("replacement-value", window, cx)
                });
            })
            .unwrap();
        settle(&runtime, cx);
        window
            .update(cx, |editor, _, _| {
                assert!(matches!(editor.rows[0].action, DraftAction::Set));
            })
            .unwrap();
        window.update(cx, |editor, _, cx| editor.save(cx)).unwrap();
        settle(&runtime, cx);
        let save = request(&mut wire);
        assert_eq!(
            save.method.as_deref(),
            Some(methods::PATCH_HARNESS_ENVIRONMENT)
        );
        assert_eq!(save.params["changes"][0]["name"], "API_KEY");
        assert_eq!(save.params["changes"][0]["value"], "replacement-value");
    }
}
