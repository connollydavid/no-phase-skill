# Writing comments in an OpenWrt package

How to comment the files a package contributes to openwrt/packages. The measurements these rules
rest on are in [References](#references).

---

## Whose voice is this file in

A package directory mixes two kinds of file, and only one of them is written in OpenWrt's voice.
Style rules apply to the first kind and never to the second.

**OpenWrt writes these.** `Makefile`, `Config.in`, everything under `files/`, and `test.sh` or
`test-version.sh`. Comments here follow the rules below.

**Upstream wrote this.** The body of any patch. Every line a patch adds is upstream's source in
upstream's style, so a comment inside a `+` line belongs to that project's conventions, not to
OpenWrt's. Do not restyle it, and do not read it as evidence of house style.

**A patch header is either.** Two kinds sit side by side in `patches/`.

- A patch that opens with `From <40-hex-sha>`, or carries `From:` and `Subject:`, is a
cherry-picked upstream commit produced by `git format-patch`. Its message is the upstream
author's. Leave it byte-for-byte. A trailing `Signed-off-by` stays with it.
- A patch with no such header was written locally for OpenWrt. Its description is yours to write,
and the rules below apply to it.

Most patches in the tree are the second kind. Read the header before you treat a patch as carrying
upstream prose.

Where a local patch needs to record where it came from, `Upstream-Status:` is the field the tree
uses, though few patches carry one.

---

## Do not comment

The tree's Makefiles are almost entirely uncommented, and that is the default to match. Write a
comment only when the line it sits above would otherwise read as a mistake.

Reach for one when:

- A flag, dependency or exclusion exists for a reason the line cannot show. `# Without libevent2 tests/async_queries sporadically fails on the bots`
- A workaround is temporary and someone should remove it later. `# -liconv due to glib2, to be revisited later`
- A value was derived from somewhere a reader cannot see. `# From go tool dist list`
- A construct looks wrong until you know the constraint. `# This is done instead of DEPENDS:=@!arc to avoid a recursive dependency when the library is conditionally selected by util/lxc`

Do not write one to say what the line already says.

---

## Form

- Put it on its own line, above what it explains. Never trail it after code.
- Start at column 0, even when the line below is indented inside a block.
- Lowercase opening, no closing full stop.
- One line. Say the reason and stop.
- Present tense.

```make
# needed otherwise errors during compile
MAKE_FLAGS:=
```

Wrap onto a second line only when the reason genuinely needs it, and keep both lines at column 0.

---

## The licence header

A package Makefile may open with a comment header, and many carry none at all. Neither choice will
be questioned. When you write one, the established form is:

```make
#
# Copyright (C) 2026 Your Name
#
# This is free software, licensed under the GNU General Public License v2.
# See /LICENSE for more information.
#
```

An `SPDX-License-Identifier:` line appears in a small minority. A header is boilerplate and carries
no information about the package, so it is exempt from the brevity rules above.

---

## Exemplars

Each of these sits directly above the line it explains.

```make
# Without libevent2 tests/async_queries sporadically fails on the bots
PKG_BUILD_DEPENDS:=libevent2 mariadb/host

# -liconv due to glib2, to be revisited later
include $(INCLUDE_DIR)/nls.mk

# Reset golang-package.mk overrides so we can use the Makefile
Build/Compile=$(call Build/Compile/Default)

# From go tool dist list
HOST_GO_VALID_OS_ARCH:= \

# MacOS bash is too old for shorewall6, use OpenWrt host tools/bash built for macos hosts
# use fakeuname to avoid 'if `uname` is Darwin' checks
MACOS_ENV := \

# shown in LuCI package description
define Package/privoxy/description
```

Every one answers why. None describes what the line does.

---

## Do not

**Trail a comment after code.** The whole tree does this 16 times against 7239 own-line comments,
and in a Makefile the text before the `#` keeps its trailing whitespace.

**Leave commented-out code behind.** Delete it. Git has it.

**Restate the line below.** A comment reading `# set the version` above `PKG_VERSION:=1.5` costs a
line and tells a reader nothing.

**Restyle a comment inside a patch body.** It is upstream's source.

**Rewrite the message of a backported patch.** The header identifies its author, and editing it
misattributes their words.

---

## Checklist

- [ ] The file is one OpenWrt writes, so these rules apply
- [ ] The comment explains why, not what
- [ ] Own line, above the code, at column 0
- [ ] Lowercase, no closing full stop, one line
- [ ] No trailing inline comment
- [ ] No commented-out code
- [ ] Any backported patch header left byte-for-byte

---

## References

Every figure below is as of `1d40ad929a` on `master`, dated 2026-06-07. Re-derive them before
citing them against a later tree.

### Corpus

The tree holds 1446 package Makefiles, 1312 patch files, 486 shell scripts under `files/`, and 320
test scripts. Comments were read from Makefiles, `Config.in`, and the shell under `files/`, with
everything inside a `patches/` directory held separately. The package under review is excluded
throughout. Addition dates come from git, so commenting rates can be split by when a package
entered the tree.

Classifying a comment as prose rather than commented-out code is a heuristic, so the prose counts
carry a small error either way.

### The provenance split

Of 1312 patch files:

| Signal | Count | Share |
|---|---|---|
| No `From:` and no `Subject:`, written locally | 856 | 65.2% |
| Carries `Subject:` | 455 | 34.7% |
| Carries `From:` | 449 | 34.2% |
| Opens `From <40-hex-sha>`, the `git format-patch` shape | 413 | 31.5% |
| Carries `Signed-off-by` | 240 | 18.3% |
| Declares `Upstream-Status` | 24 | 1.8% |

Roughly a third of the tree's patches are cherry-picked upstream commits and about two thirds were
written locally. Directory alone therefore cannot decide provenance, and the header has to be read.

### How much the tree comments

7239 comment lines sit in package Makefiles, and 5968 of those are inside the first eight lines,
where the licence header lives. That leaves 1271 body comments, of which 1043 are prose rather
than commented-out code or a separator. They fall in 248 Makefiles.

| Property | Value |
|---|---|
| Makefiles carrying any prose body comment | 248 of 1446 |
| Comments per commented Makefile | median 2, max 39 |
| Own-line against trailing inline | 7239 against 16 |
| At column 0 | 83.1% |
| Opens with a capital | 34.6% |
| Ends with a full stop | 10.9% |
| Length | median 6 words, p90 12 |

Commenting is not a habit the tree is losing or gaining.

| Added | Carry a body comment |
|---|---|
| 2014-2018 | 145 of 942, 15.4% |
| 2019-2022 | 53 of 407, 13.0% |
| 2023-2026 | 25 of 188, 13.3% |

### Genres inside the body comments

Of the 1271 body comments, 6.2% are commented-out make code, 1.6% carry a URL, 1.2% name a section
separator, 1.2% say TODO or FIXME or XXX, and 0.6% cite an issue or pull request. None cites a CVE.

### Shell under `files/`

5251 prose comments across 573 scripts, in the same register as the Makefiles: 41.4% open with a
capital, 11.0% end with a full stop, median 6 words and p90 12.

### The licence header

310 of 1446 Makefiles open with no comment at all. Among those that do, the most common exact form
is the GPL-2 pair used by 109 Makefiles, the same pair above a `Copyright (C)` line by 60, and 52
carry an `SPDX-License-Identifier`.
