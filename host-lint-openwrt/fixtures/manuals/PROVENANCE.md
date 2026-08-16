# Provenance of the vendored style manuals

These four files are the pack's sources. They are vendored rather than fetched, because they are
Claude artifacts rather than files in a tree the pack can reach, and a checker whose corpus lives
behind a network call cannot run offline or prove what it was checked against. The registry pins
the vendored copies by whole-file digest, so a manual edited without re-encoding the rules it
states reddens the drift test rather than silently changing what the pack enforces.

They are read-only inputs. Do not edit them to fix a rule; edit them only to re-import a newer
revision of the same artifact, and re-record the digest and the read date in the same commit.

| File | Artifact | Read | Measured against |
|---|---|---|---|
| `openwrt-description-style.md` | `ce7751ab-8a1a-4cfc-9151-90211f3b5cf1` | 2026-08-16 | `1d40ad929a` on `master`, dated 2026-06-07; apk 3.0.5 resolver behaviour, 2026-08-14 |
| `openwrt-comment-style.md` | `4001c454-7f67-4a37-a839-24c1e50f5a48` | 2026-08-10 | `1d40ad929a` on `master`, dated 2026-06-07 |
| `openwrt-package-commit-style.md` | `23ea5e7b-ca74-4007-b3ee-c8514f110ba4` | 2026-08-10 | full history of openwrt/packages, 27197 non-merge commits |
| `openwrt-pr-style.md` | `1e7cb641-b46b-4daa-8fae-79506a8c47d7` | 2026-08-10 | 5039 merged pull requests, and a targeted sample of 683 |

Digests as vendored:

```
8bc8f3c53001f369aca1cd710ded4072c24907c9aa09ae3c7c6078e511c52103  openwrt-description-style.md
661a0ae31c98d167d8b729247f786debe8ea90155ab05095e73f302b83971611  openwrt-comment-style.md
1106c598e5c24f2367b4ddc62d454bd7088bc50ab84508e33761aa154e725229  openwrt-package-commit-style.md
0fff7d6b21b84949a67e6c7ab722bc31d45f72281c4a54d3ba56fb77db40db84  openwrt-pr-style.md
```

## Three measurement bases, not one

The four manuals do not rest on the same evidence, and their shares are not interchangeable.

- **Tree state.** The description and comment manuals were measured at one commit, so their
  shares describe the files that survive in the feed today.
- **Commit history.** The commit manual was measured over 27197 non-merge commits across thirteen
  years, so its shares describe what authors did at the time, including in files since deleted.
- **The forge.** The pull request manual was measured over merged pull requests read from the
  GitHub API. Its subject is not in the repository at all: a pull request body lives on GitHub,
  and nothing in a clone records it.

That third basis is the one to keep in view. A checker can read a Makefile, a comment and a commit
message straight from a working tree, but it cannot read a pull request body without being handed
one. The `pr` lane therefore takes its input from an argument or standard input, and it can never
run as a repository sweep the way the other three can.

Where a manual reports a recent column it is reporting a trend, and the pack follows the recent
figure, because a rule calibrated on the whole corpus would encode a convention the project has
already left. This matters most for the pull request template, which did not exist in this form
before mid-2025: adoption runs 0.0% in 2024, 44.0% in 2025 and 71.6% in 2026. A rule keyed to the
template must not be tiered as though the whole corpus followed it, because for most of that
corpus the template was not there to follow.

## The one thing upstream does say

`.github/llm-review-rules.md:100` states that the description block is free-form prose with no
enforced convention. That is why these manuals are measured practice rather than policy, and it is
the reason no rule in this pack may be tiered as though upstream mandated it. Where that file does
speak, it agrees with the corpus: it says two spaces dominate the description block, and the count
confirms it.

The pull request manual is the exception that proves the shape. `.github/pull_request_template.md`
is a real file that upstream ships and GitHub prefills, so the rules keyed to its headings cite an
artefact rather than a measured habit. The rules about *filling* those headings remain measured:
a template body keeps the testing lines 95% of the time and gives them a value only about two
thirds of the time, and that gap is the whole reason those rules exist.

**That artefact is not vendored here, and must not be.** A file owned and moved by another project
is referenced and resolved at check time, never frozen into this directory: a vendored template
would be wrong the moment upstream edited it, and wrong silently. A short-lived gitignored cache is
permitted; it is never the authority. The decision is recorded as
`call/0055-upstream-artefacts-are-referenced-not-embedded` in the agentic-host repository, which is
where this pack's milestones live; it is named rather than linked, because that record is in a
different repository from this one and a relative path would dangle for anyone who cloned host-lint
on its own.

The distinction this directory turns on: everything in it is a **rule source**, stating what the
pack enforces, and freezing a rule source is correct. An artefact the pack **checks a submission
against** is a different kind of input and gets the opposite treatment.

One consequence lands inside a file listed above. `openwrt-pr-style.md` quotes the template in
full in a fenced block, and that quote is vendored because the manual is. No checker may read
expected headings, labels or box wording out of that fence. It is illustration. Reading the
authority from it would embed the upstream artefact by the back door, through a file that looks
like the project's own, which is precisely the failure call/0055 exists to prevent.

## The exemplar fences

The commit manual's four exemplars are fenced ```host-lint:ignore``` because they quote real commit
subjects, and the prose audit would otherwise fire on the quoted text. Preserve those fences on any
re-import. They are load-bearing, not decoration.

The pull request manual's exemplar is fenced ```markdown``` and quotes a merged body verbatim,
including a trailing space after `@commodo` and a doubled blank line. Preserve those too. The
manual's own rules are measured against bodies exactly like this one, so normalising the quotation
would put a tidied artefact behind a figure drawn from untidy ones.
