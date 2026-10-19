# Mobile Codex slash actions

The iOS and Android slash menus keep unavailable commands visible with a reason.
Codex commands that have a mobile action run locally when selected or submitted
without arguments. They never enter the chat's send queue or become model prompts.

| Commands | Mobile action |
| --- | --- |
| `/model` | Choose the model |
| `/new`, `/clear` | Open a new conversation |
| `/resume`, `/agents` | Search conversations |
| `/rename`, `/archive` | Rename or archive the current conversation |
| `/copy` | Copy the last completed assistant reply, excluding reasoning and tools |
| `/side`, `/btw` | Create an empty side chat from a main conversation |
| `/fork` | Fork through the last completed reply; a child forks as a sibling |
| `/theme` | Open the settings page containing appearance controls |

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
