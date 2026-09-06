---
name: host-lint
description: Lints phase-synonym agentic tells in commit messages, markdown headers, and code comments. Use when checking commits, reviewing code for LLM-slop, running pre-commit hooks, or when the user mentions linting, phase detection, commit hygiene, or slop.
---

Detects numbered phase-synonym patterns that signal LLM-generated plan language bleeding into commits, headers, and comments. Implements the rules in VOCABULARY.md.

## Install

Get the `host-lint` binary for your platform from the latest GitHub release, or build it with `cargo build --release`. Release assets cover linux, darwin, and windows on amd64 and arm64 (e.g. `host-lint-linux-amd64`, `host-lint-darwin-arm64`, `host-lint-windows-amd64.exe`):

```
# substitute your platform asset name
gh release download -R connollydavid/host-lint -p host-lint-linux-amd64 -O host-lint
chmod +x host-lint
```

To use as an agent skill in Claude Code, copy or symlink this directory into `.claude/skills/` of the consuming repo.

## Usage

### As a git hook

Copy the binary and `pre-commit` into `.git/hooks/` of the target repo. The script dispatches on its installed name, so install it as `pre-commit` (scans staged files), `commit-msg` (scans the commit message), or both:

```
cp host-lint .git/hooks/host-lint
cp pre-commit .git/hooks/pre-commit
cp pre-commit .git/hooks/commit-msg
chmod +x .git/hooks/host-lint .git/hooks/pre-commit .git/hooks/commit-msg
```

Installed as `commit-msg`, the script hands the staged diff to the `commit` verb when the binary carries it, and falls back to `--stdin` with a line naming the unrun check when it does not.

### As a CLI

```
# Scan commit message from stdin
echo "Phase 1: setup" | ./host-lint --stdin

# Check a commit message against the diff it describes
git diff --cached > staged.diff
./host-lint commit --message msg.txt --diff staged.diff

# Scan specific files
./host-lint README.md src/main.rs

# JSON output
./host-lint --json --stdin

# Scan all tracked files in a repo
./host-lint --all

# Scan every commit message in the repo's history
./host-lint --log
```

### Adoption / upgrade audit

Hooks only gate new commits, and rules grow over time, so run a one-shot audit when installing the skill into an existing repo or after upgrading the binary: `./host-lint --all` (fix flagged live files) and `./host-lint --log` (history findings — informational by default). If the user opts to clean history, guide the archive-then-rewrite flow from the README: create and push an archive branch preserving the original history, then `git commit --amend` (tip) or rebase/filter-repo (deeper) and force-push with lease. Link each replaced commit to its replacement with a `Superseded-by: <new-sha>` trailer via `git notes add` (push `refs/notes/commits` too) so the archive stays coherent. Never rewrite without archiving first or on branches the user does not control.

### Writing the message (guidance the tool does not check)

The tool checks the message's vocabulary, its prose shapes, and its duplication against the diff (VOCABULARY.md). The faults below recur in the same messages and have no rule; nothing enforces them, so apply them as you write:

- Describe the change; do not argue for it. If a paragraph reads as advocacy, move it to the record that owns the decision (an issue, a design record) and keep the message to what changed.
- Answer only points a reviewer raised. Cut any rebuttal of an alternative nobody proposed.
- Ground the change in this repository's own reasons. Precedent framing ("other projects already do this") matched nothing in accepted history (the out-of-scope note in VOCABULARY.md's commit-time duplication entry); when you reach for it, state the local reason instead.

### As an agent skill

Run the binary against the target and act on results:

1. Identify what to check (commit message, staged files, or specific files)
2. Run `host-lint` with appropriate flags
3. If exits 0, clean. If exits 1, a confirmed tell — report matches to the user. If exits 3, a bare-numeral warning (advisory) — reconsider the flagged numbered labels and rewrite them with descriptive names where it improves the text, but it does not block.

## Sanctioned vocabulary (LEXICON)

When the prose or strict lanes flag a phrase that is genuine project vocabulary — a domain term the trove catalog does not know — declare it so the masker clears exactly that phrase:

```
host-lint lexicon add "<phrase>" [--url <tracker-ref>]
```

A declaration is a report, not a settlement: it records that the shared grammar over-fired on this corpus, and it is owed upstream. The declaration covers the phrase only — it is not a waiver for the line or the file — and the three refusals hold: no bare-numeral master key, no declaring a phrase that is itself a tell, no declaration without a tracker ref. The full command reference lives in the README. The template carries no LEXICON: inherited spine text that trips a rule is reworded upstream, never declared away.

## Exit codes

- `0` — clean, no tells found
- `1` — one or more confirmed tells detected (blocks a commit hook)
- `2` — usage error or `git` failure
- `3` — warnings only, the advisory forms: the bare-numeral degenerate form (`5.5:`, `(5.5)`, `work-item 5.3`), an ordinal-scaffold heading, a message sentence restating a comment its diff adds. Advisory — a hook prints these and lets the commit through; an agent should reconsider them, not treat them as a hard stop.

## Portability notes

Released binaries cover linux (static musl), macOS, and windows on amd64 and arm64, with no runtime dependencies. For other platforms, build from source with `cargo build --release`.
