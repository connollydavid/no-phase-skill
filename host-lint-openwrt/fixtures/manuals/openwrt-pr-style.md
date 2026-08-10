# Writing a pull request for openwrt/packages

How to write the body of a pull request against openwrt/packages. The commit message is a
separate discipline with its own rules. The measurements behind this one are in
[References](#references).

---

## Use the template

The repository ships `.github/pull_request_template.md`, and GitHub prefills it. Keep its
headings and fill its fields. Most merged pull requests now do, and nearly all recent
new-package submissions do, on a small sample.

```markdown
## 📦 Package Details

**Maintainer:** @your-github-user

**Description:**
One or two lines saying what the package does or what the change is.

---

## 🧪 Run Testing Details

- **OpenWrt Version:** SNAPSHOT
- **OpenWrt Target/Subtarget:** mediatek/filogic
- **OpenWrt Device:** bpi-r4-pro-8x

---

## ✅ Formalities

- [x] I have reviewed the CONTRIBUTING.md file for detailed contributing guidelines.
```

---

## The fields

**Maintainer.** The GitHub handle in `PKG_MAINTAINER`. For a new package that is you. Find it in
the Makefile history for an existing one.

**Description.** One or two lines. The commit message carries the detail, so this says what the
package does or what the change is and stops.

**Run Testing Details.** Name the version, the target and subtarget, and the device you ran it
on. Give a real target such as `mediatek/filogic` and a real device, not a placeholder. Say so
plainly when you tested in a container or under emulation rather than on hardware.

**Formalities.** Tick the CONTRIBUTING box. Delete the patch section when your change carries
no patch; most template bodies drop it.

---

## Do not

**Submit an empty body.** Almost nobody does, and it reads as a change nobody wants to explain.

**Leave the testing fields blank while keeping their headings.** An empty `OpenWrt Device:` says
less than deleting the line, and it happens often enough to be worth naming.

**Restate the commit message.** The reviewer reads both. Keep the description short and let the
commit carry the reasoning.

**Tick a checkbox that is not true.** The patch boxes ask whether the patch applies with
`git am`, whether it has been refreshed, and whether it is upstreamable. With no patch in the
change, delete the section rather than tick them.

**Argue the design in the body.** Say what the change is. A maintainer who wants the reasoning
will ask, and the reply belongs in the thread.

**Put the issue-closing keyword only in the commit.** GitHub posts a cross-reference on the
issue every time a commit naming it is pushed, so an amended branch leaves one entry per
rewrite. In the pull request body it fires once.

---

## Exemplar

Quoted in full from a merged pull request, the python-pexpect submission listed in References.

```markdown
## 📦 Package Details

**Maintainer:** me, @commodo 

**Description:**
Useful for provisioning devices like modems that present themselves via a serial device.


---

## 🧪 Run Testing Details

- **OpenWrt Version:** SNAPSHOT
- **OpenWrt Target/Subtarget:** mediatek/filogic
- **OpenWrt Device:** bpi-r4-pro-8x

---

## ✅ Formalities

- [X] I have reviewed the [CONTRIBUTING.md](https://github.com/openwrt/packages/blob/master/CONTRIBUTING.md) file for detailed contributing guidelines.
```

That is the whole shape: who maintains it, what it is, where it ran, and the box ticked.

---

## Checklist

- [ ] Template headings kept and fields filled
- [ ] Maintainer is a GitHub handle
- [ ] Description is one or two lines
- [ ] Version named, target and subtarget named, device named
- [ ] CONTRIBUTING box ticked
- [ ] Patch boxes ticked only when true, or the section deleted
- [ ] Issue-closing keyword in this body, not in the commit

---

## References

Two samples, both read on 2026-08-10. Re-derive these figures before citing them against a later
tree.

The wide sample is 5039 merged pull requests, the oldest merged 2023-12-11 and the newest
2026-08-09, gathered by paging the pulls endpoint. 82.6% target `master`, the rest release
branches. The targeted sample is 683 merged pull requests found by searching titles for the
new-package wordings, the oldest merged 2014-06-13 and the newest 2026-07-18.

The bases. A body uses the template when it contains `Run Testing Details`, a heading only the
current template carries. A checkbox is ticked when the body matches `[x]` or `[X]`. A field
line is kept when its label appears in the body, and its value is given when text follows the
label on that line; bold markers and the template's own placeholder do not count as a value.
Body line counts trim surrounding whitespace before counting. Title
wordings are counted from titles that end with the wording, case ignored.

### The template is recent

| Merged in | Sample | Uses the template | Median body lines |
|---|---|---|---|
| 2023 | 73 | 0.0% | 7 |
| 2024 | 1865 | 0.0% | 6 |
| 2025 | 1729 | 44.0% | 18 |
| 2026 | 1372 | 71.6% | 23 |

The template in this form landed in mid-2025 and replaced a plainer one the measure does not
count. A body written to the older habit is not wrong so much as dated, and adoption is still
climbing.

### What 2026 submissions do

| Property | All merged | New-package |
|---|---|---|
| Sample | 1372 | 16 |
| Uses the template | 71.6% | 93.8% |
| Ticks a checkbox | 61.3% | 81.2% |
| Gives a maintainer | 85.6% | 93.8% |
| Gives a device | 45.8% | 68.8% |
| Keeps the patch section | 23.0% | 18.8% |

Body length across all 2026 merges runs a median of 23 lines, p90 39.

The new-package column rests on 16 pull requests, so read it as a direction rather than a rate.

### Field completion

Among 2026 submissions that use the template:

| Field | Line kept | Value given |
|---|---|---|
| Maintainer | 97.6% | 95.2% |
| Target/Subtarget | 96.6% | 66.2% |
| Version | 96.3% | 66.7% |
| Device | 95.2% | 63.8% |

A template user keeps the lines and nearly always names the maintainer. Each testing value is
left blank in about a third of template bodies, the habit the rules above name.

### Empty bodies

0.2% of the wide sample has no body at all, and 0.7% of 2026 merges. Across the 683
new-package submissions the figure is 0.6%.

### Title wording

Among the 683 new-package pull requests: `add new package` 213, `add package` 125,
`new package` 91, `add a new package` 15.

### The exemplar

Pull request 29980, titled `python-pexpect: add package`, merged 2026-07-13.
