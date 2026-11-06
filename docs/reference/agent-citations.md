# Agent citations

File references in chat open through the linking session's host and working
folder. A reference to another device is never read from the phone's or viewing
computer's filesystem. Existing host-file access confirmation still applies.

The shared desktop and mobile Markdown parser understands:

- Codex file directives, such as
  `:codex-file-citation{path="/project/docs/spec.pdf" purpose="source"}`.
  The displayed label is the filename; the target retains the full path.
- Path-bearing citations such as `【F:src/main.rs†L12-L20】`.
- Explicit `src/main.rs:12`, `src/main.rs:12:5`, and `src/main.rs:12-20`
  references, including inline code spans used by Claude Code and other
  terminal agents. Paths containing spaces should be enclosed in backticks or
  provided as an explicit citation or Markdown link. Bare filenames need a
  recognized source/document extension so host:port text is not mislinked.
- Cursor-style code-reference fences whose info string is
  `12:20:src/main.rs`. The source link appears before the unchanged code excerpt.
- Ordinary Markdown file and web links from any harness.

ACP agents' `resource_link` blocks and embedded resources with navigable URIs
are retained as links in both live messages and imported histories. Claude's
structured URL citations are also retained in live, subagent and imported
messages; a final message does not repeat citations already emitted by its
stream.

References do not fetch previews automatically. Only explicit file targets and
HTTP(S) sources are linked. Opaque citation IDs without a supplied destination
cannot be resolved by guessing a path or URL. File references follow the existing
file-link grammar, including its traversal and path-format restrictions.
Fenced code examples, existing Markdown links, and malformed or incomplete
citation directives are not rewritten. The raw conversation remains unchanged;
rendering preserves source offsets for streaming and task-checkbox edits.
