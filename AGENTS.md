# Repository Collaboration Guidelines

## Documentation and Public Materials

- Documentation is for users and contributors. Include only information needed
  to use, develop, or maintain the project. Describe verification results in the
  delivery response; do not commit acceptance reports, evidence summaries, or
  operation logs.
- Before committing or releasing, ensure that local usernames, directories,
  workspace paths, SDK/AVD configuration, credential locations, personal
  conventions, logs, and screenshots remain exclusively in local, untracked
  materials. Public build instructions must use generic requirements and
  placeholder paths.

## Upstream References and Notifications

- Commits, PRs/issues, comments, documentation, release notes, and responses must
  not mention or reference upstream pull requests by number, link, or any other
  identifying information, create cross-repository references, or @mention
  upstream authors. GitHub references may create associations and trigger
  notifications.
- Do not submit PRs, issues, or comments to upstream, or contact its authors.
  When importing code, describe only this repository's problems, implementation,
  behavior changes, and verification results.
- Before importing, committing, or releasing, remove upstream pull request
  references and author mentions. Cherry-picks and similar operations must not
  retain noncompliant commit messages. This rule takes precedence over any
  source-reference guidance in `FORK.md` or other files.
