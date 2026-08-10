# Writing TITLE and description for an OpenWrt package

How to fill the `TITLE:=` and `define Package/<name>/description` fields of a package submitted
to openwrt/packages. The measurements these rules rest on are in [References](#references).

---

## The skeleton

```make
define Package/foo
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Short noun phrase, no full stop
  URL:=https://example.org/
  DEPENDS:=+libbar
endef

define Package/foo/description
  foo does the thing it does, stated in the present tense. A second
  sentence may add the one capability a reader would not assume.
endef
```

---

## TITLE

Write a noun phrase that labels the package in a menu.

- No full stop.
- Open with a capital.
- Three to seven words. Use one word when the name is the description, as `libjwt` does.
- Do not name OpenWrt. The reader knows where they are.
- Do not repeat the category. `SECTION` and `CATEGORY` already place it.

Good: `QuickJS JavaScript engine`, `Embedded Linux Library`,
`Fast and Lightweight Logs and Metrics processor`.

### Variants

Separate variants of one source with a lowercase parenthetical suffix, appended by `TITLE+=` off
a shared `Default` block. One variant may carry no suffix, which reads as the plain build.

```make
define Package/foo/Default
  TITLE:=Short noun phrase
endef

define Package/foo
  $(Package/foo/Default)
  TITLE+= (full)
endef

define Package/foo-lite
  $(Package/foo/Default)
endef
```

The tree's established suffixes: `(library)`, `(libraries)`, `(utilities)`, `(full)`, `(server)`,
`(client)`, `(daemon)`, `(tools)`, `(with SSL support)`, `(without SSL support)`. A negated form
such as `(no bridge/relay support)` also appears. Some packages append the same words without
parentheses; prefer the parenthesised form.

---

## description

Say what the software does, in the present tense, third person.

- Indent two spaces. Never a tab.
- One or two lines. Say what it is, then stop.
- Wrap under 80 columns.
- End with a full stop.
- Open with the software's name and a verb, as in `Fluent Bit is a super fast…`, or with a
bare noun phrase.

A single line is a complete description: `Timeout context manager for asyncio programs`.

---

## Do not

**Describe the build.** The description says what the software does, never how the package was
configured. No `CONFIG_*`, no `menuconfig`, no "built with".

```
lang/python/numpy
  NumPy is the fundamental package for array computing with Python.

  By default, this package is built without some modules.
  For some modules to be available, the INSTALL_GFORTRAN symbol needs
  to be enabled in the OpenWrt core/toolchain.
```

The first line is the description. The rest belongs in a Makefile comment, if anywhere.

**Put a version number in prose.** It stales at the next bump.

```
libs/boost
  This package provides the Boost v1.91.0 libraries.
```

**List features.** `libs/boost` continues into 31 bulleted library names, and `utils/mstflint`
into an indented "Package Contents:" tree. `DEPENDS` and the file list already carry this.

Four shorter prohibitions:

- **A URL in the body.** `URL:=` is the field for it. The one tolerated exception is a pointer to
a document explaining a term the description had to use, as `admin/earlyoom` does for
`oom_score`.
- **Naming OpenWrt.** `admin/zabbix` names it in three sibling descriptions.
- **Opening with "This package".** Name the software instead.
- **A tab, or a line past 80 columns.** `net/lftp` carries a 610-column line.

---

## Exemplars

Copy the prose, not the indentation, which varies across these seven and is governed by the rule
above.

```
lang/quickjs        TITLE:=QuickJS JavaScript engine
  QuickJS is a small and embeddable JavaScript engine. It supports the
  ES2023 specification including modules, asynchronous generators, proxies
  and BigInt.

admin/fluent-bit    TITLE:=Fast and Lightweight Logs and Metrics processor
  Fluent Bit is a super fast, lightweight, and highly scalable logging
  and metrics processor and forwarder.

libs/linenoise      TITLE:=A minimal, zero-config, readline replacement
 A minimal, zero-config, BSD licensed, readline replacement used in Redis,
 MongoDB, Android and many other projects.

libs/libsml         TITLE:=Smart Message Language (SML) library
 libSML implements the Smart Message Language (SML) protocol specified by
 VDE's Forum Netztechnik/Netzbetrieb (FNN). It can be used to communicate
 with SML-based smart meters and related components (EDL/MUC).

python3-dns         TITLE:=DNS toolkit
dnspython is a DNS toolkit for Python. It supports almost all record
types. It can be used for queries, zone transfers, and dynamic updates.
It supports TSIG authenticated messages and EDNS0.

python3-ifaddr      TITLE:=Network interface and IP address enumeration library
ifaddr is a small Python library that allows you to find all the
Ethernet and IP addresses of the computer.

syslog-ng           TITLE:=A powerful syslog daemon
  syslog-ng reads and logs messages to the system console, log
  files, other machines and/or users as specified by its
  configuration file.
```

The shape is consistent: name the software, verb, what it does, stop.

---

## Checklist

- [ ] TITLE is a noun phrase, capitalised, no full stop, three to seven words
- [ ] TITLE does not name OpenWrt or repeat the category
- [ ] Variants separated by `TITLE+=` and a lowercase parenthetical
- [ ] Description indented two spaces
- [ ] One or two lines, wrapped under 80 columns
- [ ] Present tense, describes the software
- [ ] Ends with a full stop
- [ ] No `CONFIG_*`, no version number, no feature inventory, no URL, no "This package…"

---

## References

Every figure below is as of `1d40ad929a` on `master`, dated 2026-06-07. Re-derive them before
citing them against a later tree.

`.github/llm-review-rules.md:100` states that the description block is "free-form prose" with
**no enforced convention**. There is no rule to cite in review, so the rules above are measured
practice rather than policy. Where that file does speak, the two agree: it says "two spaces
dominate" for this block, and the corpus confirms it.

### Corpus and method

The corpus is every package Makefile in the feed at that commit. 1431 of them carry a `TITLE`,
across 12 categories, and between them they hold 2018 `TITLE:=` statements and 1866 description
blocks of real prose. A block that is only a `$(call …)` indirection is dropped.
Addition dates come from the full git history, 36841 commits back to 2014-06-01. A renamed path
loses its date, so 1074 of the 1431 Makefiles can be dated. The recent column rests on the 46
Makefiles added in 2025 and 2026, 41 of which carry a description.

*All records* counts every `TITLE:=` line. *Deduped* counts one record per Makefile, because
families such as `gphoto2` with 66 titles, and `python3-*`, repeat a house style and would
otherwise vote many times. Where the two disagree the deduped number is the honest one, and the
dated columns are deduped.

Recent practice is treated as normative. Where a convention has moved, the rules follow the
newest column and treat the older form as legacy.

### TITLE

| Property | All records | Deduped | 2025-2026 |
|---|---|---|---|
| No trailing full stop | 97.9% | 97.4% | **100%** |
| Does not open lowercase | 84.7% | 84.1% | 89.1% |
| Never names OpenWrt | 99.8% | 99.8% | **100%** |
| Length | median 4 words | median 4 | median 4.5 |

42 of 2018 titles carry a full stop and none of the recent additions do. Four titles in the whole
tree name OpenWrt. Three to seven words covers 69.5% of deduped titles; one-word titles are 12.2%
of them.

### Variant suffixes

`TITLE+=` appears 545 times and 332 of those values carry a parenthetical. 209 Makefiles define
several packages off a shared `Default`, and 77 of them leave at least one variant unsuffixed.
`(full)` is used by 11 packages, among them `utils/flashrom`, and four of the eleven pair it with
an unsuffixed sibling.

| Suffix | Count | | Suffix | Count |
|---|---|---|---|---|
| `library` | 17 | | `(with SSL support)` | 7 |
| `(library)` | 14 | | `server` | 7 |
| `utilities` | 11 | | `(without SSL support)` | 6 |
| `(full)` | 11 | | `client` | 6 |
| `(utilities)` | 8 | | `(server)` | 6 |

### description

| Property | Deduped | 2014-2016 | 2025-2026 |
|---|---|---|---|
| Two-space indent | 52.5% | 40.1% | **87.8%** |
| Tab indent | 11.9% | 18.5% | **0.0%** |
| Ends with a full stop | 81.7% | 81.0% | 80.5% |
| Opens `This package…` | 3.6% | 4.3% | **0.0%** |
| Contains a URL | 2.3% | 2.5% | 2.4% |
| Line over 80 columns | 15.8% | 15.5% | 12.2% |
| Length | median 2 lines | median 3 | median 2 |

56% of deduped blocks are one or two lines. Single-line descriptions run a median of 9 words,
p90 16. The opening word is the package's own name in 32.1% of deduped blocks.

### The prohibitions

Five of the 1240 deduped blocks mention `CONFIG_*`, `menuconfig`, "built with", "built without"
or "this build", and every dated one predates 2020. Tabs and unwrapped lines are the two forms
that were once common and have since died out; the rest were always rare. Version numbers in
prose and feature inventories were not counted, and rest on the cited cases alone.
