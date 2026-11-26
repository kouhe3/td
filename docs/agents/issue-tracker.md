# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues **in `kouhe3/td`** (the fork). Use the `gh` CLI for all operations.

## Repo pinning (read first)

This clone has two remotes: `origin` = `kouhe3/td` and `upstream` = `tdlib/td`. A fresh clone carries `remote.upstream.gh-resolved = base` inherited from the fork parent, so `gh repo view` resolves to **`tdlib/td`** — the parent project, where we must never file anything.

Fixed here with `gh repo set-default kouhe3/td` (writes `remote.origin.gh-resolved = base` into `.git/config`, verified: `gh repo view --json nameWithOwner` → `kouhe3/td`). That setting is **not versioned**, so run it again after every fresh clone.

- Every write takes `-R kouhe3/td`: `gh issue create -R kouhe3/td …`, `gh issue comment -R kouhe3/td <n> …`.
- Before the first write of a session, confirm the target: `gh repo view kouhe3/td --json nameWithOwner`.
- Never create issues, comments, labels or PRs on `tdlib/td`, and never push a branch there.

GitHub Issues were **disabled** on `kouhe3/td` until 2026-11-26; they are enabled now (`gh api repos/kouhe3/td --jq .has_issues` → `true`).

## Conventions

- **Create an issue**: `gh issue create -R kouhe3/td --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view -R kouhe3/td <number> --json number,title,body,labels,comments`.
- **List issues**: `gh issue list -R kouhe3/td --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Make an issue a sub-issue of a parent**: `gh issue create -R kouhe3/td --parent <parent> ...`, or `gh issue edit -R kouhe3/td <parent> --add-sub-issue <child>` (`gh` 2.94+). Without sub-issues, put `Part of #<parent>` at the top of the child body.
- **Blocking**: native dependencies via `gh issue create --blocked-by <n>,<n>` or `gh issue edit -R kouhe3/td <child> --add-blocked-by <blocker>`. Older `gh`: `gh api --method POST repos/kouhe3/td/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`, where the id is the blocker's numeric database id (`gh api repos/kouhe3/td/issues/<n> --jq .id`). Fallback: a `Blocked by: #<n>` line at the top of the child body.
- **Count open blockers**: `gh api repos/kouhe3/td/issues/<n> --jq .issue_dependencies_summary.blocked_by` (0 means startable).
- **Comment on an issue**: `gh issue comment -R kouhe3/td <number> --body "..."`
- **Apply / remove labels**: `gh issue edit -R kouhe3/td <number> --add-label "..."` / `--remove-label "..."`
- **Close**: post the explanation with `gh issue comment -R kouhe3/td <number> --body "..."`, then `gh issue close -R kouhe3/td <number>`.

## Pull requests as a triage surface

**PRs as a request surface: no.** _(Set to `yes` if this repo treats external PRs as feature requests; `/triage` reads this flag.)_

## When a skill says "publish to the issue tracker"

Create a GitHub issue in `kouhe3/td`.

## When a skill says "fetch the relevant ticket"

Read it as in **Read an issue** above.

## Relationship to the face-spaghetti ledger

`docs/facts.md` stays the single source of facts; issues reference fact ids (`依据：F003`). Deferred work that the ledger's `tasks/backlog.md` already carries (B1–B4) gets an issue when it becomes actionable, and the backlog row links the issue number.