# Mobile slash actions

The iOS and Android slash menus keep unavailable commands visible with a reason.
Commands supplied by the selected agent keep their native routing. Zerun also
provides the following local actions, which run when selected or submitted without
arguments. They never enter the chat's send queue or become model prompts.

| Other agents | Codex menu aliases | Mobile action |
| --- | --- | --- |
| `/zeron:model` | `/model` | Choose the model |
| `/zeron:new` | `/new`, `/clear` | Open a new conversation |
| `/zeron:resume` | `/resume`, `/agents` | Search conversations |
| `/zeron:rename` | `/rename` | Rename the current conversation |
| `/zeron:archive` | `/archive` | Archive the current conversation |
| `/zeron:copy` | `/copy` | Copy the last completed assistant reply, excluding reasoning and tools |
| `/zeron:side` | `/side`, `/btw` | Create an empty side chat from a main conversation |
| `/zeron:fork` | `/fork` | Fork through the last completed reply; a child forks as a sibling |
| `/zeron:theme` | `/theme` | Open the settings page containing appearance controls |

The `zeron:` namespace is reserved for Zerun actions, matching the desktop naming
convention. Local entries are labeled `Zerun:` in their descriptions. Other agents'
bare `/model`, `/fork`, `/name`, etc. are never mapped to these actions. Codex keeps
its existing aliases without duplicate menu rows; explicit `/zeron:` forms also
work when typed. Skill references retain their identities even with matching names.

Native menu sources remain agent-specific:

| Agent | Native commands |
| --- | --- |
| Claude Code | SDK `initialize` command list |
| Copilot, Devin, Grok, Hermes, Antigravity, Dsh | ACP session catalog and `available_commands_update` |
| OpenCode | HTTP command catalog and command endpoint |
| Pi | RPC command catalog and supported RPC control actions |
| OMP | Its RPC command catalog |
| Codex | Classified command catalog and supported app-server operations |
| Cursor | No native slash-command catalog exposed by the SDK; show Zerun actions and discovered skills |

The menu reflects the installed agent's advertised commands, including project
commands and skills. Zerun does not invent terminal-only command lists for agents
that do not expose one. Discovery warnings are retained alongside local actions.

Session-only actions remain disabled on the new-conversation page. Copy and fork
need a completed reply. Queue editing and an in-progress side-chat operation
retain their existing restrictions. Unsupported terminal/desktop actions remain
visible and disabled. Other agents' commands and skills keep their native routing.

Selecting an action consumes only the slash token and keeps the remaining draft,
attachments and selected references. Direct submission of an action with arguments
is rejected. Failed actions restore the token unless the user has since edited the
draft or changed context. Model selection, rename and other dialogs keep their
normal cancel behavior. Opening another page preserves drafts for returning.

The shared Rust resolver adds mobile UniFFI helpers; host RPCs and stored command
formats are unchanged. `/usage` still depends on Codex authentication, and plugin
catalog errors remain provider errors; this change does not override them.

Regression coverage checks shared resolution/catalog preservation across all
supported agents and both phone composers. Provider discovery and dispatch are
checked with protocol fixtures; this is not a live CLI smoke test of every agent.
