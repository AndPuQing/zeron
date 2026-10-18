//! Native import UI. Discovery and persistence are supplied by a controller;
//! this view never reads agent stores or starts a model request.
mod model;
mod rpc;
#[cfg(test)]
mod tests;

pub use model::{
    ExternalSession, ImportDevice, ImportEligibility, ImportItemState, ImportOutcome,
    SessionDiscovery, SessionImportEvent, SessionProvider,
};

use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    KeyDownEvent, Render, ScrollHandle, SharedString, Subscription, Window, div, prelude::*, px,
};

use crate::composer::{ComposerInput, ComposerInputEvent};
use crate::icons::{self, icon};
use crate::popover;
use crate::theme::Theme;
use model::ImportModel;

/// A backend can answer requests asynchronously using the dialog's receive_*
/// methods. Only devices advertising import support should be supplied.
pub type SessionImportHandler =
    Rc<dyn Fn(Entity<SessionImportDialog>, SessionImportEvent, &mut App)>;

pub struct SessionImportController {
    pub devices: Vec<ImportDevice>,
    pub handle: SessionImportHandler,
}

/// The agent filter lists every provider the importer can read; a menu keeps
/// the toolbar steady as harnesses are added.
const AGENT_FILTERS: [Option<SessionProvider>; 3] = [
    None,
    Some(SessionProvider::Codex),
    Some(SessionProvider::ClaudeCode),
];

/// Which filter menu owns the shared popup — at most one is open at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilterMenu {
    Agent,
}

pub struct SessionImportDialog {
    rpc: rpc::Requests,
    model: ImportModel,
    search: Entity<ComposerInput>,
    focus: FocusHandle,
    end_focus: FocusHandle,
    focus_pending: bool,
    focus_dialog_pending: bool,
    list_scroll: ScrollHandle,
    device_menu: popover::Popup<()>,
    device_menu_active: usize,
    filter_menu: popover::Popup<FilterMenu>,
    filter_menu_active: usize,
    _search_events: Subscription,
}

impl EventEmitter<SessionImportEvent> for SessionImportDialog {}

impl Focusable for SessionImportDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SessionImportDialog {
    pub fn new(devices: Vec<ImportDevice>, device_id: &str, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| ComposerInput::with_context("Search sessions…", "PaletteSearch", cx));
        let events = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, ComposerInputEvent::Edited) {
                this.model.active = None;
                this.list_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
                cx.notify();
            }
        });
        Self {
            rpc: rpc::Requests::default(),
            model: ImportModel::new(devices, device_id),
            search,
            focus: cx.focus_handle(),
            end_focus: cx.focus_handle(),
            focus_pending: true,
            focus_dialog_pending: false,
            list_scroll: ScrollHandle::new(),
            device_menu: popover::Popup::default(),
            device_menu_active: 0,
            filter_menu: popover::Popup::default(),
            filter_menu_active: 0,
            _search_events: events,
        }
    }

    pub fn discover(&mut self, cx: &mut Context<Self>) {
        self.load_sessions(false, cx);
    }

    pub fn receive_discovery(
        &mut self,
        request_id: u64,
        result: Result<SessionDiscovery, String>,
        cx: &mut Context<Self>,
    ) {
        if self.model.receive_discovery(request_id, result) {
            cx.notify();
        }
    }

    pub fn update_import_item(
        &mut self,
        request_id: u64,
        source_ref: &str,
        state: ImportItemState,
        cx: &mut Context<Self>,
    ) {
        if self.model.update_item(request_id, source_ref, state) {
            cx.notify();
        }
    }

    pub fn finish_import(&mut self, request_id: u64, cancelled: bool, cx: &mut Context<Self>) {
        if self.model.finish_import(request_id, cancelled) {
            self.focus_dialog_pending = true;
            cx.notify();
        }
    }

    pub fn is_importing(&self) -> bool {
        self.model.batch.as_ref().is_some_and(|batch| batch.running)
    }

    fn load_sessions(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.is_importing() {
            return;
        }
        if !more {
            self.focus_pending = true;
        }
        if let Some(event) = self.model.discover(more) {
            cx.emit(event);
            cx.notify();
        }
    }

    fn choose_device(&mut self, index: usize, cx: &mut Context<Self>) {
        self.close_device_menu(cx);
        self.close_filter_menu(cx);
        if index == self.model.device_index || self.is_importing() {
            cx.notify();
            return;
        }
        self.model.device_index = index;
        self.model.current_project_only = self.model.devices[index].current_project.is_some();
        self.model.batch = None;
        self.load_sessions(false, cx);
    }

    fn toggle_session(&mut self, source_ref: String, cx: &mut Context<Self>) {
        self.model.active = Some(source_ref.clone());
        self.model.toggle_selection(&source_ref);
        cx.notify();
    }

    fn start_import(&mut self, retry: bool, cx: &mut Context<Self>) {
        self.close_device_menu(cx);
        self.close_filter_menu(cx);
        if let Some(event) = self.model.begin_import(retry) {
            // The submit/retry control unmounts in the next render. Transfer
            // its focus before the old handle leaves the dispatch tree.
            self.focus_pending = false;
            self.focus_dialog_pending = true;
            cx.emit(event);
            cx.notify();
        }
    }

    fn close_or_cancel(&mut self, cx: &mut Context<Self>) {
        if self.is_importing() {
            if let Some(event) = self.model.cancel() {
                cx.emit(event);
                cx.notify();
            }
        } else {
            cx.emit(SessionImportEvent::Close);
        }
    }

    fn choose_more(&mut self, cx: &mut Context<Self>) {
        if self.is_importing() {
            return;
        }
        self.close_filter_menu(cx);
        self.model.batch = None;
        self.model.selected.clear();
        self.focus_pending = true;
        cx.notify();
    }

    fn close_device_menu(&mut self, cx: &mut Context<Self>) {
        if self.device_menu.begin_close() {
            popover::reap_popup(cx, |dialog| &mut dialog.device_menu);
            cx.notify();
        }
    }

    fn close_filter_menu(&mut self, cx: &mut Context<Self>) {
        if self.filter_menu.begin_close() {
            popover::reap_popup(cx, |dialog| &mut dialog.filter_menu);
            cx.notify();
        }
    }

    fn choose_filter(&mut self, kind: FilterMenu, index: usize, cx: &mut Context<Self>) {
        self.close_filter_menu(cx);
        match kind {
            FilterMenu::Agent if index < AGENT_FILTERS.len() => {
                self.model.provider = AGENT_FILTERS[index];
            }
            _ => return,
        }
        self.model.active = None;
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(kind) = self.filter_menu.as_open().copied() {
            let count = match kind {
                FilterMenu::Agent => AGENT_FILTERS.len(),
            };
            match key {
                "escape" | "tab" => self.close_filter_menu(cx),
                "up" | "down" => {
                    self.filter_menu_active = popover::menu_step(
                        Some(self.filter_menu_active),
                        count,
                        if key == "down" { 1 } else { -1 },
                    )
                    .unwrap_or(0)
                }
                "enter" => {
                    let index = self.filter_menu_active;
                    self.choose_filter(kind, index, cx);
                }
                _ => return,
            }
        } else if self.device_menu.is_open() {
            match key {
                "escape" | "tab" => self.close_device_menu(cx),
                "up" | "down" => {
                    self.device_menu_active = popover::menu_step(
                        Some(self.device_menu_active),
                        self.model.devices.len(),
                        if key == "down" { 1 } else { -1 },
                    )
                    .unwrap_or(0)
                }
                "enter" => self.choose_device(self.device_menu_active, cx),
                _ => return,
            }
        } else {
            match key {
                "escape" => self.close_or_cancel(cx),
                "tab" => {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    if !self.focus.contains_focused(window, cx) {
                        if event.keystroke.modifiers.shift {
                            window.focus(&self.end_focus, cx);
                            window.focus_prev(cx);
                        } else {
                            window.focus(&self.focus, cx);
                            window.focus_next(cx);
                        }
                        if !self.focus.contains_focused(window, cx) {
                            window.focus(&self.focus, cx);
                        }
                    }
                }
                "enter"
                    if event.keystroke.modifiers.platform || event.keystroke.modifiers.control =>
                {
                    self.start_import(false, cx)
                }
                "up" | "down" => {
                    let rows = if let Some(batch) = &self.model.batch {
                        batch.rows.iter().collect()
                    } else {
                        self.model.visible_sessions(self.search.read(cx).text())
                    };
                    let active = rows
                        .iter()
                        .position(|row| Some(&row.source_ref) == self.model.active.as_ref());
                    if let Some(index) =
                        popover::menu_step(active, rows.len(), if key == "down" { 1 } else { -1 })
                    {
                        let id = rows[index].source_ref.clone();
                        let headers = rows[..=index]
                            .iter()
                            .map(|row| row.cwd.as_str())
                            .collect::<std::collections::HashSet<_>>()
                            .len();
                        let notices = if self.model.batch.is_none() {
                            self.model.source_errors.len()
                                + usize::from(self.model.discovery.error().is_some())
                        } else {
                            0
                        };
                        self.list_scroll.scroll_to_item(index + headers + notices);
                        self.model.active = Some(id);
                    }
                }
                "enter"
                    if self.focus.is_focused(window)
                        || self.search.focus_handle(cx).is_focused(window) =>
                {
                    if !event.is_held {
                        let id = self.model.active.clone().or_else(|| {
                            self.model
                                .visible_sessions(self.search.read(cx).text())
                                .first()
                                .map(|row| row.source_ref.clone())
                        });
                        if let Some(id) = id {
                            self.toggle_session(id, cx);
                        }
                    }
                }
                "space" if self.focus.is_focused(window) => {
                    if !event.is_held
                        && let Some(id) = self.model.active.clone()
                    {
                        self.toggle_session(id, cx);
                    }
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// One filter dropdown: the trigger reports the active choice, the menu
    /// lists every choice with the current one checked. A menu, not a row of
    /// chips, so added harnesses do not crowd the toolbar.
    fn render_filter_picker(
        &self,
        theme: &Theme,
        kind: FilterMenu,
        id: &'static str,
        menu_id: &'static str,
        labels: &[&str],
        selected: usize,
        aria_label: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owns_menu = self.filter_menu.get() == Some(&kind);
        let mut trigger = popover::btn_ghost(theme, labels[selected], id)
            .id(id)
            .debug_selector(move || format!("{id}-trigger"))
            .role(gpui::Role::Button)
            .aria_label(aria_label)
            .aria_expanded(owns_menu && self.filter_menu.is_open())
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(6.0))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.filter_menu
                        .note_trigger_press_matching(|open| *open == kind);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.filter_menu.take_press_was_open() {
                    this.close_filter_menu(cx);
                } else {
                    this.filter_menu.open(kind);
                    this.filter_menu_active = selected;
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
            }))
            .child(
                icon(icons::ALT_ARROW_DOWN)
                    .size(px(12.0))
                    .flex_none()
                    .text_color(theme.text_muted.opacity(0.5)),
            );
        if owns_menu {
            let menu =
                popover::popover_card(theme)
                    .w(px(180.0))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.close_filter_menu(cx);
                    }))
                    .children(labels.iter().enumerate().map(|(index, label)| {
                        popover::menu_row_nav(
                            theme,
                            index == selected,
                            index == self.filter_menu_active,
                            format!("{menu_id}-{index}"),
                        )
                        .id((menu_id, index))
                        .debug_selector(move || format!("{menu_id}-{index}"))
                        .role(gpui::Role::Button)
                        .aria_selected(index == selected)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_filter(kind, index, cx);
                        }))
                        .child(div().flex_none().size(px(14.0)).flex().items_center().when(
                            index == selected,
                            |slot| {
                                slot.child(icon(icons::CHECK).size(px(12.0)).text_color(theme.text))
                            },
                        ))
                        .child(SharedString::from((*label).to_string()))
                    }))
                    .into_any_element();
            trigger = trigger.child(popover::anchored_menu_below_layer(
                menu_id,
                menu,
                self.filter_menu.closing_since(),
                6.0,
                // The dialog is a priority-2 modal layer, so the default
                // priority-1 menu would paint underneath the card.
                3,
            ));
        }
        trigger.into_any_element()
    }

    fn render_filters(&self, theme: &Theme, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let agent_labels =
            AGENT_FILTERS.map(|provider| provider.map_or("All agents", SessionProvider::label));
        let agent_selected = AGENT_FILTERS
            .iter()
            .position(|provider| *provider == self.model.provider)
            .unwrap_or(0);
        let has_project = self
            .model
            .devices
            .get(self.model.device_index)
            .is_some_and(|d| d.current_project.is_some());
        let scope = popover::btn_ghost(
            theme,
            if self.model.current_project_only {
                "This project"
            } else {
                "All projects"
            },
            "import-project-scope",
        )
        .id("import-project-scope")
        .role(gpui::Role::Button)
        .tab_index(0)
        .on_click(cx.listener(|this, _, _, cx| {
            this.model.current_project_only = !this.model.current_project_only;
            this.model.active = None;
            cx.notify();
        }));
        let search = div()
            .flex_1()
            .when(width < 620.0, |search| search.flex_none().w_full())
            .min_w(px(160.0))
            .h(px(32.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(popover::palette_search_icon(theme))
            .child(div().flex_1().min_w_0().child(self.search.clone()));
        let filters = div()
            .flex_none()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.0))
            .when(needs_provider_filter(&self.model), |row| {
                row.child(self.render_filter_picker(
                    theme,
                    FilterMenu::Agent,
                    "import-agent-filter",
                    "import-agent-option",
                    &agent_labels,
                    agent_selected,
                    "Agent filter",
                    cx,
                ))
            })
            .when(has_project, |row| row.child(scope));
        let stacked = width < 620.0;
        div()
            .flex_none()
            .px(px(20.0))
            .py(px(8.0))
            .flex()
            .gap(px(if stacked { 4.0 } else { 12.0 }))
            .when(stacked, |toolbar| toolbar.flex_col())
            .when(!stacked, |toolbar| toolbar.items_center())
            .border_b_1()
            .border_color(theme.border)
            .child(search)
            .child(filters)
            .into_any_element()
    }

    fn render_session_row(
        &self,
        row: &ExternalSession,
        index: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.model.active.as_deref() == Some(&row.source_ref);
        let selected = self.model.selected.contains(&row.source_ref);
        let available = row.eligibility == ImportEligibility::Available
            && (selected
                || self.model.selected.len() < zeron_proto::MAX_EXTERNAL_SESSION_IMPORT_BATCH);
        let batch_state = self
            .model
            .batch
            .as_ref()
            .and_then(|batch| batch.states.get(&row.source_ref));
        let (status, color) = match batch_state {
            Some(ImportItemState::Pending) => (Some("Waiting".to_string()), theme.text_muted),
            Some(ImportItemState::Importing) => (Some("Importing…".to_string()), theme.text),
            Some(ImportItemState::Cancelled) => {
                (Some("Not imported".to_string()), theme.text_muted)
            }
            Some(ImportItemState::Finished(ImportOutcome::Imported { .. })) => {
                (Some("Imported · Archived".to_string()), theme.success)
            }
            Some(ImportItemState::Finished(ImportOutcome::AlreadyManaged { .. })) => {
                (Some("Already in Zerun".to_string()), theme.text_muted)
            }
            Some(ImportItemState::Finished(ImportOutcome::Failed { reason, .. })) => {
                (Some(reason.clone()), theme.danger)
            }
            None => match &row.eligibility {
                // The title badge already says it; a running session explains
                // itself through the row tooltip instead of a second line.
                ImportEligibility::Available
                | ImportEligibility::AlreadyManaged { .. }
                | ImportEligibility::Running => (None, theme.text_muted),
                ImportEligibility::Unavailable { reason } => {
                    (Some(reason.clone()), theme.text_muted)
                }
            },
        };
        let badge = match &row.eligibility {
            ImportEligibility::AlreadyManaged { .. } => {
                Some(crate::settings::widgets::badge_active(theme, "Imported"))
            }
            ImportEligibility::Running => Some(
                crate::settings::widgets::badge(theme, "Running")
                    .bg(theme.warning.opacity(0.12))
                    .text_color(theme.warning_muted),
            ),
            ImportEligibility::Available | ImportEligibility::Unavailable { .. } => None,
        };
        let chat_id = batch_state
            .and_then(ImportItemState::chat_id)
            .map(str::to_owned)
            .or_else(|| {
                if let ImportEligibility::AlreadyManaged { chat_id } = &row.eligibility {
                    Some(chat_id.clone())
                } else {
                    None
                }
            });
        let id = row.source_ref.clone();
        let row_id = id.clone();
        let (brand, tint) = crate::pickers::harness_brand_icon(row.provider.harness());
        let selection = div()
            .id(("import-select", index))
            .debug_selector(move || format!("import-select-{index}"))
            .role(gpui::Role::CheckBox)
            .aria_label(SharedString::from(format!(
                "{} {}",
                if selected { "Deselect" } else { "Select" },
                row.title
            )))
            .aria_toggled(if selected {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
            .tab_index(0)
            .tab_stop(available)
            .flex_none()
            .size(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .when(available, |control| control.cursor_pointer())
            .when(!available, |control| control.opacity(0.4))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_session(id.clone(), cx);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(selection_box(theme, selected));
        let content = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        icon(brand)
                            .size(px(14.0))
                            .flex_none()
                            .text_color(tint.unwrap_or(theme.text_muted)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(crate::typography::ui_rems(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .when(
                                matches!(row.eligibility, ImportEligibility::Unavailable { .. }),
                                |title| title.text_color(theme.text_muted),
                            )
                            .child(row.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .w(px(72.0))
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .children(badge),
                            )
                            .child(
                                div()
                                    .w(px(36.0))
                                    .flex_none()
                                    .text_right()
                                    .text_size(crate::typography::ui_rems(11.0))
                                    .text_color(theme.text_muted)
                                    .child(crate::state::format_time_ago(
                                        row.updated_at,
                                        chrono::Utc::now(),
                                    )),
                            )
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .when_some(
                                        chat_id.filter(|_| !self.is_importing()),
                                        |slot, chat_id| {
                                            slot.child(
                                                popover::btn_ghost(
                                                    theme,
                                                    "Open",
                                                    format!("import-open-{index}"),
                                                )
                                                .id(("import-open", index))
                                                .role(gpui::Role::Button)
                                                .aria_label(SharedString::from(format!(
                                                    "Open {}",
                                                    row.title
                                                )))
                                                .tab_index(0)
                                                .flex_none()
                                                .py(px(2.0))
                                                .px(px(6.0))
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.emit(SessionImportEvent::OpenChat {
                                                        chat_id: chat_id.clone(),
                                                    });
                                                    cx.stop_propagation();
                                                })),
                                            )
                                        },
                                    ),
                            ),
                    ),
            )
            .when_some(status, |content, status| {
                content.child(
                    div()
                        .pl(px(22.0))
                        .text_size(crate::typography::ui_rems(12.0))
                        .text_color(color)
                        .child(status),
                )
            });
        popover::menu_row(theme, active, format!("import-row-{}", row.source_ref))
            .id(("import-row", index))
            .debug_selector(move || format!("import-row-{index}"))
            .role(gpui::Role::ListItem)
            .aria_label(SharedString::from(format!(
                "{} · {}{} · {}",
                row.title,
                row.provider.label(),
                match row.eligibility {
                    ImportEligibility::AlreadyManaged { .. } => " · Imported",
                    ImportEligibility::Running => " · Running",
                    _ => "",
                },
                row.cwd,
            )))
            .aria_selected(selected)
            .min_h(px(46.0))
            .gap(px(6.0))
            .when(row.eligibility == ImportEligibility::Running, |item| {
                item.tooltip(crate::settings::widgets::text_tooltip(
                    "Finish or stop its current turn, then scan again to import.",
                ))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.toggle_session(row_id.clone(), cx);
            }))
            .when(self.model.batch.is_none(), |item| item.child(selection))
            .when(self.model.batch.is_some(), |item| {
                item.child(div().flex_none().w(px(28.0)))
            })
            .child(content)
            .into_any_element()
    }

    fn render_list(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = if let Some(batch) = &self.model.batch {
            batch.rows.iter().collect()
        } else {
            self.model.visible_sessions(self.search.read(cx).text())
        };
        let mut list = div()
            .id("import-session-list")
            .debug_selector(|| "import-session-list".into())
            .role(gpui::Role::List)
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .p(px(8.0))
            .flex()
            .flex_col()
            .gap(px(3.0));
        if self.model.batch.is_none() {
            if let Some(error) = self.model.discovery.error() {
                list =
                    list.child(self.render_notice(0, "Couldn't load sessions", error, theme, cx));
            }
            for (index, error) in self.model.source_errors.iter().enumerate() {
                list = list.child(self.render_notice(
                    index + 1,
                    "Source unavailable",
                    error,
                    theme,
                    cx,
                ));
            }
        }
        let mut project = String::new();
        for (index, row) in rows.iter().enumerate() {
            if row.cwd != project {
                project = row.cwd.clone();
                let group_refs = rows
                    .iter()
                    .filter(|candidate| {
                        candidate.cwd == row.cwd
                            && candidate.eligibility == ImportEligibility::Available
                    })
                    .map(|row| row.source_ref.clone())
                    .collect::<Vec<_>>();
                let all_selected = !group_refs.is_empty()
                    && group_refs.iter().all(|id| self.model.selected.contains(id));
                let selectable = self.model.batch.is_none() && !group_refs.is_empty();
                list = list.child(
                    div()
                        .id(("import-group", index))
                        .debug_selector(move || format!("import-group-{index}"))
                        .role(gpui::Role::CheckBox)
                        .aria_label(SharedString::from(format!(
                            "{} sessions in {}",
                            if all_selected { "Deselect" } else { "Select" },
                            row.project
                        )))
                        .aria_toggled(if all_selected {
                            gpui::Toggled::True
                        } else {
                            gpui::Toggled::False
                        })
                        .tab_index(0)
                        .tab_stop(selectable)
                        .mt(px(if index == 0 { 0.0 } else { 8.0 }))
                        .px(px(8.0))
                        .py(px(5.0))
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(crate::typography::ui_rems(12.0))
                        .text_color(theme.text_muted)
                        .tooltip(crate::settings::widgets::text_tooltip(row.cwd.clone()))
                        .when(selectable, |header| {
                            header
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    for id in &group_refs {
                                        if all_selected {
                                            this.model.selected.remove(id);
                                        } else if !this.model.selected.contains(id) {
                                            this.model.toggle_selection(id);
                                        }
                                    }
                                    cx.notify();
                                }))
                        })
                        // The slot stays reserved so project names line up with
                        // the session rows and with each other.
                        .child(
                            div()
                                .flex_none()
                                .size(px(28.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(selectable, |slot| {
                                    slot.child(selection_box(theme, all_selected))
                                }),
                        )
                        .child(
                            icon(icons::FOLDER)
                                .size(px(13.0))
                                .text_color(theme.text_muted),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(row.project.clone()),
                        ),
                );
            }
            list = list.child(self.render_session_row(row, index, theme, cx));
        }
        if rows.is_empty() && self.model.batch.is_none() {
            let loading = self.model.discovery.is_loading();
            let failed =
                self.model.discovery.error().is_some() || !self.model.source_errors.is_empty();
            let filtered = self.model.provider.is_some()
                || self.model.current_project_only
                || !self.search.read(cx).text().trim().is_empty();
            list = list.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .p(px(24.0))
                    .when(loading, |state| {
                        state.child(crate::loaders::zeron_loader(
                            "import-scan",
                            theme,
                            6.0,
                            cx.entity_id(),
                            cx,
                        ))
                    })
                    .child(if loading {
                        "Looking for saved sessions…"
                    } else if failed {
                        "No sessions available"
                    } else if filtered {
                        "No matching sessions"
                    } else {
                        "No unimported sessions found"
                    })
                    .child(
                        div()
                            .text_size(crate::typography::ui_rems(12.0))
                            .text_color(theme.text_muted)
                            .child(if filtered {
                                "Try another agent, project, or search."
                            } else {
                                if self.model.next_cursor.is_some() {
                                    "Load more to keep looking for unimported sessions."
                                } else {
                                    "Imported sessions are available in Archived."
                                }
                            }),
                    ),
            );
        }
        if self.model.batch.is_none() && self.model.next_cursor.is_some() {
            list = list.child(
                popover::btn_ghost(
                    theme,
                    if self.model.discovery.is_loading() {
                        "Loading…"
                    } else {
                        "Load more sessions"
                    },
                    "import-load-more",
                )
                .id("import-load-more")
                .role(gpui::Role::Button)
                .tab_index(0)
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.model.discovery.is_loading() {
                        this.load_sessions(true, cx);
                    }
                })),
            );
        }
        list.into_any_element()
    }

    fn render_notice(
        &self,
        index: usize,
        title: &str,
        copy: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("import-notice", index))
            .px(px(12.0))
            .py(px(10.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(crate::typography::ui_rems(13.0))
                    .child(title.to_owned()),
            )
            .child(popover::dialog_body(theme, copy.to_owned()))
            .child(
                popover::btn_ghost(theme, "Retry", "import-list-retry")
                    .id("import-list-retry")
                    .role(gpui::Role::Button)
                    .tab_index(0)
                    .on_click(cx.listener(|this, _, _, cx| this.load_sessions(false, cx))),
            )
            .into_any_element()
    }

    fn render_footer(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut actions = div().flex().items_center().justify_end().gap(px(8.0));
        let mut summary;
        if let Some(batch) = &self.model.batch {
            summary = if batch.running {
                format!("{} of {} processed", batch.completed(), batch.rows.len())
            } else {
                let failed = batch
                    .states
                    .values()
                    .filter(|state| {
                        matches!(
                            state,
                            ImportItemState::Finished(ImportOutcome::Failed { .. })
                        )
                    })
                    .count();
                let skipped = batch
                    .states
                    .values()
                    .filter(|state| {
                        matches!(
                            state,
                            ImportItemState::Finished(ImportOutcome::AlreadyManaged { .. })
                        )
                    })
                    .count();
                let cancelled = batch
                    .states
                    .values()
                    .filter(|state| matches!(state, ImportItemState::Cancelled))
                    .count();
                let mut parts = vec![format!("{} imported", batch.imported())];
                if skipped > 0 {
                    parts.push(format!("{skipped} already in Zerun"));
                }
                if failed > 0 {
                    parts.push(format!("{failed} failed"));
                }
                if cancelled > 0 {
                    parts.push(format!("{cancelled} not imported"));
                }
                parts.join(" · ")
            };
            if batch.running {
                actions = actions.child(
                    popover::btn_ghost(
                        theme,
                        if batch.cancelling {
                            "Stopping…"
                        } else {
                            "Stop import"
                        },
                        "import-cancel-batch",
                    )
                    .id("import-cancel-batch")
                    .role(gpui::Role::Button)
                    .tab_index(0)
                    .tab_stop(!batch.cancelling)
                    .on_click(cx.listener(|this, _, _, cx| this.close_or_cancel(cx))),
                );
            } else {
                actions = actions.child(
                    popover::btn_ghost(theme, "Choose more", "import-choose-more")
                        .id("import-choose-more")
                        .role(gpui::Role::Button)
                        .tab_index(0)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_more(cx))),
                );
                if !batch.retry_refs().is_empty() {
                    actions = actions.child(
                        popover::btn_ghost(theme, "Retry remaining", "import-retry-remaining")
                            .id("import-retry-remaining")
                            .role(gpui::Role::Button)
                            .tab_index(0)
                            .on_click(cx.listener(|this, _, _, cx| this.start_import(true, cx))),
                    );
                }
                actions = actions.child(
                    popover::btn_primary(theme, "Done")
                        .id("import-done")
                        .role(gpui::Role::Button)
                        .tab_index(0)
                        .on_click(cx.listener(|this, _, _, cx| this.close_or_cancel(cx))),
                );
            }
        } else {
            let selected = self.model.selected.len();
            let shown_selected = self
                .model
                .visible_sessions(self.search.read(cx).text())
                .iter()
                .filter(|row| self.model.selected.contains(&row.source_ref))
                .count();
            // Nothing selected needs no words; the dimmed import button says it.
            summary = if selected == 0 {
                String::new()
            } else if selected > shown_selected {
                format!(
                    "{selected} selected · {} hidden by filters",
                    selected - shown_selected
                )
            } else {
                format!("{selected} selected")
            };
            if selected == zeron_proto::MAX_EXTERNAL_SESSION_IMPORT_BATCH {
                summary.push_str(" · batch limit reached");
            }
            actions = actions
                .child(
                    popover::btn_ghost(theme, "Cancel", "import-close")
                        .id("import-close")
                        .role(gpui::Role::Button)
                        .tab_index(0)
                        .on_click(cx.listener(|this, _, _, cx| this.close_or_cancel(cx))),
                )
                .child(
                    div()
                        .flex_none()
                        .when(selected == 0, |wrapper| wrapper.opacity(0.4))
                        .child(
                            popover::btn_primary(
                                theme,
                                &if selected == 0 {
                                    "Import sessions".to_owned()
                                } else {
                                    format!(
                                        "Import {selected} session{}",
                                        if selected == 1 { "" } else { "s" }
                                    )
                                },
                            )
                            .id("import-submit")
                            .debug_selector(|| "import-submit".into())
                            .role(gpui::Role::Button)
                            .tab_index(0)
                            .tab_stop(selected > 0)
                            .when(selected == 0, |button| {
                                button.cursor(gpui::CursorStyle::Arrow)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.start_import(false, cx))),
                        ),
                );
        }
        div()
            .flex_none()
            .p(px(16.0))
            .border_t_1()
            .border_color(theme.border)
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(crate::typography::ui_rems(12.0))
                            .text_color(theme.text_muted)
                            .child(summary),
                    )
                    .child(actions),
            )
            .into_any_element()
    }
}

impl Render for SessionImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).for_popup();
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 48.0).clamp(280.0, 680.0);
        let height = (f32::from(viewport.height) - 80.0).clamp(350.0, 680.0);
        let focus_dialog = std::mem::take(&mut self.focus_dialog_pending);
        let focus_search = std::mem::take(&mut self.focus_pending);
        if focus_dialog || (focus_search && self.model.batch.is_some()) {
            window.focus(&self.focus, cx);
        } else if focus_search {
            window.focus(&self.search.focus_handle(cx), cx);
        }
        let importing = self.is_importing();
        let title = if importing {
            "Importing sessions…"
        } else if self.model.batch.is_some() {
            "Import results"
        } else {
            "Import existing sessions"
        };
        let mut heading = div().min_w_0().flex().items_center().gap(px(8.0)).child(
            div().flex_1().min_w_0().child(
                popover::dialog_title(&theme, title)
                    .text_size(crate::typography::ui_rems(16.0))
                    .truncate(),
            ),
        );
        if self.model.devices.len() > 1 && self.model.batch.is_none() {
            let device = &self.model.devices[self.model.device_index];
            let mut trigger = popover::btn_ghost_base(&theme, "import-source-device")
                .id("import-source-device")
                .debug_selector(|| "import-source-device".into())
                .role(gpui::Role::Button)
                .aria_label(SharedString::from(format!(
                    "Source device: {}",
                    device.name
                )))
                .aria_expanded(self.device_menu.is_open())
                .tab_index(0)
                .flex_none()
                .max_w(px(160.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(4.0))
                .tooltip(crate::settings::widgets::text_tooltip(device.name.clone()))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.device_menu.note_trigger_press();
                        cx.stop_propagation();
                    }),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.device_menu.take_press_was_open() {
                        this.close_device_menu(cx);
                    } else {
                        this.device_menu.open(());
                        this.device_menu_active = this.model.device_index;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }
                }))
                .child(
                    icon(icons::MONITOR)
                        .size(px(13.0))
                        .flex_none()
                        .text_color(theme.text_muted),
                )
                .child(div().min_w_0().truncate().child(device.name.clone()))
                .child(
                    icon(icons::ALT_ARROW_DOWN)
                        .size(px(12.0))
                        .flex_none()
                        .text_color(theme.text_muted.opacity(0.5)),
                );
            if self.device_menu.get().is_some() {
                let menu =
                    popover::popover_card(&theme)
                        .w(px(220.0))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.close_device_menu(cx);
                        }))
                        .children(
                            self.model
                                .devices
                                .iter()
                                .enumerate()
                                .map(|(index, device)| {
                                    popover::menu_row_nav(
                                        &theme,
                                        index == self.model.device_index,
                                        index == self.device_menu_active,
                                        format!("import-device-{index}"),
                                    )
                                    .id(("import-device", index))
                                    .debug_selector(move || format!("import-device-{index}"))
                                    .role(gpui::Role::Button)
                                    .aria_label(device.name.clone())
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.choose_device(index, cx)
                                    }))
                                    .child(device.name.clone())
                                }),
                        )
                        .into_any_element();
                // The dialog is a priority-2 modal layer, so the default
                // priority-1 menu would paint underneath the card; the
                // dropdown must sit one layer above the dialog.
                trigger = trigger.child(popover::anchored_menu_below_end_layer(
                    "import-device-menu",
                    menu,
                    self.device_menu.closing_since(),
                    6.0,
                    3,
                ));
            }
            heading = heading.child(trigger);
        }
        let header = div()
            .flex_none()
            .px(px(20.0))
            .pt(px(16.0))
            .pb(px(10.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(heading.when(!importing, |heading| {
                heading.child(
                    div()
                        .id("import-dialog-close")
                        .role(gpui::Role::Button)
                        .aria_label("Close import")
                        .tab_index(0)
                        .size(px(24.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .hover(|button| button.bg(theme.glass_hover()))
                        .on_click(cx.listener(|this, _, _, cx| this.close_or_cancel(cx)))
                        .child(
                            icon(icons::CLOSE)
                                .size(px(14.0))
                                .text_color(theme.text_muted),
                        ),
                )
            }))
            .child(
                popover::dialog_body(
                    &theme,
                    "Imports are saved in Archived. Project files stay shared.",
                )
                .text_size(crate::typography::ui_rems(12.0)),
            );
        let body = div()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .flex()
            .flex_col()
            .when(self.model.batch.is_none(), |list| {
                list.child(self.render_filters(&theme, width, cx))
            })
            .child(self.render_list(&theme, cx));
        popover::dialog_card(&theme)
            .id("session-import-dialog")
            .debug_selector(|| "session-import-dialog".into())
            .role(gpui::Role::Dialog)
            .aria_label(title)
            .track_focus(&self.focus)
            .key_context("SessionImport")
            .tab_group()
            .w(px(width))
            .h(px(height))
            .p_0()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(header)
            .when_some(
                self.model.batch.as_ref().filter(|batch| batch.running),
                |card, batch| {
                    card.child(
                        div().h(px(2.0)).flex_none().bg(theme.glass_hover()).child(
                            div()
                                .h_full()
                                .w(gpui::relative(
                                    batch.completed() as f32 / batch.rows.len().max(1) as f32,
                                ))
                                .bg(theme.text_muted),
                        ),
                    )
                },
            )
            .child(body)
            .child(self.render_footer(&theme, cx))
            .child(
                div()
                    .track_focus(&self.end_focus)
                    .tab_index(0)
                    .tab_stop(false),
            )
    }
}

#[cfg(feature = "session-import-fixture")]
impl SessionImportDialog {
    pub(crate) fn fixture_capture_ready(&self, scene: &str) -> bool {
        match scene {
            "scanning" => self.model.discovery.is_loading(),
            "scan-error" => self.model.discovery.error().is_some(),
            "results" | "partial" => self
                .model
                .batch
                .as_ref()
                .is_some_and(|batch| !batch.running),
            "device-menu" => {
                self.model.discovery.ready().is_some() && self.device_menu.get().is_some()
            }
            _ => self.model.discovery.ready().is_some(),
        }
    }

    pub(crate) fn fixture_prepare_scene(&mut self, scene: &str, cx: &mut Context<Self>) {
        if scene == "device-menu" {
            self.device_menu.open(());
            self.device_menu_active = self.model.device_index;
        }
        let open_menu = match scene {
            "filter-agent" => Some(FilterMenu::Agent),
            _ => None,
        };
        if let Some(kind) = open_menu {
            self.filter_menu.open(kind);
            self.filter_menu_active = match kind {
                FilterMenu::Agent => AGENT_FILTERS
                    .iter()
                    .position(|provider| *provider == self.model.provider),
            }
            .unwrap_or(0);
        }
        if matches!(scene, "selected" | "results" | "partial") {
            self.model.selected = self
                .model
                .sessions
                .iter()
                .filter(|row| {
                    row.eligibility == ImportEligibility::Available
                        && row.cwd.ends_with("/fieldnotes")
                })
                .map(|row| row.source_ref.clone())
                .collect();
            if matches!(scene, "results" | "partial") {
                self.start_import(false, cx);
            }
        }
        cx.notify();
    }
}

/// Keep an applied agent filter visible so it can always be cleared.
fn needs_provider_filter(model: &ImportModel) -> bool {
    model.provider.is_some()
        || model.sessions.first().is_some_and(|first| {
            model
                .sessions
                .iter()
                .any(|row| row.provider != first.provider)
        })
}

/// The rounded selection box shared by session rows and project headers.
fn selection_box(theme: &Theme, checked: bool) -> gpui::Div {
    div()
        .size(px(15.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked {
            theme.text
        } else {
            theme.border_strong
        })
        .when(checked, |control| {
            control
                .bg(theme.text)
                .child(icon(icons::CHECK).size(px(13.0)).text_color(theme.on_solid))
        })
}

pub(super) fn path_within(path: &str, parent: &str) -> bool {
    let normalize = |value: &str| {
        let value = value.replace('\\', "/");
        if value.as_bytes().get(1) == Some(&b':') {
            value.to_lowercase()
        } else {
            value
        }
    };
    let path = normalize(path);
    let parent = normalize(parent);
    let parent = parent.trim_end_matches('/');
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}
