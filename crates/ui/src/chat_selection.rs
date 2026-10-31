//! Snapshot a selection before focus moves, then stage it in its owning chat.
use crate::{icons, popover, theme::Theme};
use gpui::{
    ClipboardItem, Context, Empty, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, Pixels,
    Point, Render, Role, SharedString, Window, prelude::*, px,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatQuote {
    pub chat_id: String,
    pub source: String,
    pub text: String,
}

impl ChatQuote {
    pub fn new(chat_id: String, source: String, text: String) -> Option<Self> {
        if chat_id.is_empty() || text.trim().is_empty() {
            return None;
        }
        Some(Self {
            chat_id,
            source,
            text,
        })
    }

    pub fn prompt(&self) -> String {
        let source = self.source.replace(['\r', '\n'], " ");
        let quote = self
            .text
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Quoted from {source}:\n\n{quote}\n\n")
    }

    pub fn file(
        chat_id: String,
        path: &str,
        contents: &str,
        range: std::ops::Range<usize>,
    ) -> Option<Self> {
        let text = contents.get(range.clone())?;
        if range.is_empty() {
            return None;
        }
        let first = contents[..range.start]
            .bytes()
            .filter(|&c| c == b'\n')
            .count()
            + 1;
        let last = first
            + text
                .trim_end_matches('\n')
                .bytes()
                .filter(|&c| c == b'\n')
                .count();
        Self::new(
            chat_id,
            format!("{path}:{first}-{last} (workspace file)"),
            text.to_owned(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSelectionAction {
    AddToChat,
    AskInSideChat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSelectionEvent {
    pub quote: ChatQuote,
    pub action: ChatSelectionAction,
}

struct Menu {
    quote: ChatQuote,
    position: Point<Pixels>,
    active: Option<usize>,
}

pub(crate) struct SelectionMenu {
    menu: popover::Popup<Menu>,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
}

impl EventEmitter<ChatSelectionEvent> for SelectionMenu {}

impl SelectionMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            menu: Default::default(),
            focus: cx.focus_handle(),
            return_focus: None,
        }
    }

    pub fn open(
        &mut self,
        quote: ChatQuote,
        position: Point<Pixels>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.open(Menu {
            quote,
            position,
            active: None,
        });
        if focus {
            if !self.focus.is_focused(window) {
                self.return_focus = window.focused(cx);
            }
            window.focus(&self.focus, cx);
        } else {
            self.return_focus = None;
        }
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.menu.begin_close() {
            popover::reap_popup(cx, |this: &mut Self| &mut this.menu);
        }
        cx.notify();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_open() else {
            return;
        };
        let quote = menu.quote.clone();
        self.close(cx);
        match index {
            0 => {
                cx.write_to_clipboard(ClipboardItem::new_string(quote.text));
                if let Some(focus) = self.return_focus.take() {
                    window.focus(&focus, cx);
                }
            }
            1 | 2 => cx.emit(ChatSelectionEvent {
                quote,
                action: if index == 1 {
                    ChatSelectionAction::AddToChat
                } else {
                    ChatSelectionAction::AskInSideChat
                },
            }),
            _ => {}
        }
    }
}

impl Render for SelectionMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(menu) = self.menu.get() else {
            return Empty.into_any_element();
        };
        let (position, active) = (menu.position, menu.active);
        let theme = Theme::of(cx).for_popup();
        let mut card = popover::popover_card(&theme)
            .id("selection-chat-actions-card")
            .track_focus(&self.focus)
            .role(Role::Menu)
            .w(px(208.0))
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close(cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match popover::classify_key(
                    &event.keystroke.key,
                    event.keystroke.modifiers.platform,
                    event.keystroke.modifiers.control,
                ) {
                    popover::MenuKey::Escape => {
                        this.close(cx);
                        if let Some(focus) = this.return_focus.take() {
                            window.focus(&focus, cx);
                        }
                    }
                    key @ (popover::MenuKey::Up | popover::MenuKey::Down) => {
                        if let Some(menu) = this.menu.open_mut() {
                            menu.active = popover::menu_step(
                                menu.active,
                                3,
                                if matches!(key, popover::MenuKey::Up) {
                                    -1
                                } else {
                                    1
                                },
                            );
                        }
                        cx.notify();
                    }
                    popover::MenuKey::Enter => {
                        if let Some(index) = this.menu.as_open().and_then(|m| m.active) {
                            this.activate(index, window, cx);
                        }
                    }
                    _ => return,
                }
                window.prevent_default();
                cx.stop_propagation();
            }));
        for (index, (label, icon)) in [
            ("Copy", icons::COPY),
            ("Add to chat", icons::CHAT_ROUND_LINE),
            ("Ask in side chat", icons::CHAT_ROUND_LINE),
        ]
        .into_iter()
        .enumerate()
        {
            let id = SharedString::from(format!("selection-chat-action-{index}"));
            card = card.child(
                popover::menu_row(&theme, active == Some(index), id.clone())
                    .id(id)
                    .role(Role::MenuItem)
                    .aria_label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate(index, window, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        icons::icon(icon)
                            .size(px(16.0))
                            .text_color(theme.text_muted),
                    )
                    .child(label),
            );
        }
        popover::menu_at(
            "selection-chat-actions",
            position,
            card.into_any_element(),
            self.menu.closing_since(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn selection_menu_keyboard_emits_the_captured_chat_and_text(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(Theme::dark());
        });
        let (menu, cx) = cx.add_window_view(|_, cx| SelectionMenu::new(cx));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = events.clone();
        let _sub = cx.update(|_, cx| {
            cx.subscribe(&menu, move |_, event, _| {
                recorded.borrow_mut().push(event.clone())
            })
        });
        let quote =
            ChatQuote::new("owner".into(), "answer".into(), "selected words".into()).unwrap();
        cx.update(|window, cx| {
            window.activate_window();
            menu.update(cx, |menu, cx| {
                menu.open(
                    quote.clone(),
                    gpui::point(px(30.), px(30.)),
                    true,
                    window,
                    cx,
                )
            });
            window.draw(cx).clear();
        });
        cx.simulate_keystrokes("down down enter");
        cx.run_until_parked();
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(events.borrow()[0].quote, quote);
        assert_eq!(events.borrow()[0].action, ChatSelectionAction::AddToChat);
    }

    #[test]
    fn file_quote_keeps_unicode_unsaved_text_and_excludes_the_next_line() {
        let file = "header\n新内容 🦀\nlast";
        let quote = ChatQuote::file(
            "chat".into(),
            "src/main.rs",
            file,
            7..file.find("last").unwrap(),
        )
        .unwrap();
        assert_eq!(quote.source, "src/main.rs:2-2 (workspace file)");
        assert!(quote.prompt().contains("> 新内容 🦀"));
        assert!(!quote.prompt().contains("last"));
        assert!(ChatQuote::file("chat".into(), "x", file, 8..10).is_none());
    }
}
