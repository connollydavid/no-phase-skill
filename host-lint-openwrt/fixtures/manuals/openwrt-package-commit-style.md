# Writing a commit for an OpenWrt package

How to write the commit message for a change to a package in openwrt/packages. The measurements
these rules rest on are in [References](#references).

---

## The skeleton

```
foopkg: update to 1.2.3

One or two lines saying what changed and why, wrapped near 72 columns.

Signed-off-by: Your Name <you@example.org>
```

---

## Subject

Write `<package>: <what you did>`.

- Prefix with the bare package name. Write `foopkg:` where the directory is `net/foopkg`.
- Lowercase after the colon.
- No full stop.
- Keep it near 30 characters and under 72.
- Present tense. Say what the change does.

The four verbs that carry most of the tree are `update`, `add`, `fix` and `bump`. Prefer the one
that says what happened:

```
foopkg: update to 1.2.3
foopkg: add new package
foopkg: fix build on 6.18.40
foopkg: remove obsolete patch
```

For a new package, `add new package` is the established wording. `add package` and `new package`
are both used and neither will be questioned.

---

## Body

Write one, unless the subject is the whole story.

- Say what changed and why, in one or two lines.
- Wrap near 72 columns.
- Leave a blank line after the subject.

A routine version bump often needs nothing beyond `Update to the latest upstream stable release.`,
or a `Changelog:` link. A fix should name the cause, not the symptom: what upstream changed, which
flag was wrong, which platform broke.

Do not restate the diff. The reader can see which lines moved; write down what they cannot see.

---

## Trailers

`Signed-off-by: Name <email>` is required on every commit, and CI rejects a commit without one.
The name must be a real name, and the address must not be a no-reply. The sign-off must match
the commit author.

Other trailers are used sparingly:

- `Fixes: <sha> ("<subject>")` when the change repairs an earlier commit. Give that commit's
hash and its subject in quotes, which most such lines in the tree do.
- `Co-authored-by:`, `Reported-by:`, `Suggested-by:` where someone else earned the credit.
- `(cherry picked from commit <sha>)` on a backport, which `git cherry-pick -x` writes for you.

Referencing a GitHub issue from a commit is rare in this tree, and it has a cost worth knowing
about: every push of a commit that names an issue posts a cross-reference on it, so a branch that
is amended and force-pushed while it is being reviewed leaves one entry per rewrite. Put the
closing keyword in the pull request body instead, where it fires once.

---

## Do not

**Repeat the package name in the body.** The subject already carries it.

**Write a subject that says nothing.** `foopkg: update`, `foopkg: fixes`, `foopkg: changes`. Say
what changed.

**End the subject with a full stop.**

**Capitalise after the colon.** Both forms appear in the tree, but lowercase is the majority and
is growing.

**Use the path as the prefix.** `utils/foopkg:` reads as a directory, and the tree uses the bare
package name.

**Bundle unrelated changes.** One package, one concern, one commit. A version bump that also
rewrites an init script should be two.

**Omit the sign-off.** It is the one trailer that is not optional.

---

## Exemplars

Quoted from the tree. Trailers are omitted.

```host-lint:ignore
libsml: add new package

libSML implements the Smart Message Language protocol used by German smart
meters (FNN specification). It is used by projects like volkszaehler for
reading smart meter data.
```

```host-lint:ignore
liblo: update to 0.36

Update to the latest upstream stable release.
```

```host-lint:ignore
ovpn-dco: fix build on 6.18.40

6.18.40 commit 073d95725269 changed the prototype of proto::recvmsg.
Update the ifdef.
```

```host-lint:ignore
perl: fix arch in powerpc64 config

The powerpc64.config file incorrectly sets arch=powerpc instead of
arch=powerpc64. This causes Perl to misidentify the architecture on
64-bit PowerPC targets.
```

Each names the cause and stops. None describes the diff.

---

## Checklist

- [ ] Subject reads `<package>: <what you did>`
- [ ] Lowercase after the colon, no full stop, under 72 columns
- [ ] Blank line, then a body saying what changed and why
- [ ] Body wrapped near 72 columns
- [ ] `Signed-off-by` matching the commit author, with a real name and a reachable address
- [ ] One concern in the commit
- [ ] Issue reference in the pull request body, not the commit

---

## References

Every figure below comes from the full history of openwrt/packages, read on 2026-08-10. The
corpus is 27197 non-merge commits. Its oldest commit is dated 2013-03-02 and its newest
2026-08-09. Re-derive these figures before citing them against a later tree.

Trailers are excluded when counting body prose, so "has prose" means text beyond
`Signed-off-by` and its siblings.

### Subject

A subject matches when it fits `^[^\s:]+: \S`, a prefix without spaces, a colon, a space and
text. The lowercase share is judged on the first character after the colon, over subjects with
text after a colon. Every other row is a share of all commits in its column.

| Property | All | 2024-2026 |
|---|---|---|
| Matches `<name>: <text>` | 97.9% | **99.5%** |
| No trailing full stop | 98.3% | **99.8%** |
| Lowercase after the colon | 76.6% | **86.9%** |
| Median length | 33 | 31 |

Across all commits, length p90 is 55 characters and only 2.4% exceed 72. The prefix contains a
slash in 3.2% of commits, so the bare package name is the norm by a wide margin.

First word after the colon, lowercased, as a share of all commits:

| Verb | Share |
|---|---|
| `update` | 38.0% |
| `add` | 10.9% |
| `fix` | 10.3% |
| `bump` | 7.8% |
| `remove` | 3.2% |
| `use` | 1.7% |

### Body

| Property | All | 2024-2026 |
|---|---|---|
| Carries prose beyond trailers | 61.0% | **83.2%** |
| Carries `Signed-off-by` | 98.4% | **99.8%** |

Among bodies that carry prose, the prose runs a median of 2 lines, p90 9. The widest prose line
has a median width of 66 characters, and 15.0% of these bodies have a line wider than 75.

The shift toward writing a body is the largest movement in the corpus.

| Commits added | Carry prose |
|---|---|
| 2013-2016 | 37.7% |
| 2017-2020 | 58.9% |
| 2021-2023 | 61.1% |
| 2024-2026 | 83.2% |

Commits without body prose are now the minority.

### Trailer frequency

A commit counts toward a row when its body has at least one line opening with that trailer.

| Trailer | Share of all commits |
|---|---|
| `Signed-off-by` | 98.4% |
| `Fixes` | 1.6% |
| `Link` | 0.5% |
| `Co-authored-by` | 0.3% |
| `Reported-by` | 0.2% |
| `Ref` | 0.2% |
| `Closes` | 0.1% |

Issue and commit references, counted as commits carrying the form: `Fixes: <sha>` 255,
`Fixes: <url>` 90, `Fixes: #N` 74, `Closes: #N` 14, `Closes: <url>` 6, plus a single commit
whose `Closes` names a foreign repository. So a `Closes` trailer of any spelling appears in 21
of 27197 commits, and the URL spelling is the rarest of the forms above. The `Fixes: <sha>` form
appears on 261 lines in total, and 188 of them append the fixed commit's subject in quotes.

### New packages

3155 commits open with `add` or `new` after the colon, counted case-insensitively. Of those,
63.0% carry body prose. The established subject wordings, counted case-insensitively as the
whole text after the colon, are `add new package` used 322 times, `add package` 217, and
`new package` 137.
