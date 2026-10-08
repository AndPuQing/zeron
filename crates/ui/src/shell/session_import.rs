use super::*;
use crate::session_import::{SessionImportController, SessionImportDialog, SessionImportEvent};

#[cfg(feature = "session-import-fixture")]
mod fixture;
#[cfg(test)]
mod tests;

pub(super) struct ImportDialogState {
    pub view: Entity<SessionImportDialog>,
    previous_focus: Option<FocusHandle>,
    _events: Subscription,
}

pub(super) struct SessionImportUi {
    custom_controller: bool,
    engine: Option<crate::state::EngineHandle>,
    controller: Option<SessionImportController>,
    pub dialog: Option<ImportDialogState>,
    menu: popover::Popup<()>,
    menu_focus: FocusHandle,
}

impl SessionImportUi {
    pub fn new(cx: &mut Context<Shell>) -> Self {
        Self {
            custom_controller: false,
            engine: None,
            controller: None,
            dialog: None,
            menu: popover::Popup::default(),
            menu_focus: cx.focus_handle(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.controller
            .as_ref()
            .is_some_and(|controller| !controller.devices.is_empty())
    }

    pub fn owns_keyboard(&self) -> bool {
        self.dialog.is_some() || self.menu.is_open()
    }
}

impl Shell {
    /// Fixture/integration seam; production controllers come from engine capabilities.
    pub fn configure_session_import(
        &mut self,
        controller: SessionImportController,
        cx: &mut Context<Self>,
    ) {
        self.session_import.custom_controller = true;
        self.session_import.controller = Some(controller);
        cx.notify();
    }

    pub(super) fn refresh_session_import(&mut self, cx: &mut Context<Self>) {
        if self.session_import.custom_controller {
            return;
        }
        let state = self.state.read(cx);
        let engine = state
            .engine()
            .filter(|_| matches!(state.connection, ConnectionStatus::Ready))
            .cloned();
        let changed = match (&self.session_import.engine, &engine) {
            (Some(old), Some(new)) => !old.same_connection(new),
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.session_import.dialog = None;
            self.session_import.menu = popover::Popup::default();
        }
        self.session_import.engine = engine.clone();
        let Some(engine) = engine else {
            self.session_import.controller = None;
            return;
        };
        let mut ids = state
            .devices
            .iter()
            .map(|d| d.id.clone())
            .collect::<Vec<_>>();
        if !ids.contains(&engine.engine_info().device_id) {
            ids.insert(0, engine.engine_info().device_id.clone());
        }
        let devices = ids
            .into_iter()
            .filter(|id| {
                state.device_supports(id, zeron_proto::capabilities::EXTERNAL_SESSION_IMPORT_V1)
                    && state.device_online(id, Utc::now())
            })
            .map(|id| crate::session_import::ImportDevice {
                name: state.device_name(&id).unwrap_or("This device").to_owned(),
                current_project: state
                    .selected_space_row()
                    .filter(|s| s.device_id == id)
                    .map(|s| s.path.clone()),
                id,
            })
            .collect();
        self.session_import.controller = Some(SessionImportController {
            devices,
            handle: std::rc::Rc::new(move |view, event, cx| {
                view.update(cx, |view, cx| {
                    view.handle_rpc_event(engine.clone(), event, cx)
                });
            }),
        });
    }

    pub(super) fn open_session_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session_import.dialog.is_some() {
            return;
        }
        let Some(controller) = &self.session_import.controller else {
            return;
        };
        if controller.devices.is_empty() {
            return;
        }
        let devices = controller.devices.clone();
        let handler = controller.handle.clone();
        let device_id = self
            .state
            .read(cx)
            .local_device_id
            .clone()
            .unwrap_or_else(|| devices[0].id.clone());
        self.close_command_palette(window, cx);
        self.add_space = None;
        self.close_session_import_menu(cx);
        let previous_focus = window
            .focused(cx)
            .filter(|focus| focus != &self.session_import.menu_focus);
        let view = cx.new(|cx| SessionImportDialog::new(devices, &device_id, cx));
        let events = cx.subscribe_in(&view, window, move |this, view, event, window, cx| {
            if matches!(event, SessionImportEvent::Close) {
                this.close_session_import(window, cx);
                return;
            }
            handler(view.clone(), event.clone(), cx);
            if let SessionImportEvent::OpenChat { chat_id } = event {
                if this.state.read(cx).engine().is_some() {
                    this.set_chat_archived(chat_id.clone(), false, cx);
                }
                this.close_session_import(window, cx);
                this.open_chat(chat_id.clone(), cx);
            }
        });
        self.session_import.dialog = Some(ImportDialogState {
            view: view.clone(),
            previous_focus,
            _events: events,
        });
        view.update(cx, |view, cx| view.discover(cx));
        cx.notify();
    }

    fn close_session_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.session_import.dialog.take() {
            if let Some(focus) = dialog.previous_focus {
                window.focus(&focus, cx);
            } else {
                window.focus(&self.composer.focus_handle(cx), cx);
            }
            cx.notify();
        }
    }

    fn close_session_import_menu(&mut self, cx: &mut Context<Self>) {
        if self.session_import.menu.begin_close() {
            popover::reap_popup(cx, |shell| &mut shell.session_import.menu);
            cx.notify();
        }
    }

    pub(super) fn render_session_import_action(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.session_import.is_available() {
            return None;
        }
        let mut trigger = div()
            .id("sessions-options")
            .debug_selector(|| "sessions-options".into())
            .role(gpui::Role::Button)
            .aria_label("Session options")
            .tab_index(0)
            .size(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .hover(|button| button.bg(theme.glass_hover()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.session_import.menu.note_trigger_press();
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                if this.session_import.menu.take_press_was_open() {
                    this.close_session_import_menu(cx);
                } else {
                    this.session_import.menu.open(());
                    window.focus(&this.session_import.menu_focus, cx);
                    cx.notify();
                }
            }))
            .tooltip(crate::settings::widgets::text_tooltip("Session options"))
            .child(
                icon(icons::MORE_HORIZONTAL)
                    .size(px(14.0))
                    .text_color(theme.text_muted),
            );
        if self.session_import.menu.get().is_some() {
            let theme = theme.for_popup();
            let menu = popover::popover_card(&theme)
                .id("sessions-options-menu")
                .track_focus(&self.session_import.menu_focus)
                .w(px(236.0))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_session_import_menu(cx)))
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "escape" => this.close_session_import_menu(cx),
                        "enter" => this.open_session_import(window, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(
                    popover::menu_row(&theme, false, "sessions-import-entry")
                        .id("sessions-import-entry")
                        .role(gpui::Role::Button)
                        .aria_label("Import existing sessions")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_session_import(window, cx);
                            cx.stop_propagation();
                        }))
                        .child(
                            icon(icons::ARCHIVE_UP_MINIMALISTIC)
                                .size(px(16.0))
                                .text_color(theme.text_muted),
                        )
                        .child("Import existing sessions…"),
                )
                .into_any_element();
            trigger = trigger.child(popover::anchored_menu_below(
                "sessions-options-popover",
                menu,
                self.session_import.menu.closing_since(),
            ));
        }
        Some(trigger.into_any_element())
    }
}
