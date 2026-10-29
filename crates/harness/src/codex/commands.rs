//! Versioned CLI command catalog and app-server mappings (codex-cli 0.161.0).
//! Keep terminal-only commands visible without sending them to the model.
use crate::HarnessError;
use serde_json::{Value, json};
use zeron_proto::SlashCommand;

#[derive(Clone, Copy)]
pub enum Delivery {
    Native,
    Desktop,
    Unavailable(&'static str),
}

pub struct Entry {
    pub name: &'static str,
    pub description: &'static str,
    pub hint: Option<&'static str>,
    pub delivery: Delivery,
}

const TERMINAL: &str = "Use this command in the Codex terminal UI.";
const CONFIG: &str = "Configure this in Codex CLI; this menu does not edit Codex configuration.";
const SESSION: &str = "Use Codex CLI for this operation on the native session.";

macro_rules! entry {
    ($name:literal, $description:literal, $delivery:expr) => {
        Entry {
            name: $name,
            description: $description,
            hint: None,
            delivery: $delivery,
        }
    };
}
use Delivery::{Desktop, Native, Unavailable};

pub const CATALOG: &[Entry] = &[
    entry!(
        "permissions",
        "Change approval and sandbox policy",
        Unavailable(
            "Zerun owns this session’s approval and sandbox policy; this Codex CLI picker is unavailable here."
        )
    ),
    entry!(
        "approvals",
        "Change approval policy",
        Unavailable(
            "Zerun owns this session’s approval policy; this Codex CLI picker is unavailable here."
        )
    ),
    entry!("ide", "Toggle IDE context", Unavailable(TERMINAL)),
    entry!("keymap", "Configure keyboard shortcuts", Desktop),
    entry!("vim", "Toggle terminal Vim editing", Unavailable(TERMINAL)),
    entry!(
        "setup-default-sandbox",
        "Configure the Windows elevated sandbox",
        Unavailable(CONFIG)
    ),
    entry!("agents", "Search and open conversations", Desktop),
    entry!(
        "subagents",
        "Open project files and the subagent panel",
        Desktop
    ),
    entry!("apps", "List available Codex apps", Native),
    entry!("plugins", "List Codex plugins", Native),
    entry!("hooks", "List lifecycle hooks", Native),
    entry!("clear", "Start a new conversation", Desktop),
    entry!("rename", "Rename this conversation", Desktop),
    entry!("archive", "Archive this conversation", Desktop),
    entry!("delete", "Delete the native session", Unavailable(SESSION)),
    entry!("compact", "Compact this conversation's context", Native),
    entry!(
        "copy",
        "Copy the latest completed assistant message",
        Desktop
    ),
    entry!("diff", "Open changes", Desktop),
    entry!("exit", "Quit Zerun", Desktop),
    entry!(
        "experimental",
        "Configure experimental features",
        Unavailable(CONFIG)
    ),
    entry!(
        "approve",
        "Approve a specific denied action",
        Unavailable("Use Codex CLI to review and approve the denied action.")
    ),
    entry!("memories", "View Codex memory status", Native),
    entry!("skills", "List skills in this project", Native),
    entry!(
        "import",
        "Import Claude Code or Cursor configuration",
        Unavailable(CONFIG)
    ),
    entry!("feedback", "Send feedback to OpenAI", Unavailable(TERMINAL)),
    entry!(
        "init",
        "Create or update project AGENTS.md instructions",
        Native
    ),
    entry!("logout", "Manage Codex accounts", Desktop),
    Entry {
        name: "mcp",
        description: "List MCP servers and tools",
        hint: Some("optional: verbose"),
        delivery: Native,
    },
    entry!("mention", "Browse project files", Desktop),
    entry!("model", "Choose model and reasoning", Desktop),
    entry!("fast", "Choose the model's service tier", Desktop),
    entry!("plan", "Configure Plan in Zerun permissions", Desktop),
    Entry {
        name: "goal",
        description: "Set, inspect, pause, resume, edit or clear a goal",
        hint: Some("[--budget tokens] objective | pause | resume | clear | edit objective"),
        delivery: Native,
    },
    entry!("ps", "List this session's background terminals", Native),
    entry!("stop", "Stop this session's background terminals", Native),
    entry!(
        "clean",
        "Stop background terminals (alias for /stop)",
        Native
    ),
    entry!("fork", "Fork this conversation", Desktop),
    entry!(
        "app",
        "Open the native session in the ChatGPT desktop app",
        Unavailable(SESSION)
    ),
    entry!("side", "Open a side chat", Desktop),
    entry!("btw", "Open a side chat (alias for /side)", Desktop),
    entry!(
        "raw",
        "Toggle raw terminal scrollback",
        Unavailable(TERMINAL)
    ),
    entry!("resume", "Search and open conversations", Desktop),
    entry!("new", "Start a new conversation", Desktop),
    entry!("quit", "Quit Zerun", Desktop),
    Entry {
        name: "review",
        description: "Review uncommitted changes, or supply instructions",
        hint: Some("optional instructions"),
        delivery: Native,
    },
    entry!("status", "View native session identity and status", Native),
    entry!("usage", "View account rate limits", Native),
    entry!(
        "debug-config",
        "Inspect effective Codex configuration",
        Unavailable(CONFIG)
    ),
    entry!(
        "statusline",
        "Configure the terminal status line",
        Unavailable(TERMINAL)
    ),
    entry!(
        "title",
        "Configure the terminal window title",
        Unavailable(TERMINAL)
    ),
    entry!("theme", "Open appearance settings", Desktop),
    entry!("pets", "Choose a terminal pet", Unavailable(TERMINAL)),
    entry!(
        "pet",
        "Choose a terminal pet (alias for /pets)",
        Unavailable(TERMINAL)
    ),
    entry!(
        "tui",
        "Choose the terminal display mode",
        Unavailable(TERMINAL)
    ),
    entry!(
        "export",
        "Export the native transcript",
        Unavailable(SESSION)
    ),
    entry!("recap", "Summarize this conversation", Native),
    entry!(
        "cd",
        "Change the native session working directory",
        Unavailable(
            "Create a Zerun conversation in the desired project; imported sessions retain their original directory."
        )
    ),
    entry!("pwd", "Show this session's working directory", Native),
    entry!("cwd", "Show working directory (alias for /pwd)", Native),
    entry!(
        "worktree",
        "Continue in a native worktree",
        Unavailable("Choose a project worktree when creating a Zerun conversation.")
    ),
    entry!(
        "daemon",
        "Manage the Codex background server",
        Unavailable(TERMINAL)
    ),
    entry!(
        "warnings",
        "Inspect Codex CLI startup warnings",
        Unavailable(TERMINAL)
    ),
];

pub fn catalog() -> Vec<SlashCommand> {
    CATALOG
        .iter()
        .map(|entry| SlashCommand {
            name: entry.name.into(),
            description: entry.description.into(),
            input_hint: entry.hint.map(str::to_owned),
            unavailable_reason: match entry.delivery {
                Unavailable(reason) => Some(reason.into()),
                Desktop => Some("Use this action in Zerun’s desktop / menu.".into()),
                Native => None,
            },
        })
        .collect()
}

pub fn entry(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.name == name)
}

fn invalid(message: impl Into<String>) -> HarnessError {
    HarnessError::Protocol(message.into())
}

pub fn request(
    name: &str,
    args: &str,
    thread: &str,
) -> Result<(&'static str, Value), HarnessError> {
    let Some(entry) = entry(name) else {
        return Err(invalid("Unknown Codex command"));
    };
    match entry.delivery {
        Unavailable(reason) => return Err(invalid(format!("/{name}: {reason}"))),
        Desktop => {
            return Err(invalid(format!(
                "/{name} is a desktop action. Select it from Zerun's / menu."
            )));
        }
        Native => {}
    }
    if name == "goal" {
        return goal_request(args, thread);
    }
    match name {
        "review" => {
            return Ok((
                "review/start",
                json!({"threadId":thread,"delivery":"inline","target":
            if args.is_empty() { json!({"type":"uncommittedChanges"}) }
            else { json!({"type":"custom","instructions":args}) }}),
            ));
        }
        // These need the resolved model/project context from turn/start params.
        "mcp" if args == "verbose" => {
            return Ok(("mcpServerStatus/list", json!({"threadId":thread})));
        }
        _ => {}
    }
    if !args.is_empty() {
        return Err(invalid(format!("/{name} takes no arguments in Zerun.")));
    }
    Ok(match name {
        "compact" => ("thread/compact/start", json!({"threadId":thread})),
        "status" | "pwd" | "cwd" => (
            "thread/read",
            json!({"threadId":thread,"includeTurns":false}),
        ),
        "usage" => ("account/rateLimits/read", json!({})),
        "ps" => (
            "thread/backgroundTerminals/list",
            json!({"threadId":thread}),
        ),
        "stop" | "clean" => (
            "thread/backgroundTerminals/clean",
            json!({"threadId":thread}),
        ),
        "mcp" => ("mcpServerStatus/list", json!({"threadId":thread})),
        "apps" => ("app/list", json!({"threadId":thread})),
        "plugins" => ("plugin/list", json!({"forceRefetch":false})),
        "hooks" => ("hooks/list", json!({})),
        "skills" => ("skills/list", json!({})),
        "memories" => ("memory/status", json!({})),
        "init" => ("zerun/init", json!({})),
        "recap" => ("zerun/recap", json!({})),
        _ => return Err(invalid(format!("/{name} has no native mapping"))),
    })
}

fn goal_request(args: &str, thread: &str) -> Result<(&'static str, Value), HarnessError> {
    let mut params = json!({"threadId":thread,"origin":"user"});
    match args {
        "" | "status" => return Ok(("thread/goal/get", json!({"threadId":thread}))),
        "clear" => return Ok(("thread/goal/clear", params)),
        "pause" | "resume" => {
            params["status"] = if args == "pause" { "paused" } else { "active" }.into();
            return Ok(("thread/goal/set", params));
        }
        "edit" => return Err(invalid("Use /goal edit <objective> to revise the goal.")),
        _ => {}
    }
    let mut objective = args.strip_prefix("edit ").unwrap_or(args).trim();
    if let Some(rest) = objective.strip_prefix("--budget ") {
        let (budget, rest) = rest
            .split_once(char::is_whitespace)
            .ok_or_else(|| invalid("Use /goal --budget <positive tokens> <objective>."))?;
        let budget = budget
            .parse::<i64>()
            .ok()
            .filter(|budget| *budget > 0)
            .ok_or_else(|| invalid("The goal token budget must be a positive integer."))?;
        params["tokenBudget"] = budget.into();
        objective = rest.trim();
    }
    if objective.is_empty() || objective.chars().count() > 4000 {
        return Err(invalid("A goal objective must contain 1–4000 characters."));
    }
    params["objective"] = objective.into();
    params["status"] = "active".into();
    Ok(("thread/goal/set", params))
}

pub(super) enum Started {
    Turn(String),
    Reply(String),
}

pub(super) async fn execute(
    client: &crate::jsonrpc::RpcClient,
    method: &str,
    mut arguments: Value,
    mut turn: Value,
) -> Result<Started, HarnessError> {
    let decoded = zeron_proto::invocation::invocation_prompt(
        turn["input"][0]["text"].as_str().unwrap_or_default(),
    );
    let name = zeron_proto::invocation::leading_command(&decoded)
        .map(|(name, _)| name)
        .unwrap_or_default();
    if matches!(method, "zerun/init" | "zerun/recap") {
        turn["input"] = json!([{"type":"text","text":if method == "zerun/init" {
            "Inspect this project and create or update AGENTS.md with concise, accurate instructions for coding agents. Preserve existing useful instructions."
        } else {
            "Summarize this conversation's objective, completed work, decisions, and remaining tasks. Do not perform further work in this turn."
        }}]);
        let response = client.request("turn/start", turn).await?;
        return Ok(Started::Turn(
            response["turn"]["id"].as_str().unwrap_or_default().into(),
        ));
    }
    if matches!(method, "skills/list" | "hooks/list" | "plugin/list") {
        if let Some(cwd) = turn["cwd"].as_str() {
            arguments["cwds"] = json!([cwd]);
        }
    }
    let mut response = client.request(method, arguments.clone()).await?;
    if matches!(
        method,
        "mcpServerStatus/list" | "app/list" | "thread/backgroundTerminals/list"
    ) {
        let mut cursors = std::collections::HashSet::new();
        while let Some(cursor) = response["nextCursor"].as_str().map(str::to_owned) {
            if cursors.len() >= 100 || !cursors.insert(cursor.clone()) {
                return Err(invalid(
                    "Codex repeated or exceeded the catalog page limit.",
                ));
            }
            arguments["cursor"] = cursor.into();
            let next = client.request(method, arguments.clone()).await?;
            let items = next["data"]
                .as_array()
                .ok_or_else(|| invalid("Invalid Codex catalog page."))?;
            response["data"]
                .as_array_mut()
                .ok_or_else(|| invalid("Invalid Codex catalog page."))?
                .extend(items.iter().cloned());
            response["nextCursor"] = next["nextCursor"].clone();
        }
    }
    if matches!(
        method,
        "turn/start" | "thread/compact/start" | "review/start"
    ) {
        return Ok(Started::Turn(
            response["turn"]["id"].as_str().unwrap_or_default().into(),
        ));
    }
    Ok(Started::Reply(reply(name, method, &response)))
}

fn reply(name: &str, method: &str, value: &Value) -> String {
    if method == "thread/goal/clear" {
        return "Goal cleared.".into();
    }
    if method.starts_with("thread/goal/") {
        let goal = &value["goal"];
        if goal.is_null() {
            return "No goal is set for this conversation.".into();
        }
        let budget = goal["tokenBudget"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "not set".into());
        return format!(
            "Goal: {}\nStatus: {}\nTokens used: {} · Budget: {}\nTime used: {} seconds",
            goal["objective"].as_str().unwrap_or_default(),
            goal["status"].as_str().unwrap_or("unknown"),
            goal["tokensUsed"].as_i64().unwrap_or(0),
            budget,
            goal["timeUsedSeconds"].as_i64().unwrap_or(0)
        );
    }
    if method == "thread/backgroundTerminals/clean" {
        return "Background terminals stopped.".into();
    }
    if method == "thread/read" {
        let thread = &value["thread"];
        if matches!(name, "pwd" | "cwd") {
            return thread["cwd"]
                .as_str()
                .unwrap_or("Working directory unavailable.")
                .into();
        }
        return format!(
            "Session: {}\nName: {}\nDirectory: {}\nStatus: {}",
            thread["id"].as_str().unwrap_or_default(),
            thread["name"].as_str().unwrap_or("Untitled"),
            thread["cwd"].as_str().unwrap_or_default(),
            thread["status"]["type"].as_str().unwrap_or("unknown")
        );
    }
    if method == "account/rateLimits/read" || method == "memory/status" {
        // These response schemas contain counters/status, not config or credentials.
        return format!(
            "{}\n{}",
            if method == "memory/status" {
                "Memory status"
            } else {
                "Account usage"
            },
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }
    if method == "thread/backgroundTerminals/list" {
        let rows = value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|process| {
                format!(
                    "- {}: {}",
                    process["processId"].as_str().unwrap_or("unknown"),
                    process["command"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        return if rows.is_empty() {
            "No background terminals.".into()
        } else {
            rows.join("\n")
        };
    }
    let mut rows = Vec::new();
    collect_names(value, &mut rows);
    rows.sort();
    rows.dedup();
    if rows.is_empty() {
        format!("/{name}: no entries reported.")
    } else {
        format!("/{name}\n{}", rows.join("\n"))
    }
}

// List only public labels, never arbitrary config maps, commands, or credentials.
fn collect_names(value: &Value, rows: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_names(item, rows);
            }
        }
        Value::Object(object) => {
            if let Some(name) = object
                .get("name")
                .or_else(|| object.get("title"))
                .or_else(|| object.get("id"))
                .or_else(|| object.get("eventName"))
                .and_then(Value::as_str)
            {
                rows.push(format!("- {name}"));
            }
            for key in [
                "data",
                "apps",
                "plugins",
                "marketplaces",
                "hooks",
                "skills",
                "tools",
                "servers",
            ] {
                if let Some(items) = object.get(key) {
                    if key == "tools" {
                        if let Some(tools) = items.as_object() {
                            for tool in tools.values() {
                                collect_names(tool, rows);
                            }
                        }
                    } else {
                        collect_names(items, rows);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_cli_command_is_visible_and_classified() {
        let documented = "permissions ide keymap vim setup-default-sandbox agents subagents apps plugins hooks clear rename archive delete compact copy diff exit experimental approve memories skills import feedback init logout mcp mention model fast plan goal ps stop fork app side btw raw resume new quit review status usage debug-config statusline title theme pets pet tui export recap cd pwd cwd worktree daemon warnings";
        let rows = catalog();
        let unique: std::collections::HashSet<_> =
            rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(unique.len(), rows.len());
        for name in documented.split_whitespace() {
            assert!(unique.contains(name), "missing /{name}");
        }
        for row in rows {
            if !matches!(entry(&row.name).unwrap().delivery, Native) {
                assert!(row.unavailable_reason.is_some());
                assert!(request(&row.name, "", "thread").is_err());
            }
        }
    }

    #[test]
    fn goal_mutations_are_explicit_and_preserve_usage_and_unspecified_budgets() {
        for (arg, status) in [("pause", "paused"), ("resume", "active")] {
            let (method, p) = request("goal", arg, "thread").unwrap();
            assert_eq!(method, "thread/goal/set");
            assert_eq!(p["origin"], "user");
            assert_eq!(p["status"], status);
            assert!(p.get("objective").is_none());
            assert!(p.get("tokenBudget").is_none());
        }
        let (method, p) = request("goal", "ship the feature", "thread").unwrap();
        assert_eq!(method, "thread/goal/set");
        assert_eq!(p["objective"], "ship the feature");
        assert!(p.get("tokenBudget").is_none());
        assert_eq!(
            request("goal", "--budget 4000 ship it", "thread")
                .unwrap()
                .1["tokenBudget"],
            4000
        );
        assert_eq!(
            request("goal", "edit revised objective", "thread")
                .unwrap()
                .1["objective"],
            "revised objective"
        );
        assert_eq!(request("goal", "", "thread").unwrap().0, "thread/goal/get");
        assert_eq!(
            request("goal", "clear", "thread").unwrap().0,
            "thread/goal/clear"
        );
        for invalid in [
            "edit",
            "--budget 0 ship",
            "--budget -1 ship",
            "--budget invalid ship",
            "--budget 40",
        ] {
            assert!(request("goal", invalid, "thread").is_err(), "{invalid}");
        }
        assert!(request("goal", &"x".repeat(4001), "thread").is_err());
    }
}
