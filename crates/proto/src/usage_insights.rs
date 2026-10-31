//! Personal usage insights: how each agent has done on the conversations
//! stored on one computer. Derived data, never synced. The host reads each
//! transcript into a [`ChatUsage`] (`zeron_doc::usage`), caches those
//! locally, and [`aggregate`]s them per request. Pure.
//!
//! Everything here is a heuristic over what transcripts record:
//! - a turn is one assistant reply. It *completed* when it finished without
//!   an error, *failed* when it ended on one, and was *interrupted* when it
//!   was stopped or cut off;
//! - a completed turn was *reworked* when the person's next message asks to
//!   correct it ([`is_correction`]);
//! - a session's task type comes from keywords in its first message
//!   ([`classify_task`]).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// What a session was about, inferred from its opening request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskCategory {
    Bugfix,
    Test,
    Refactor,
    Performance,
    Docs,
    Review,
    Ops,
    Feature,
    Explain,
    Other,
}

impl TaskCategory {
    /// Every category. The order is also the tie-break when two categories
    /// match a request equally often: "fix the failing test" is a bug fix,
    /// "add tests" is a test task.
    pub const ALL: [TaskCategory; 10] = [
        TaskCategory::Bugfix,
        TaskCategory::Test,
        TaskCategory::Refactor,
        TaskCategory::Performance,
        TaskCategory::Docs,
        TaskCategory::Review,
        TaskCategory::Ops,
        TaskCategory::Feature,
        TaskCategory::Explain,
        TaskCategory::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TaskCategory::Bugfix => "Bug fixes",
            TaskCategory::Test => "Tests",
            TaskCategory::Refactor => "Refactoring",
            TaskCategory::Performance => "Performance",
            TaskCategory::Docs => "Docs",
            TaskCategory::Review => "Reviews",
            TaskCategory::Ops => "Build and release",
            TaskCategory::Feature => "Features",
            TaskCategory::Explain => "Questions and research",
            TaskCategory::Other => "Other",
        }
    }

    fn keywords(self) -> &'static Keywords {
        match self {
            TaskCategory::Bugfix => &BUGFIX,
            TaskCategory::Test => &TEST,
            TaskCategory::Refactor => &REFACTOR,
            TaskCategory::Performance => &PERFORMANCE,
            TaskCategory::Docs => &DOCS,
            TaskCategory::Review => &REVIEW,
            TaskCategory::Ops => &OPS,
            TaskCategory::Feature => &FEATURE,
            TaskCategory::Explain => &EXPLAIN,
            TaskCategory::Other => &NONE,
        }
    }
}

/// One category's vocabulary: whole English words, English word prefixes,
/// and substrings (Chinese, or English phrases with spaces).
struct Keywords {
    words: &'static [&'static str],
    stems: &'static [&'static str],
    phrases: &'static [&'static str],
}

const NONE: Keywords = Keywords {
    words: &[],
    stems: &[],
    phrases: &[],
};

const BUGFIX: Keywords = Keywords {
    words: &[
        "fix",
        "fixes",
        "fixed",
        "fixing",
        "bug",
        "bugs",
        "buggy",
        "crash",
        "crashes",
        "crashed",
        "crashing",
        "broken",
        "breaks",
        "error",
        "errors",
        "fail",
        "fails",
        "failed",
        "failing",
        "failure",
        "failures",
        "issue",
        "issues",
        "wrong",
        "incorrect",
        "panic",
        "panics",
        "exception",
        "traceback",
        "hotfix",
        "flaky",
        "glitch",
    ],
    stems: &["regress"],
    phrases: &[
        "doesn't work",
        "does not work",
        "not working",
        "stopped working",
        "修复",
        "修一下",
        "修正",
        "报错",
        "错误",
        "崩溃",
        "闪退",
        "失败",
        "异常",
        "不工作",
        "不生效",
        "没生效",
        "没反应",
        "不能用",
        "用不了",
        "有问题",
        "卡死",
        "白屏",
        "无法",
    ],
};

const TEST: Keywords = Keywords {
    words: &[
        "test",
        "tests",
        "testing",
        "tested",
        "unittest",
        "unittests",
        "coverage",
        "spec",
        "specs",
        "e2e",
    ],
    stems: &[],
    phrases: &["测试", "单测", "用例", "覆盖率"],
};

const REFACTOR: Keywords = Keywords {
    words: &[
        "cleanup",
        "rename",
        "renamed",
        "renaming",
        "extract",
        "tidy",
        "modernize",
        "decouple",
    ],
    stems: &[
        "refactor",
        "restructur",
        "reorganiz",
        "reorganis",
        "simplif",
        "dedup",
    ],
    phrases: &[
        "clean up",
        "重构",
        "整理",
        "简化",
        "重命名",
        "拆分",
        "抽取",
        "抽离",
        "清理",
        "解耦",
        "代码结构",
        "优化代码",
    ],
};

const PERFORMANCE: Keywords = Keywords {
    words: &[
        "perf",
        "performance",
        "performant",
        "slow",
        "slower",
        "slowness",
        "faster",
        "speed",
        "speedup",
        "latency",
        "memory",
        "leak",
        "leaks",
        "benchmark",
        "benchmarks",
        "profiling",
        "profiler",
        "fps",
        "jank",
        "lag",
        "laggy",
        "throughput",
        "cpu",
    ],
    stems: &["optimi"],
    phrases: &[
        "性能", "卡顿", "太慢", "很慢", "变慢", "内存", "延迟", "优化", "耗时", "加速", "泄漏",
        "帧率",
    ],
};

const DOCS: Keywords = Keywords {
    words: &[
        "doc",
        "docs",
        "documentation",
        "document",
        "documents",
        "documented",
        "readme",
        "changelog",
        "comment",
        "comments",
        "docstring",
        "docstrings",
        "tutorial",
        "typo",
        "typos",
        "translate",
        "translation",
        "wording",
    ],
    stems: &[],
    phrases: &["文档", "注释", "教程", "翻译", "文案", "错别字", "措辞"],
};

const REVIEW: Keywords = Keywords {
    words: &[
        "review",
        "reviews",
        "reviewing",
        "audit",
        "critique",
        "feedback",
    ],
    stems: &[],
    phrases: &["审查", "评审", "审阅", "检查一下"],
};

const OPS: Keywords = Keywords {
    words: &[
        "deploy",
        "deploys",
        "deployed",
        "deploying",
        "deployment",
        "release",
        "releases",
        "releasing",
        "ci",
        "pipeline",
        "docker",
        "dockerfile",
        "kubernetes",
        "k8s",
        "install",
        "installer",
        "upgrade",
        "bump",
        "dependency",
        "dependencies",
        "deps",
        "build",
        "builds",
        "compile",
        "compiling",
        "packaging",
        "migrate",
        "migration",
    ],
    stems: &[],
    phrases: &[
        "github actions",
        "部署",
        "发布",
        "构建",
        "打包",
        "编译",
        "安装",
        "升级",
        "依赖",
        "流水线",
        "上线",
        "配置环境",
        "环境配置",
    ],
};

const FEATURE: Keywords = Keywords {
    words: &[
        "add",
        "adds",
        "added",
        "adding",
        "implement",
        "implements",
        "implemented",
        "implementing",
        "implementation",
        "create",
        "creates",
        "creating",
        "support",
        "supports",
        "feature",
        "features",
        "introduce",
        "enable",
        "allow",
        "integrate",
        "integration",
        "scaffold",
    ],
    stems: &[],
    phrases: &[
        "实现",
        "添加",
        "新增",
        "增加",
        "加一个",
        "加个",
        "做一个",
        "支持",
        "开发",
        "功能",
        "接入",
        "集成",
        "写一个",
        "搭建",
    ],
};

const EXPLAIN: Keywords = Keywords {
    words: &[
        "explain",
        "explains",
        "explained",
        "explaining",
        "why",
        "how",
        "understand",
        "understanding",
        "investigate",
        "investigation",
        "research",
        "analyze",
        "analyse",
        "analysis",
        "overview",
        "summarize",
        "summarise",
        "summary",
        "walkthrough",
        "compare",
        "learn",
        "question",
        "brainstorm",
    ],
    stems: &[],
    phrases: &[
        "解释",
        "为什么",
        "怎么",
        "如何",
        "是什么",
        "了解",
        "调研",
        "分析",
        "搞清楚",
        "弄清楚",
        "看看",
        "介绍",
        "总结",
        "梳理",
        "研究",
        "讲讲",
        "说说",
        "区别",
        "对比",
        "方案",
    ],
};

/// How much of a message the classifiers read: the request, not the logs or
/// code pasted after it.
const HEAD_CHARS: usize = 2_000;

fn head(text: &str, chars: usize) -> String {
    text.chars().take(chars).collect::<String>().to_lowercase()
}

/// Lowercase ASCII words; everything else (punctuation, CJK) separates them.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
}

/// How many distinct words and phrases of `keywords` occur in `text`
/// (already lowercased).
fn score(keywords: &Keywords, text: &str) -> usize {
    let tokens: HashSet<&str> = words(text).collect();
    let words = tokens
        .iter()
        .filter(|token| {
            keywords.words.contains(token)
                || keywords.stems.iter().any(|stem| token.starts_with(stem))
        })
        .count();
    let phrases = keywords
        .phrases
        .iter()
        .filter(|phrase| text.contains(*phrase))
        .count();
    words + phrases
}

/// The task type of a session from its opening request: the category with
/// the most keyword matches, ties broken by [`TaskCategory::ALL`] order. A
/// request matching nothing reads as a question when it ends with a
/// question mark, else Other.
pub fn classify_task(prompt: &str) -> TaskCategory {
    let text = head(prompt, HEAD_CHARS);
    let mut best = (0, TaskCategory::Other);
    for category in TaskCategory::ALL {
        let score = score(category.keywords(), &text);
        if score > best.0 {
            best = (score, category);
        }
    }
    if best.0 > 0 {
        return best.1;
    }
    let trimmed = text.trim_end();
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        TaskCategory::Explain
    } else {
        TaskCategory::Other
    }
}

/// Openings that reject the previous reply outright.
const CORRECTION_OPENINGS: &[&str] = &[
    "no ", "no,", "no.", "no!", "nope", "wrong", "不对", "不是", "错了", "不行",
];

/// "No …" openings that wave something off instead of rejecting the reply.
const AGREEABLE_OPENINGS: &[&str] = &[
    "no need",
    "no problem",
    "no worries",
    "no rush",
    "no thanks",
    "不是很急",
];

/// Phrases that ask for the previous reply to be corrected.
const CORRECTION_PHRASES: &[&str] = &[
    "still not",
    "still fail",
    "still broken",
    "still doesn",
    "still the same",
    "still getting",
    "still see",
    "still happen",
    "still crash",
    "still error",
    "still show",
    "doesn't work",
    "does not work",
    "didn't work",
    "did not work",
    "not working",
    "isn't working",
    "no longer works",
    "broken",
    "you broke",
    "that's wrong",
    "that is wrong",
    "is wrong",
    "was wrong",
    "not what i",
    "not quite",
    "incorrect",
    "revert that",
    "revert it",
    "revert this",
    "revert those",
    "revert your",
    "revert the change",
    "undo that",
    "undo it",
    "undo this",
    "undo those",
    "undo your",
    "undo the change",
    "roll it back",
    "roll that back",
    "roll back your",
    "roll back the change",
    "try again",
    "didn't fix",
    "did not fix",
    "doesn't fix",
    "not fixed",
    "same error",
    "same issue",
    "same problem",
    "failed",
    "it fails",
    "now fails",
    "test fails",
    "tests fail",
    "build fails",
    "is failing",
    "are failing",
    "now failing",
    "error:",
    "traceback",
    "panicked",
    "还是不行",
    "还是有",
    "还是报",
    "还是没",
    "还是会",
    "还是一样",
    "依然",
    "仍然",
    "不对",
    "不行",
    "没用",
    "没有用",
    "报错了",
    "一直报错",
    "报错：",
    "报错:",
    "出错了",
    "错了",
    "失败了",
    "又失败",
    "还是失败",
    "回滚",
    "撤销",
    "撤回",
    "重新做",
    "重新改",
    "重写",
    "不是这样",
    "不是我要",
    "不是我想",
    "有问题",
    "没生效",
    "没有生效",
    "不生效",
    "没反应",
    "又坏",
    "坏了",
    "没修好",
    "没有修好",
    "不起作用",
    "不工作",
    "搞错",
    "弄错",
    "漏了",
    "不符合",
];

/// How much of a follow-up [`is_correction`] reads.
const CORRECTION_HEAD_CHARS: usize = 600;

/// Whether a follow-up message asks to correct the reply before it: it
/// rejects it ("no, …", "不对") or reports that the result is still wrong,
/// broken, or failing.
pub fn is_correction(text: &str) -> bool {
    let text = head(text, CORRECTION_HEAD_CHARS);
    let text = text.trim_start();
    if text.is_empty() {
        return false;
    }
    if AGREEABLE_OPENINGS
        .iter()
        .any(|opening| text.starts_with(opening))
    {
        return false;
    }
    text == "no"
        || CORRECTION_OPENINGS
            .iter()
            .any(|opening| text.starts_with(opening))
        || CORRECTION_PHRASES
            .iter()
            .any(|phrase| text.contains(phrase))
}

/// How an assistant turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcome {
    /// Finished without an error.
    Completed,
    /// Ended on an error.
    Failed,
    /// Stopped by the person, or cut off by a crash or shutdown.
    Interrupted,
}

/// One finished assistant turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    /// The slug (`HarnessId::as_str`) of the harness the turn ran on.
    pub harness: String,
    /// Epoch millis the reply started.
    pub started_at: i64,
    /// Wall-clock length, when the transcript recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub outcome: TurnOutcome,
    /// The person's next message asked to correct this reply.
    #[serde(default)]
    pub reworked: bool,
    #[serde(default)]
    pub tool_calls: u32,
}

/// One session's usage, as read from its transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatUsage {
    pub category: TaskCategory,
    #[serde(default)]
    pub turns: Vec<TurnRecord>,
}

/// `GetUsageInsights` params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageInsightsRequest {
    /// Only turns that started at or after this epoch millis; `None` = all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<i64>,
}

/// Counts over a set of turns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTally {
    /// Sessions with at least one counted turn.
    pub sessions: u32,
    pub turns: u32,
    pub completed: u32,
    pub failed: u32,
    pub interrupted: u32,
    pub reworked: u32,
    pub tool_calls: u32,
    /// Sum of the recorded turn durations; turns without one add nothing.
    pub total_duration_ms: i64,
    /// Median recorded duration of the completed turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_duration_ms: Option<i64>,
}

impl UsageTally {
    /// Completed turns over all turns.
    pub fn success_rate(&self) -> Option<f32> {
        (self.turns > 0).then(|| self.completed as f32 / self.turns as f32)
    }

    /// Reworked turns over completed turns.
    pub fn rework_rate(&self) -> Option<f32> {
        (self.completed > 0).then(|| self.reworked as f32 / self.completed as f32)
    }

    /// Success weighted by how often a success held up: what
    /// [`CategoryUsage::best_harness`] ranks by.
    pub fn score(&self) -> Option<f32> {
        Some(self.success_rate()? * (1.0 - self.rework_rate().unwrap_or(0.0)))
    }
}

/// One harness's counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessUsage {
    /// Harness slug (`HarnessId::as_str`).
    pub harness: String,
    #[serde(flatten)]
    pub tally: UsageTally,
}

/// One task type's counts, overall and per harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub category: TaskCategory,
    #[serde(flatten)]
    pub tally: UsageTally,
    /// Most turns first.
    pub harnesses: Vec<HarnessUsage>,
    /// The harness with the best [`UsageTally::score`] among those with at
    /// least [`MIN_SAMPLE_TURNS`] turns; `None` unless two or more qualify.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_harness: Option<String>,
}

/// `GetUsageInsights` reply.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageInsightsReport {
    /// Epoch millis the report was computed.
    pub generated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<i64>,
    /// Some sessions have not been read yet: ask again for the rest.
    pub indexing: bool,
    pub total: UsageTally,
    /// Most turns first.
    pub harnesses: Vec<HarnessUsage>,
    /// Most sessions first; categories without a turn are left out.
    pub categories: Vec<CategoryUsage>,
}

/// Turns a harness needs in a category before it is compared there: one
/// lucky reply should not crown an agent.
pub const MIN_SAMPLE_TURNS: u32 = 5;

#[derive(Default)]
struct Accumulator {
    tally: UsageTally,
    durations: Vec<i64>,
}

impl Accumulator {
    fn add(&mut self, turn: &TurnRecord) {
        let tally = &mut self.tally;
        tally.turns += 1;
        tally.tool_calls += turn.tool_calls;
        match turn.outcome {
            TurnOutcome::Completed => {
                tally.completed += 1;
                if turn.reworked {
                    tally.reworked += 1;
                }
            }
            TurnOutcome::Failed => tally.failed += 1,
            TurnOutcome::Interrupted => tally.interrupted += 1,
        }
        if let Some(duration) = turn.duration_ms.filter(|d| *d >= 0) {
            tally.total_duration_ms += duration;
            if turn.outcome == TurnOutcome::Completed {
                self.durations.push(duration);
            }
        }
    }

    fn finish(mut self, sessions: u32) -> UsageTally {
        self.tally.sessions = sessions;
        self.tally.median_duration_ms = median(&mut self.durations);
        self.tally
    }
}

fn median(values: &mut [i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    })
}

/// Per-harness tallies from `(session index, turn)` pairs, most turns first.
fn by_harness<'a>(turns: impl Iterator<Item = (usize, &'a TurnRecord)>) -> Vec<HarnessUsage> {
    let mut groups: HashMap<&str, (Accumulator, HashSet<usize>)> = HashMap::new();
    for (session, turn) in turns {
        let (acc, sessions) = groups.entry(turn.harness.as_str()).or_default();
        acc.add(turn);
        sessions.insert(session);
    }
    let mut out: Vec<HarnessUsage> = groups
        .into_iter()
        .map(|(harness, (acc, sessions))| HarnessUsage {
            harness: harness.to_string(),
            tally: acc.finish(sessions.len() as u32),
        })
        .collect();
    out.sort_by(|a, b| {
        b.tally
            .turns
            .cmp(&a.tally.turns)
            .then_with(|| a.harness.cmp(&b.harness))
    });
    out
}

/// [`CategoryUsage::best_harness`] over a category's harnesses.
fn best_harness(harnesses: &[HarnessUsage]) -> Option<String> {
    let qualified: Vec<&HarnessUsage> = harnesses
        .iter()
        .filter(|h| h.tally.turns >= MIN_SAMPLE_TURNS)
        .collect();
    if qualified.len() < 2 {
        return None;
    }
    qualified
        .into_iter()
        .max_by(|a, b| {
            let (sa, sb) = (
                a.tally.score().unwrap_or(0.0),
                b.tally.score().unwrap_or(0.0),
            );
            sa.total_cmp(&sb)
                .then_with(|| a.tally.turns.cmp(&b.tally.turns))
        })
        .map(|h| h.harness.clone())
}

/// The report over `chats`, counting only turns that started at or after
/// `since_ms`. `generated_at` and `indexing` are left for the caller.
pub fn aggregate<'a>(
    chats: impl IntoIterator<Item = &'a ChatUsage>,
    since_ms: Option<i64>,
) -> UsageInsightsReport {
    let in_range = |turn: &&TurnRecord| since_ms.is_none_or(|since| turn.started_at >= since);
    let chats: Vec<(TaskCategory, Vec<&TurnRecord>)> = chats
        .into_iter()
        .map(|chat| (chat.category, chat.turns.iter().filter(in_range).collect()))
        .filter(|(_, turns): &(TaskCategory, Vec<&TurnRecord>)| !turns.is_empty())
        .collect();

    let mut total = Accumulator::default();
    for (_, turns) in &chats {
        for turn in turns {
            total.add(turn);
        }
    }
    let harnesses = by_harness(
        chats
            .iter()
            .enumerate()
            .flat_map(|(ix, (_, turns))| turns.iter().map(move |turn| (ix, *turn))),
    );

    let mut categories: Vec<CategoryUsage> = TaskCategory::ALL
        .into_iter()
        .filter_map(|category| {
            let members: Vec<(usize, &Vec<&TurnRecord>)> = chats
                .iter()
                .enumerate()
                .filter(|(_, (c, _))| *c == category)
                .map(|(ix, (_, turns))| (ix, turns))
                .collect();
            if members.is_empty() {
                return None;
            }
            let mut acc = Accumulator::default();
            for (_, turns) in &members {
                for turn in turns.iter() {
                    acc.add(turn);
                }
            }
            let harnesses = by_harness(
                members
                    .iter()
                    .flat_map(|(ix, turns)| turns.iter().map(move |turn| (*ix, *turn))),
            );
            Some(CategoryUsage {
                category,
                tally: acc.finish(members.len() as u32),
                best_harness: best_harness(&harnesses),
                harnesses,
            })
        })
        .collect();
    // Stable: equal session counts keep the category order.
    categories.sort_by(|a, b| b.tally.sessions.cmp(&a.tally.sessions));

    UsageInsightsReport {
        generated_at: 0,
        since_ms,
        indexing: false,
        total: total.finish(chats.len() as u32),
        harnesses,
        categories,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(harness: &str, outcome: TurnOutcome, reworked: bool, duration: i64) -> TurnRecord {
        TurnRecord {
            harness: harness.into(),
            started_at: 1_000,
            duration_ms: Some(duration),
            outcome,
            reworked,
            tool_calls: 2,
        }
    }

    #[test]
    fn classifies_english_and_chinese_requests() {
        use TaskCategory::*;
        for (prompt, expected) in [
            ("Fix the failing test in the parser", Bugfix),
            ("Add tests for the queue", Test),
            ("Implement dark mode in settings", Feature),
            ("Refactor the session store and simplify its API", Refactor),
            (
                "The sidebar is slow when scrolling; optimize it",
                Performance,
            ),
            ("Update the README with install steps", Docs),
            ("Please review this diff", Review),
            ("Deploy the edge worker and bump the release", Ops),
            ("Explain how the sync engine works", Explain),
            ("What does this module do?", Explain),
            ("你先搞清楚这个项目是做什么的", Explain),
            ("修复登录页面的报错", Bugfix),
            ("实现个人使用统计功能", Feature),
            ("给 parser 写单测", Test),
            ("重构一下这个模块，优化代码结构", Refactor),
            ("首页滚动卡顿，性能太差", Performance),
            ("hello there", Other),
        ] {
            assert_eq!(classify_task(prompt), expected, "{prompt}");
        }
    }

    #[test]
    fn words_match_whole_tokens_not_substrings() {
        // "address" is not "add", "latest" is not "test", "docker" is not "doc".
        assert_eq!(
            classify_task("update the address field"),
            TaskCategory::Other
        );
        assert_eq!(
            classify_task("use the latest docker image"),
            TaskCategory::Ops
        );
        // Stems still catch inflections.
        assert_eq!(classify_task("optimise startup"), TaskCategory::Performance);
    }

    #[test]
    fn corrections_are_rejections_or_reports_that_it_is_still_wrong() {
        for text in [
            "no, that's not it",
            "Nope",
            "It still fails on Windows",
            "doesn't work",
            "error: cannot find value",
            "还是不行",
            "不对，应该在左边",
            "又报错了",
            "没生效",
            "测试失败了",
            "please undo that change",
            "the build failed again",
        ] {
            assert!(is_correction(text), "{text}");
        }
        for text in [
            "great, now add docs",
            "Now implement the settings page",
            "nothing else",
            "notice the second file too",
            "now add a regression test",
            "add exception handling",
            "add undo support",
            "add a revert button",
            "no need for tests, just commit",
            "handle the case where the request fails",
            "加个报错提示",
            "处理失败的情况",
            "继续",
            "好的，提交吧",
            "",
        ] {
            assert!(!is_correction(text), "{text}");
        }
    }

    #[test]
    fn tallies_rates_and_medians() {
        let chats = vec![
            ChatUsage {
                category: TaskCategory::Bugfix,
                turns: vec![
                    turn("codex", TurnOutcome::Completed, true, 100),
                    turn("codex", TurnOutcome::Completed, false, 300),
                    turn("codex", TurnOutcome::Failed, false, 50),
                ],
            },
            ChatUsage {
                category: TaskCategory::Feature,
                turns: vec![
                    turn("claude-code", TurnOutcome::Completed, false, 200),
                    turn("claude-code", TurnOutcome::Interrupted, false, 10),
                ],
            },
            ChatUsage {
                category: TaskCategory::Feature,
                turns: vec![],
            },
        ];
        let report = aggregate(&chats, None);
        assert_eq!(report.total.sessions, 2);
        assert_eq!(report.total.turns, 5);
        assert_eq!(report.total.completed, 3);
        assert_eq!(report.total.failed, 1);
        assert_eq!(report.total.interrupted, 1);
        assert_eq!(report.total.reworked, 1);
        assert_eq!(report.total.tool_calls, 10);
        assert_eq!(report.total.total_duration_ms, 660);
        assert_eq!(report.total.median_duration_ms, Some(200));
        assert_eq!(report.total.success_rate(), Some(0.6));

        assert_eq!(report.harnesses[0].harness, "codex");
        let codex = &report.harnesses[0].tally;
        assert_eq!((codex.sessions, codex.turns), (1, 3));
        assert_eq!(codex.rework_rate(), Some(0.5));
        assert_eq!(codex.median_duration_ms, Some(200));

        assert_eq!(report.categories.len(), 2);
        assert_eq!(report.categories[0].category, TaskCategory::Bugfix);
        assert_eq!(report.categories[1].tally.sessions, 1);
    }

    #[test]
    fn since_filters_turns_and_drops_sessions_left_empty() {
        let mut old = turn("codex", TurnOutcome::Completed, false, 100);
        old.started_at = 10;
        let mut new = turn("codex", TurnOutcome::Completed, false, 100);
        new.started_at = 2_000;
        let chats = vec![
            ChatUsage {
                category: TaskCategory::Bugfix,
                turns: vec![old.clone()],
            },
            ChatUsage {
                category: TaskCategory::Docs,
                turns: vec![old, new],
            },
        ];
        let report = aggregate(&chats, Some(1_000));
        assert_eq!(report.since_ms, Some(1_000));
        assert_eq!((report.total.sessions, report.total.turns), (1, 1));
        assert_eq!(report.categories.len(), 1);
        assert_eq!(report.categories[0].category, TaskCategory::Docs);
    }

    #[test]
    fn best_harness_needs_two_agents_with_enough_turns() {
        let session = |harness: &str, completed: usize, reworked: usize, failed: usize| ChatUsage {
            category: TaskCategory::Bugfix,
            turns: (0..completed)
                .map(|ix| turn(harness, TurnOutcome::Completed, ix < reworked, 100))
                .chain((0..failed).map(|_| turn(harness, TurnOutcome::Failed, false, 100)))
                .collect(),
        };
        // Codex: 5/6 succeed, 1 of 5 reworked → 0.667. Claude: 6/6, 3 reworked → 0.5.
        let chats = vec![session("codex", 5, 1, 1), session("claude-code", 6, 3, 0)];
        let report = aggregate(&chats, None);
        assert_eq!(report.categories[0].best_harness.as_deref(), Some("codex"));

        // A second agent with too few turns leaves nothing to compare.
        let chats = vec![session("codex", 5, 1, 1), session("claude-code", 2, 0, 0)];
        assert_eq!(aggregate(&chats, None).categories[0].best_harness, None);
    }

    #[test]
    fn report_round_trips_with_flattened_tallies() {
        let chats = vec![ChatUsage {
            category: TaskCategory::Review,
            turns: vec![turn("codex", TurnOutcome::Completed, false, 100)],
        }];
        let report = aggregate(&chats, None);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["harnesses"][0]["harness"], "codex");
        assert_eq!(json["harnesses"][0]["turns"], 1);
        assert_eq!(json["categories"][0]["category"], "review");
        let back: UsageInsightsReport = serde_json::from_value(json).unwrap();
        assert_eq!(back, report);
    }
}
