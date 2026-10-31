//! Settings → Usage: how each agent has done on the sessions stored on this
//! computer — success and rework rates, time spent, which agent fares best
//! per task type — beside the plan limits the agents report. The engine
//! works the numbers out from local transcripts (`GetUsageInsights`); the
//! limits are the Accounts list's last known usage windows.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, Entity, SharedString, Subscription, Task, Window, div, prelude::*, px,
};

use zeron_proto::usage_insights::{
    CategoryUsage, HarnessUsage, MIN_SAMPLE_TURNS, UsageInsightsReport, UsageTally,
};
use zeron_proto::{AgentAccount, AgentAccountsSnapshot, HarnessId};
use zeron_rpc::methods;

use crate::popover::{self, Loadable};
use crate::settings::accounts::{self, AccountsSnapshotCache};
use crate::settings::widgets;
use crate::state::AppState;
use crate::theme::Theme;

/// A report older than this is fetched again when the page is drawn.
const STALE_AFTER: Duration = Duration::from_secs(60);
/// While the engine is still reading older sessions, ask again this often.
const INDEXING_RETRY: Duration = Duration::from_millis(800);

/// The window of turns the page counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageRange {
    Week,
    #[default]
    Month,
    Quarter,
    All,
}

impl UsageRange {
    pub const ALL: [UsageRange; 4] = [
        UsageRange::Week,
        UsageRange::Month,
        UsageRange::Quarter,
        UsageRange::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UsageRange::Week => "7 days",
            UsageRange::Month => "30 days",
            UsageRange::Quarter => "90 days",
            UsageRange::All => "All time",
        }
    }

    /// The request's lower bound for a page drawn at `now_ms`. Pure.
    pub fn since_ms(self, now_ms: i64) -> Option<i64> {
        let days = match self {
            UsageRange::Week => 7,
            UsageRange::Month => 30,
            UsageRange::Quarter => 90,
            UsageRange::All => return None,
        };
        Some(now_ms - days * 86_400_000)
    }
}

/// "83%", or a dash when there is nothing to divide. Pure.
pub fn percent(rate: Option<f32>) -> String {
    match rate {
        Some(rate) => format!("{}%", (rate.clamp(0.0, 1.0) * 100.0).round() as u32),
        None => "—".into(),
    }
}

/// "45s", "3m 12s", "1h 5m"; under a second reads "<1s". Pure.
pub fn duration(ms: Option<i64>) -> String {
    match ms {
        None => "—".into(),
        Some(ms) if ms < 1_000 => "<1s".into(),
        Some(ms) => crate::transcript::format_elapsed(ms / 1_000),
    }
}

/// A harness slug's display name; a slug this build does not know reads
/// as itself. Pure.
pub fn harness_name(slug: &str) -> String {
    HarnessId::from_slug(slug)
        .map(|id| zeron_proto::providers::provider_name(id).to_string())
        .unwrap_or_else(|| slug.to_string())
}

fn count(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The accounts worth a limits row: ones reporting a usage window, live
/// login first within each agent. Pure.
pub fn limit_accounts(snapshot: &AgentAccountsSnapshot) -> Vec<&AgentAccount> {
    let mut accounts: Vec<&AgentAccount> = snapshot
        .accounts
        .iter()
        .filter(|account| !account.usage_windows.is_empty())
        .collect();
    accounts.sort_by_key(|account| (account.harness.as_str(), !account.active));
    accounts
}

pub struct UsagePage {
    state: Entity<AppState>,
    scroll: widgets::PageScroll,
    range: UsageRange,
    report: Loadable<UsageInsightsReport>,
    /// The range `report` counts when it is ready; it lags `range` while a
    /// newly picked range loads.
    report_range: UsageRange,
    /// The engine predates `GetUsageInsights`.
    unsupported: bool,
    loading: bool,
    loaded_at: Option<Instant>,
    error: Option<SharedString>,
    load_task: Option<Task<()>>,
    accounts: Option<AgentAccountsSnapshot>,
    accounts_task: Option<Task<()>>,
    _observe: Subscription,
}

impl UsagePage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&state, |_, _, cx| cx.notify());
        let accounts = cx
            .try_global::<AccountsSnapshotCache>()
            .and_then(|cache| cache.0.get(&None))
            .cloned();
        let mut page = Self {
            state,
            scroll: widgets::PageScroll::default(),
            range: UsageRange::default(),
            report: Loadable::Idle,
            report_range: UsageRange::default(),
            unsupported: false,
            loading: false,
            loaded_at: None,
            error: None,
            load_task: None,
            accounts,
            accounts_task: None,
            _observe: observe,
        };
        page.load(cx);
        page.load_accounts(cx);
        page
    }

    pub fn range(&self) -> UsageRange {
        self.range
    }

    pub fn set_range(&mut self, range: UsageRange, cx: &mut Context<Self>) {
        if self.range == range {
            return;
        }
        self.range = range;
        self.load(cx);
        cx.notify();
    }

    /// Fetch the report for the current range, asking again while the
    /// engine is still reading older sessions. A painted report stays up
    /// while a newer one loads.
    fn load(&mut self, cx: &mut Context<Self>) {
        // A range change supersedes the old request even while disconnected.
        // Leaving its loading flag set would block refresh_if_stale after
        // another engine connects.
        self.load_task = None;
        self.loading = false;
        self.unsupported = false;
        self.loaded_at = None;
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            self.report = Loadable::Error("Engine not connected".into());
            return;
        };
        if !engine
            .engine_info()
            .supports(zeron_proto::capabilities::USAGE_INSIGHTS_V1)
        {
            self.unsupported = true;
            return;
        }
        if !matches!(self.report, Loadable::Ready(_)) {
            self.report = Loadable::Loading;
        }
        self.loading = true;
        let range = self.range;
        let since = range.since_ms(chrono::Utc::now().timestamp_millis());
        self.load_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let result = engine
                    .client()
                    .call(
                        methods::GET_USAGE_INSIGHTS,
                        serde_json::json!({ "sinceMs": since }),
                    )
                    .await
                    .map_err(|err| err.to_string())
                    .and_then(|value| {
                        serde_json::from_value::<UsageInsightsReport>(value)
                            .map_err(|err| err.to_string())
                    });
                let again = this
                    .update(cx, |page, cx| {
                        if page.range != range {
                            return false;
                        }
                        let again = matches!(&result, Ok(report) if report.indexing);
                        match result {
                            Ok(report) => {
                                page.report = Loadable::Ready(report);
                                page.report_range = range;
                            }
                            // A failed refresh keeps the painted numbers, but
                            // only this range's: another range's must not
                            // stand in for them under this range's chip.
                            Err(err)
                                if matches!(page.report, Loadable::Ready(_))
                                    && page.report_range == range =>
                            {
                                page.error = Some(format!("Usage refresh failed: {err}").into());
                            }
                            Err(err) => page.report = Loadable::Error(err),
                        }
                        if !again {
                            page.loading = false;
                            page.loaded_at = Some(Instant::now());
                        }
                        cx.notify();
                        again
                    })
                    .unwrap_or(false);
                if !again {
                    break;
                }
                cx.background_executor().timer(INDEXING_RETRY).await;
            }
        }));
    }

    /// This device's accounts with their last known usage — the engine's
    /// plain list, no provider probe (Accounts and the composer ring do
    /// those). Shares their cache.
    fn load_accounts(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        self.accounts_task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(
                    methods::LIST_AGENT_ACCOUNTS,
                    serde_json::json!({ "forceUsage": false }),
                )
                .await
                .ok()
                .and_then(|value| serde_json::from_value::<AgentAccountsSnapshot>(value).ok());
            if let Some(snapshot) = result {
                this.update(cx, |page, cx| {
                    cx.default_global::<AccountsSnapshotCache>()
                        .0
                        .insert(None, snapshot.clone());
                    page.accounts = Some(snapshot);
                    cx.notify();
                })
                .ok();
            }
        }));
    }

    /// Drawn after a minute: fetch again. Never loaded (no engine yet, or
    /// one without the report): check again, which is free until an engine
    /// that answers is attached.
    fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let stale = self.loaded_at.is_none_or(|at| at.elapsed() > STALE_AFTER);
        if stale {
            self.load(cx);
            if self.loading {
                self.load_accounts(cx);
            }
        }
    }

    fn on_scroll_hovered(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.scroll.set_list_hovered(*hovered) {
            cx.notify();
        }
    }

    fn render_ranges(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .mt(px(20.0))
            .px(px(8.0))
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(6.0))
            .children(UsageRange::ALL.into_iter().enumerate().map(|(ix, range)| {
                let selected = range == self.range;
                div()
                    .id(("usage-range", ix))
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded_full()
                    .text_size(crate::typography::ui_rems(12.0))
                    .cursor_pointer()
                    .tab_index(0)
                    .role(gpui::Role::Button)
                    .aria_label(SharedString::from(format!("Show {}", range.label())))
                    .focus_visible(|s| s.border_2().border_color(theme.accent))
                    .when(selected, |el| {
                        el.bg(theme.wash(0.10))
                            .text_color(theme.text)
                            .font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .when(!selected, |el| {
                        el.text_color(theme.text_muted)
                            .hover(|s| s.bg(theme.wash(0.06)).text_color(theme.text))
                    })
                    .on_click(cx.listener(move |page, _, _, cx| page.set_range(range, cx)))
                    .child(SharedString::from(range.label()))
            }))
            .into_any_element()
    }

    fn render_body(&self, theme: &Theme) -> AnyElement {
        if self.unsupported {
            return notice(
                theme,
                "Update to see usage",
                "This computer's engine is older than the Usage page. Restart Zerun after updating.",
            );
        }
        let report = match &self.report {
            Loadable::Ready(report) => report,
            Loadable::Error(err) => {
                return notice(theme, "Usage unavailable", err.clone());
            }
            Loadable::Idle | Loadable::Loading => {
                return notice(
                    theme,
                    "Reading sessions…",
                    "Counting turns from this computer's transcripts.",
                );
            }
        };
        // Another range's numbers stay up while the picked one loads, dimmed
        // and labeled so they never read as the picked range's.
        if self.report_range != self.range {
            return div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .mt(px(12.0))
                        .px(px(8.0))
                        .text_size(crate::typography::ui_rems(12.0))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(format!(
                            "Loading {}…",
                            self.range.label()
                        ))),
                )
                .child(div().opacity(0.45).child(self.render_report(theme, report)))
                .into_any_element();
        }
        self.render_report(theme, report)
    }

    fn render_report(&self, theme: &Theme, report: &UsageInsightsReport) -> AnyElement {
        if report.total.turns == 0 {
            let detail = if report.indexing {
                "Still reading older sessions on this computer."
            } else if self.report_range == UsageRange::All {
                "Finished agent replies on this computer will be counted here."
            } else {
                "No agent replies finished in this period. Try a longer range."
            };
            return notice(theme, "No turns yet", detail);
        }
        div()
            .flex()
            .flex_col()
            .child(render_totals(theme, &report.total))
            .child(widgets::section(
                theme,
                "Agents",
                render_harnesses(theme, &report.harnesses),
            ))
            .when(!report.categories.is_empty(), |el| {
                el.child(widgets::section(
                    theme,
                    "Task types",
                    render_categories(theme, &report.categories),
                ))
            })
            .when(report.indexing, |el| {
                el.child(
                    div()
                        .mt(px(12.0))
                        .px(px(8.0))
                        .text_size(crate::typography::ui_rems(12.0))
                        .text_color(theme.text_muted)
                        .child(SharedString::from("Still reading older sessions…")),
                )
            })
            .into_any_element()
    }

    fn render_limits(&self, theme: &Theme) -> AnyElement {
        let accounts = self
            .accounts
            .as_ref()
            .map(limit_accounts)
            .unwrap_or_default();
        let block = if accounts.is_empty() {
            widgets::section_card(theme)
                .mt_0()
                .child(
                    div()
                        .px(px(16.0))
                        .py(px(14.0))
                        .text_size(crate::typography::ui_rems(12.5))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(
                            "Agents that report plan limits, such as Claude Code and Codex, show them here once signed in.",
                        )),
                )
        } else {
            let now = chrono::Utc::now();
            widgets::section_card(theme)
                .mt_0()
                .children(accounts.into_iter().enumerate().map(|(ix, account)| {
                    let mut meta: Vec<AnyElement> = Vec::new();
                    if let Some(who) = account
                        .email
                        .clone()
                        .or_else(|| account.display_name.clone())
                    {
                        meta.push(SharedString::from(who).into_any_element());
                    }
                    if let Some(plan) = account.plan_label.clone() {
                        meta.push(SharedString::from(plan).into_any_element());
                    }
                    if let Some(reset) = next_reset(account, now) {
                        meta.push(SharedString::from(reset).into_any_element());
                    }
                    widgets::card_row(theme, ix == 0)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .child(brand_mark(account.harness, theme))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(data_title(
                                            theme,
                                            zeron_proto::providers::provider_name(account.harness),
                                        ))
                                        .when(!meta.is_empty(), |el| {
                                            el.child(widgets::meta_line(theme, meta))
                                        }),
                                ),
                        )
                        .child(
                            div().flex_none().flex().flex_col().gap(px(4.0)).children(
                                account
                                    .usage_windows
                                    .iter()
                                    .map(|window| accounts::render_usage_meter(window, theme)),
                            ),
                        )
                        .into_any_element()
                }))
        };
        widgets::section(theme, "Plan limits", block).into_any_element()
    }
}

/// The soonest window reset still ahead, as "Resets in 2h 5m". Pure.
fn next_reset(account: &AgentAccount, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let soonest = account
        .usage_windows
        .iter()
        .filter_map(|window| window.resets_at)
        .filter(|at| *at > now)
        .min()?;
    Some(format!(
        "Resets in {}",
        crate::transcript::format_elapsed((soonest - now).num_seconds())
    ))
}

/// A data row's title: a name, not a setting, so the settings search
/// rightly does not index it (unlike [`widgets::row_title`]).
fn data_title(theme: &Theme, title: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w_0()
        .truncate()
        .text_size(crate::typography::ui_rems(13.0))
        .line_height(crate::typography::ui_rems(17.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.text)
        .child(title.into())
}

fn brand_mark(harness: HarnessId, theme: &Theme) -> AnyElement {
    let (path, tint) = crate::pickers::harness_brand_icon(harness);
    crate::icons::icon(path)
        .size(px(16.0))
        .flex_none()
        .text_color(tint.unwrap_or(theme.text_muted))
        .into_any_element()
}

/// A harness slug's mark; a slug this build does not know gets a bot.
fn slug_mark(slug: &str, theme: &Theme) -> AnyElement {
    match HarnessId::from_slug(slug) {
        Some(id) => brand_mark(id, theme),
        None => crate::icons::icon(crate::icons::BOT)
            .size(px(16.0))
            .flex_none()
            .text_color(theme.text_muted)
            .into_any_element(),
    }
}

fn notice(theme: &Theme, title: &str, detail: impl Into<SharedString>) -> AnyElement {
    div()
        .mt(px(72.0))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .text_color(theme.text_muted)
        .child(
            crate::icons::icon(crate::icons::CHART)
                .size(px(28.0))
                .text_color(theme.text_muted.opacity(0.25)),
        )
        .child(
            div()
                .mt(px(12.0))
                .text_size(crate::typography::ui_rems(14.0))
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .mt(px(4.0))
                .max_w(px(420.0))
                .text_size(crate::typography::ui_rems(12.0))
                .child(detail.into()),
        )
        .into_any_element()
}

/// One headline figure: a large value over a muted caption.
fn stat(theme: &Theme, value: String, caption: &str) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(104.0))
        .px(px(16.0))
        .py(px(14.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(crate::typography::ui_rems(20.0))
                .line_height(crate::typography::ui_rems(26.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(SharedString::from(value)),
        )
        .child(
            div()
                .text_size(crate::typography::ui_rems(12.0))
                .text_color(theme.text_muted)
                .child(SharedString::from(caption.to_string())),
        )
        .into_any_element()
}

fn render_totals(theme: &Theme, total: &UsageTally) -> AnyElement {
    widgets::section_card(theme)
        .flex_row()
        .flex_wrap()
        .child(stat(theme, total.sessions.to_string(), "Sessions"))
        .child(stat(theme, total.turns.to_string(), "Turns"))
        .child(stat(theme, percent(total.success_rate()), "Succeeded"))
        .child(stat(theme, percent(total.rework_rate()), "Needed rework"))
        .child(stat(
            theme,
            duration((total.total_duration_ms > 0).then_some(total.total_duration_ms)),
            "Agent time",
        ))
        .into_any_element()
}

/// A right-aligned figure with its caption, in a fixed-width column.
fn figure(theme: &Theme, value: String, caption: &str, tone: Option<gpui::Hsla>) -> AnyElement {
    div()
        .w(px(72.0))
        .flex_none()
        .flex()
        .flex_col()
        .items_end()
        .child(
            div()
                .text_size(crate::typography::ui_rems(13.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(tone.unwrap_or(theme.text))
                .child(SharedString::from(value)),
        )
        .child(
            div()
                .text_size(crate::typography::ui_rems(11.0))
                .text_color(theme.text_muted)
                .child(SharedString::from(caption.to_string())),
        )
        .into_any_element()
}

/// The success figure's tone: quiet when healthy, amber, then red.
fn success_tone(theme: &Theme, rate: Option<f32>) -> Option<gpui::Hsla> {
    match rate {
        Some(rate) if rate < 0.5 => Some(theme.danger),
        Some(rate) if rate < 0.8 => Some(theme.warning),
        _ => None,
    }
}

fn render_harnesses(theme: &Theme, harnesses: &[HarnessUsage]) -> AnyElement {
    widgets::section_card(theme)
        .mt_0()
        .children(harnesses.iter().enumerate().map(|(ix, usage)| {
            let tally = &usage.tally;
            let mut meta = vec![
                SharedString::from(count(tally.sessions, "session", "sessions")).into_any_element(),
                SharedString::from(count(tally.turns, "turn", "turns")).into_any_element(),
            ];
            if tally.failed > 0 {
                meta.push(
                    SharedString::from(format!("{} failed", tally.failed)).into_any_element(),
                );
            }
            if tally.interrupted > 0 {
                meta.push(
                    SharedString::from(format!("{} stopped", tally.interrupted)).into_any_element(),
                );
            }
            widgets::card_row(theme, ix == 0)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(slug_mark(&usage.harness, theme))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(data_title(theme, harness_name(&usage.harness)))
                                .child(widgets::meta_line(theme, meta)),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(figure(
                            theme,
                            percent(tally.success_rate()),
                            "succeeded",
                            success_tone(theme, tally.success_rate()),
                        ))
                        .child(figure(theme, percent(tally.rework_rate()), "rework", None))
                        .child(figure(
                            theme,
                            duration(tally.median_duration_ms),
                            "typical",
                            None,
                        )),
                )
                .into_any_element()
        }))
        .into_any_element()
}

/// What a task type's row says on the right: its best agent when two or
/// more have enough turns to compare, else each agent's success rate. Pure.
pub fn category_verdict(category: &CategoryUsage) -> Vec<(String, String)> {
    if let Some(best) = &category.best_harness {
        let rate = category
            .harnesses
            .iter()
            .find(|h| &h.harness == best)
            .and_then(|h| h.tally.success_rate());
        return vec![(best.clone(), format!("Best · {}", percent(rate)))];
    }
    category
        .harnesses
        .iter()
        .take(3)
        .map(|h| (h.harness.clone(), percent(h.tally.success_rate())))
        .collect()
}

fn render_categories(theme: &Theme, categories: &[CategoryUsage]) -> AnyElement {
    widgets::section_card(theme)
        .mt_0()
        .children(categories.iter().enumerate().map(|(ix, category)| {
            let tally = &category.tally;
            let meta = vec![
                SharedString::from(count(tally.sessions, "session", "sessions")).into_any_element(),
                SharedString::from(format!("{} succeeded", percent(tally.success_rate())))
                    .into_any_element(),
                SharedString::from(format!("{} rework", percent(tally.rework_rate())))
                    .into_any_element(),
            ];
            widgets::card_row(theme, ix == 0)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.0))
                        .flex()
                        .flex_col()
                        .child(data_title(theme, category.category.label()))
                        .child(widgets::meta_line(theme, meta)),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .children(category_verdict(category).into_iter().map(|(slug, label)| {
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .text_size(crate::typography::ui_rems(12.0))
                                .text_color(theme.text_muted)
                                .child(slug_mark(&slug, theme))
                                .child(SharedString::from(label))
                        })),
                )
                .into_any_element()
        }))
        .into_any_element()
}

impl popover::ScrollRailHost for UsagePage {
    fn rail_bar(&mut self) -> &mut popover::MenuScrollbarState {
        self.scroll.rail_bar()
    }

    fn rail_scroll(&self) -> Option<gpui::ScrollHandle> {
        self.scroll.rail_scroll()
    }
}

impl Render for UsagePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::toast::take_error(&mut self.error, cx);
        self.refresh_if_stale(cx);
        let theme = Theme::of(cx).for_settings_surface();
        let ranges = self.render_ranges(&theme, cx);
        let body = self.render_body(&theme);
        let limits = self.render_limits(&theme);
        let footnote = div()
            .mt(px(24.0))
            .px(px(8.0))
            .text_size(crate::typography::ui_rems(11.5))
            .line_height(crate::typography::ui_rems(16.0))
            .text_color(theme.text_muted.opacity(0.8))
            .child(SharedString::from(format!(
                "A turn is one agent reply. It succeeded when it finished without an error; it needed \
                 rework when your next message asked to fix or redo it. Task types come from each \
                 session's first message. An agent is named best for a task type once two or more have \
                 at least {MIN_SAMPLE_TURNS} turns there. Worked out on this computer from its own \
                 transcripts; nothing is sent anywhere."
            )));
        let scrollbar = popover::rail(self, "usage-page-scrollbar", &theme, cx);
        div()
            .id("usage-page-host")
            .relative()
            .size_full()
            .on_hover(cx.listener(Self::on_scroll_hovered))
            .child(
                crate::edge_fade::edge_faded(
                    16.0,
                    true,
                    true,
                    div()
                        .id("usage-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll.scroll)
                        .child(
                            widgets::page_column()
                                .child(widgets::page_header(&theme, "Usage", None))
                                .child(widgets::page_subtitle(
                                    &theme,
                                    "How each agent has done on the sessions stored on this computer.",
                                ))
                                .child(ranges)
                                .child(body)
                                .child(limits)
                                .child(footnote),
                        ),
                )
                .fade_overflow_y(&self.scroll.scroll),
            )
            .children(scrollbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeron_proto::usage_insights::{
        ChatUsage, TaskCategory, TurnOutcome, TurnRecord, aggregate,
    };

    #[test]
    fn ranges_count_back_whole_days() {
        let now = 100 * 86_400_000;
        assert_eq!(UsageRange::Week.since_ms(now), Some(93 * 86_400_000));
        assert_eq!(UsageRange::Quarter.since_ms(now), Some(10 * 86_400_000));
        assert_eq!(UsageRange::All.since_ms(now), None);
        assert_eq!(UsageRange::default(), UsageRange::Month);
    }

    #[test]
    fn figures_read_compactly() {
        assert_eq!(percent(Some(0.834)), "83%");
        assert_eq!(percent(Some(1.0)), "100%");
        assert_eq!(percent(None), "—");
        assert_eq!(duration(Some(400)), "<1s");
        assert_eq!(duration(Some(45_000)), "45s");
        assert_eq!(duration(Some(192_000)), "3m 12s");
        assert_eq!(duration(None), "—");
        assert_eq!(harness_name("claude-code"), "Claude Code");
        assert_eq!(harness_name("copilot"), "GitHub Copilot");
        assert_eq!(harness_name("omp"), "Oh My Pi");
        assert_eq!(harness_name("dsh"), "DeepSeek Harness");
        for id in HarnessId::ALL {
            assert_eq!(
                harness_name(id.as_str()),
                zeron_proto::providers::provider_name(id)
            );
        }
        assert_eq!(harness_name("a-future-agent"), "a-future-agent");
        assert_eq!(count(1, "turn", "turns"), "1 turn");
        assert_eq!(count(3, "turn", "turns"), "3 turns");
    }

    fn turns(harness: &str, completed: usize, failed: usize) -> Vec<TurnRecord> {
        let turn = |outcome| TurnRecord {
            harness: harness.into(),
            started_at: 0,
            duration_ms: Some(1_000),
            outcome,
            reworked: false,
        };
        (0..completed)
            .map(|_| turn(TurnOutcome::Completed))
            .chain((0..failed).map(|_| turn(TurnOutcome::Failed)))
            .collect()
    }

    #[test]
    fn a_category_names_its_best_agent_or_lists_their_rates() {
        let chats = vec![
            ChatUsage {
                category: TaskCategory::Bugfix,
                turns: turns("codex", 6, 0),
            },
            ChatUsage {
                category: TaskCategory::Bugfix,
                turns: turns("claude-code", 3, 3),
            },
        ];
        let report = aggregate(&chats, None);
        assert_eq!(
            category_verdict(&report.categories[0]),
            vec![("codex".to_string(), "Best · 100%".to_string())]
        );

        let chats = vec![ChatUsage {
            category: TaskCategory::Docs,
            turns: turns("codex", 1, 1),
        }];
        let report = aggregate(&chats, None);
        assert_eq!(
            category_verdict(&report.categories[0]),
            vec![("codex".to_string(), "50%".to_string())]
        );
    }

    #[test]
    fn limits_list_reporting_accounts_live_login_first() {
        let account = |id: &str, harness, active, windows: usize| AgentAccount {
            id: id.into(),
            harness,
            email: None,
            plan_label: None,
            active,
            usage_windows: (0..windows)
                .map(|_| zeron_proto::AgentUsageWindow {
                    label: "Session".into(),
                    used_fraction: 0.5,
                    resets_at: None,
                })
                .collect(),
            ..serde_json::from_value(serde_json::json!({
                "id": id, "harness": "codex", "email": null, "planLabel": null, "active": false
            }))
            .unwrap()
        };
        let snapshot = AgentAccountsSnapshot {
            accounts: vec![
                account("c2", HarnessId::Codex, false, 1),
                account("c1", HarnessId::Codex, true, 1),
                account("x", HarnessId::Cursor, true, 0),
            ],
            warnings: vec![],
        };
        let ids: Vec<&str> = limit_accounts(&snapshot)
            .into_iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(ids, ["c1", "c2"]);
    }

    #[gpui::test]
    fn a_range_changed_while_disconnected_loads_after_reconnection(cx: &mut gpui::TestAppContext) {
        use crate::state::EngineHandle;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _runtime = runtime.enter();
        let (out, mut requests) = tokio::sync::mpsc::channel(8);
        let (_replies, inbound) = tokio::sync::mpsc::channel(8);
        let state = cx.new(|_| {
            let mut state = AppState::new();
            state.set_test_engine(EngineHandle::from_test_client(zeron_rpc::RpcClient::new(
                out, inbound,
            )));
            state
        });
        let page = cx.new(|cx| UsagePage::new(state.clone(), cx));
        cx.run_until_parked();
        let mut requested_usage = false;
        while let Ok(frame) = requests.try_recv() {
            let frame: serde_json::Value = serde_json::from_str(&frame).unwrap();
            requested_usage |= frame["method"] == methods::GET_USAGE_INSIGHTS;
        }
        assert!(requested_usage, "the first report is still pending");

        state.update(cx, |state, cx| state.prepare_runtime_replacement(cx));
        page.update(cx, |page, cx| {
            page.set_range(UsageRange::Week, cx);
            assert!(!page.loading, "the old request cannot block a new engine");
            assert!(page.load_task.is_none());
            assert!(matches!(page.report, Loadable::Error(_)));
        });

        let (out, mut requests) = tokio::sync::mpsc::channel(8);
        let (_replies, inbound) = tokio::sync::mpsc::channel(8);
        state.update(cx, |state, _| {
            state.set_test_engine(EngineHandle::from_test_client(zeron_rpc::RpcClient::new(
                out, inbound,
            )));
        });
        let before = chrono::Utc::now().timestamp_millis();
        page.update(cx, |page, cx| page.refresh_if_stale(cx));
        cx.run_until_parked();
        let after = chrono::Utc::now().timestamp_millis();
        let mut requested_week = false;
        while let Ok(frame) = requests.try_recv() {
            let frame: serde_json::Value = serde_json::from_str(&frame).unwrap();
            if frame["method"] == methods::GET_USAGE_INSIGHTS {
                let since = frame["params"]["sinceMs"].as_i64().unwrap();
                assert!(since >= UsageRange::Week.since_ms(before).unwrap());
                assert!(since <= UsageRange::Week.since_ms(after).unwrap());
                requested_week = true;
            }
        }
        assert!(requested_week, "reconnection requests the selected range");
    }

    #[gpui::test]
    fn populated_reports_and_range_refresh_states_render(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        cx.update(|cx| {
            crate::settings::init(Default::default(), dir.path(), cx);
            gpui_base::init(cx);
            cx.set_global(Theme::default());
        });
        let state = cx.new(|_| AppState::new());
        let (page, cx) = cx.add_window_view(|_, cx| UsagePage::new(state.clone(), cx));
        let report = aggregate(
            &[
                ChatUsage {
                    category: TaskCategory::Bugfix,
                    turns: turns("codex", 6, 0),
                },
                ChatUsage {
                    category: TaskCategory::Bugfix,
                    turns: turns("claude-code", 3, 3),
                },
            ],
            Some(0),
        );
        for (range, indexing) in [
            (UsageRange::Month, false),
            (UsageRange::Month, true),
            (UsageRange::Week, false),
        ] {
            page.update(cx, |page, cx| {
                let mut report = report.clone();
                report.indexing = indexing;
                page.report = Loadable::Ready(report);
                page.report_range = UsageRange::Month;
                page.range = range;
                page.loading = indexing || range != UsageRange::Month;
                page.loaded_at = Some(Instant::now());
                cx.notify();
            });
            cx.update(|window, cx| window.draw(cx).clear());
        }
        page.update(cx, |page, cx| {
            page.report = Loadable::Error("engine disconnected".into());
            page.loading = false;
            page.loaded_at = Some(Instant::now());
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear());
    }
}
