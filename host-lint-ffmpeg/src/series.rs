//! The series lane (host-lint#22, series-lane).
//!
//! Checks that only make sense over several commits in order: whether a consumer
//! lands before its provider, whether a new registration brought its obligations with
//! it, whether a fix meant for backporting stayed focused.
//!
//! The generated-header allowlist is the part that decides whether this lane is
//! usable at all. `config_components.h` and its siblings do not exist in the tree —
//! `configure` writes them — so a provider-before-consumer check that demanded to
//! find their source would flag every series that includes one.

use crate::cosmetic;
use crate::rules::Tier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub tier: Tier,
    pub commit: String,
    pub detail: String,
}

/// One commit in the series, in order.
#[derive(Debug, Clone)]
pub struct Commit {
    pub id: String,
    pub subject: String,
    pub body: String,
    /// Paths this commit adds.
    pub added_paths: Vec<String>,
    /// Paths this commit modifies or deletes.
    pub touched_paths: Vec<String>,
    /// The unified diff, for the checks that read content.
    pub diff: String,
}

/// Headers `configure` generates. They are never in the tree, so a series including
/// one must not be told its provider is missing.
const GENERATED_HEADERS: &[&str] = &[
    "config.h",
    "config_components.h",
    "libavutil/avconfig.h",
    "libavutil/ffversion.h",
];

pub fn is_generated_header(path: &str) -> bool {
    GENERATED_HEADERS.iter().any(|g| path.ends_with(g))
}

/// Includes a commit adds, in the order they appear.
fn added_includes(diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .filter_map(|l| {
            let t = l[1..].trim();
            let rest = t.strip_prefix("#include")?.trim();
            let inner = rest
                .strip_prefix('"')
                .and_then(|r| r.split('"').next())
                .or_else(|| rest.strip_prefix('<').and_then(|r| r.split('>').next()))?;
            Some(inner.to_string())
        })
        .collect()
}

/// Object files a commit adds to a Makefile, e.g. `OBJS-$(CONFIG_FOO) += foo.o`.
fn added_makefile_objects(diff: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_makefile = false;
    for line in diff.lines() {
        if let Some(p) = line.strip_prefix("+++ b/") {
            in_makefile = p.ends_with("Makefile") || p.ends_with(".mak");
            continue;
        }
        if !in_makefile || !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        for tok in line[1..].split_whitespace() {
            if let Some(stem) = tok.strip_suffix(".o") {
                out.push(format!("{stem}.c"));
            }
        }
    }
    out
}

/// Whether the series provides a path by the time `upto` is reached, counting both
/// roots: an include may resolve relative to the library directory or to the tree.
fn provided_by(commits: &[Commit], upto: usize, needle: &str) -> bool {
    let base = needle.rsplit('/').next().unwrap_or(needle);
    commits[..=upto].iter().any(|c| {
        c.added_paths
            .iter()
            .chain(c.touched_paths.iter())
            .any(|p| p == needle || p.ends_with(&format!("/{needle}")) || p.ends_with(&format!("/{base}")))
    })
}

/// Check the series.
///
/// The `check` form knows nothing of the base; `check_over` exempts files
/// the base tree already provides, which is every upstream header and
/// source a series merely consumes. Without the exemption the provider
/// rule flags `libavutil/mem.h` and friends once per consumer, drowning
/// the real ordering findings.
#[cfg_attr(not(test), allow(dead_code))] // the base-less form is the tests' API
pub fn check(commits: &[Commit]) -> Vec<Finding> {
    check_over(commits, &std::collections::HashSet::new())
}

pub fn check_over(commits: &[Commit], base_files: &std::collections::HashSet<String>) -> Vec<Finding> {
    let mut out = Vec::new();

    for (i, c) in commits.iter().enumerate() {
        // A consumer must not land before its provider. Generated headers are exempt:
        // configure writes them, so they are never in the tree to be found.
        for inc in added_includes(&c.diff) {
            if is_generated_header(&inc) {
                continue;
            }
            // Only project-local includes; a system header is not ours to provide.
            if !inc.contains('/') && !inc.starts_with("libav") && !inc.ends_with(".h") {
                continue;
            }
            let ours = inc.starts_with("libav") || inc.starts_with("libsw");
            if ours && base_files.contains(inc.as_str()) {
                continue;
            }
            if ours && !provided_by(commits, i, &inc) {
                out.push(Finding {
                    rule: "series-provider-before-consumer",
                    tier: Tier::Heuristic,
                    commit: c.id.clone(),
                    detail: format!("includes {inc:?}, which no earlier commit in the series provides"),
                });
            }
        }

        // The same rule over Makefile object references, which is the form the
        // original check missed: a build that lists foo.o before foo.c exists is
        // broken at exactly the commit a bisect will land on.
        for obj in added_makefile_objects(&c.diff) {
            let in_base = |o: &str| {
                base_files.contains(o)
                    || base_files
                        .iter()
                        .any(|p| p.rsplit('/').next() == Some(o))
            };
            if in_base(&obj) {
                continue;
            }
            if !provided_by(commits, i, &obj) {
                out.push(Finding {
                    rule: "series-provider-before-consumer",
                    tier: Tier::Mechanical,
                    commit: c.id.clone(),
                    detail: format!("a Makefile lists the object for {obj:?}, which no earlier commit provides"),
                });
            }
        }

        // A new registration carries obligations. Each one fires separately so an
        // author is told which is missing rather than that something is.
        if let Some(kind) = registration_kind(&c.diff) {
            let has = |suffix: &str| {
                c.added_paths
                    .iter()
                    .chain(c.touched_paths.iter())
                    .any(|p| p.ends_with(suffix) || p.contains(suffix))
            };
            if !has("Changelog") {
                out.push(Finding {
                    rule: "series-registration-changelog",
                    tier: Tier::Heuristic,
                    commit: c.id.clone(),
                    detail: format!("registers a new {kind} and touches no Changelog"),
                });
            }
            if !has("doc/") {
                out.push(Finding {
                    rule: "series-registration-doc",
                    tier: Tier::Heuristic,
                    commit: c.id.clone(),
                    detail: format!("registers a new {kind} and touches no documentation"),
                });
            }
            if !has("MAINTAINERS") {
                out.push(Finding {
                    rule: "series-registration-maintainers",
                    tier: Tier::Heuristic,
                    commit: c.id.clone(),
                    detail: format!("registers a new {kind} and adds no MAINTAINERS entry"),
                });
            }
        }

        // An avpriv_ symbol crossing libraries is an ABI-visible move, so the minor
        // version has to rise with it or a mixed install breaks.
        if adds_avpriv(&c.diff) && !touches_version_header(c) {
            out.push(Finding {
                rule: "series-version-bump",
                tier: Tier::Mechanical,
                commit: c.id.clone(),
                detail: "adds an avpriv_ symbol without touching a version header; a cross-library symbol needs the minor bump".to_string(),
            });
        }

        // A fix meant for a release branch should carry the fix and nothing else.
        if references_ticket(&c.body) && cosmetic::classify(&c.diff) == cosmetic::Kind::Mixed {
            out.push(Finding {
                rule: "series-backport-focus",
                tier: Tier::Heuristic,
                commit: c.id.clone(),
                detail: "references a ticket or CVE and carries formatting changes too; a fix intended for backporting should stay focused".to_string(),
            });
        }

        // Security routing, three ways. A note rather than a finding: where a report
        // goes is the author's decision and the classifier only says which door it
        // looks like.
        if let Some(route) = security_route(&c.subject, &c.body) {
            out.push(Finding {
                rule: "series-security-routing",
                tier: Tier::Heuristic,
                commit: c.id.clone(),
                detail: format!("looks like a security fix; route: {route}"),
            });
        }

        // A FATE test naming a sample that the series does not add needs a
        // samples-request note, or the test cannot run for anybody else.
        for sample in added_fate_samples(&c.diff) {
            if !c.body.to_ascii_lowercase().contains("samples")
                && !c.added_paths.iter().any(|p| p.contains("fate-suite"))
            {
                out.push(Finding {
                    rule: "series-fate-sample",
                    tier: Tier::Heuristic,
                    commit: c.id.clone(),
                    detail: format!("a FATE test references the sample {sample:?} and the commit neither adds it nor notes a samples request"),
                });
            }
        }
    }

    out
}

fn registration_kind(diff: &str) -> Option<&'static str> {
    // Registration happens by adding an entry to one of the tables. Every table is
    // covered, because the original check knew only the codec and format lists and
    // therefore never fired on a new filter.
    const TABLES: &[(&str, &str)] = &[
        ("extern const FFCodec ff_", "codec"),
        ("extern const FFOutputFormat ff_", "muxer"),
        ("extern const AVInputFormat ff_", "demuxer"),
        ("extern const AVFilter ff_", "filter"),
        ("extern const FFBitStreamFilter ff_", "bitstream filter"),
        ("extern const URLProtocol ff_", "protocol"),
    ];
    for line in diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")) {
        for (needle, kind) in TABLES {
            if line[1..].trim_start().starts_with(needle) {
                return Some(kind);
            }
        }
    }
    None
}

fn adds_avpriv(diff: &str) -> bool {
    // Definition-shaped only: an added line that begins at column zero and
    // mentions avpriv_ is a top-level definition or header declaration. A
    // call site is indented inside a function body, and flagging every
    // consumer for the defining commit's missing bump is noise that buries
    // the one finding that matters.
    diff.lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .any(|l| {
            let body = &l[1..];
            !body.starts_with(' ') && !body.starts_with('\t') && body.contains("avpriv_")
        })
}

fn touches_version_header(c: &Commit) -> bool {
    c.added_paths
        .iter()
        .chain(c.touched_paths.iter())
        .any(|p| p.ends_with("version.h") || p.ends_with("version_major.h"))
}

fn references_ticket(body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    b.contains("cve-") || b.contains("ticket") || b.contains("fixes #") || b.contains("trac.ffmpeg.org")
}

/// Three doors, and which one a fix looks like it belongs to.
fn security_route(subject: &str, body: &str) -> Option<&'static str> {
    let t = format!("{subject} {body}").to_ascii_lowercase();
    const EXPLOITABLE: &[&str] = &["buffer overflow", "out of bounds write", "heap overflow", "arbitrary code", "use after free"];
    const NON_EXPLOITABLE: &[&str] = &["undefined behaviour", "undefined behavior", "memory leak", "integer overflow", "null pointer"];
    if EXPLOITABLE.iter().any(|k| t.contains(k)) {
        return Some("ffmpeg-security, privately, because it looks exploitable");
    }
    if NON_EXPLOITABLE.iter().any(|k| t.contains(k)) {
        return Some("a Forgejo pull request, because it looks non-exploitable");
    }
    None
}

fn added_fate_samples(diff: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fate = false;
    for line in diff.lines() {
        if let Some(p) = line.strip_prefix("+++ b/") {
            in_fate = p.contains("tests/fate/");
            continue;
        }
        if !in_fate || !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        for tok in line[1..].split_whitespace() {
            if let Some(rest) = tok.strip_prefix("$(TARGET_SAMPLES)/") {
                out.push(rest.trim_end_matches(['"', ',']).to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, subject: &str, body: &str, added: &[&str], diff: &str) -> Commit {
        Commit {
            id: id.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            added_paths: added.iter().map(|s| s.to_string()).collect(),
            touched_paths: Vec::new(),
            diff: diff.to_string(),
        }
    }

    #[test]
    fn a_series_including_a_generated_header_passes() {
        // configure writes config_components.h; demanding its source would flag
        // every series that includes one.
        let s = [c(
            "a",
            "avcodec/x: use the components header",
            "",
            &[],
            "--- a/libavcodec/x.c\n+++ b/libavcodec/x.c\n@@ -1,0 +1,1 @@\n+#include \"config_components.h\"\n",
        )];
        let f = check(&s);
        assert!(
            f.iter().all(|x| x.rule != "series-provider-before-consumer"),
            "{f:?}"
        );
        assert!(is_generated_header("config_components.h"));
        assert!(is_generated_header("libavutil/avconfig.h"));
        assert!(!is_generated_header("libavcodec/h264.h"));
    }

    #[test]
    fn a_makefile_object_ahead_of_its_source_flags() {
        let s = [c(
            "a",
            "avcodec: build newthing",
            "",
            &[],
            "--- a/libavcodec/Makefile\n+++ b/libavcodec/Makefile\n@@ -1,0 +1,1 @@\n+OBJS-$(CONFIG_NEWTHING) += newthing.o\n",
        )];
        let f = check(&s);
        let hit = f.iter().find(|x| x.rule == "series-provider-before-consumer");
        assert!(hit.is_some(), "{f:?}");
        assert_eq!(hit.unwrap().tier, Tier::Mechanical, "a broken build is not advisory");
    }

    #[test]
    fn a_makefile_object_after_its_source_passes() {
        let s = [
            c("a", "avcodec/newthing: add it", "", &["libavcodec/newthing.c"], ""),
            c(
                "b",
                "avcodec: build newthing",
                "",
                &[],
                "--- a/libavcodec/Makefile\n+++ b/libavcodec/Makefile\n@@ -1,0 +1,1 @@\n+OBJS-$(CONFIG_NEWTHING) += newthing.o\n",
            ),
        ];
        assert!(check(&s).iter().all(|x| x.rule != "series-provider-before-consumer"));
    }

    #[test]
    fn a_filter_registration_fires_each_obligation_separately() {
        // Every table is covered: the original check knew only codecs and formats and
        // so never fired on a new filter at all.
        let s = [c(
            "a",
            "avfilter/vf_new: add it",
            "",
            &["libavfilter/vf_new.c"],
            "--- a/libavfilter/allfilters.c\n+++ b/libavfilter/allfilters.c\n@@ -1,0 +1,1 @@\n+extern const AVFilter ff_vf_new;\n",
        )];
        let f = check(&s);
        for want in [
            "series-registration-changelog",
            "series-registration-doc",
            "series-registration-maintainers",
        ] {
            assert!(f.iter().any(|x| x.rule == want), "{want} missing from {f:?}");
        }
    }

    #[test]
    fn a_registration_with_all_three_is_silent() {
        let mut cm = c(
            "a",
            "avfilter/vf_new: add it",
            "",
            &["libavfilter/vf_new.c"],
            "--- a/libavfilter/allfilters.c\n+++ b/libavfilter/allfilters.c\n@@ -1,0 +1,1 @@\n+extern const AVFilter ff_vf_new;\n",
        );
        cm.touched_paths = vec![
            "Changelog".to_string(),
            "doc/filters.texi".to_string(),
            "MAINTAINERS".to_string(),
        ];
        let f = check(&[cm]);
        assert!(f.iter().all(|x| !x.rule.starts_with("series-registration")), "{f:?}");
    }

    #[test]
    fn every_registration_table_is_recognised() {
        for (line, _kind) in [
            ("+extern const FFCodec ff_x_decoder;", "codec"),
            ("+extern const AVFilter ff_vf_x;", "filter"),
            ("+extern const FFBitStreamFilter ff_x_bsf;", "bitstream filter"),
            ("+extern const URLProtocol ff_x_protocol;", "protocol"),
        ] {
            let diff = format!("--- a/x\n+++ b/x\n@@ -1,0 +1,1 @@\n{line}\n");
            assert!(registration_kind(&diff).is_some(), "{line} not recognised");
        }
    }

    #[test]
    fn an_avpriv_move_without_a_version_bump_flags() {
        let s = [c(
            "a",
            "avutil: share a helper",
            "",
            &[],
            "--- a/libavutil/x.c\n+++ b/libavutil/x.c\n@@ -1,0 +1,1 @@\n+int avpriv_new_helper(void) { return 0; }\n",
        )];
        let f = check(&s);
        assert!(f.iter().any(|x| x.rule == "series-version-bump"), "{f:?}");

        let mut with = s[0].clone();
        with.touched_paths = vec!["libavutil/version.h".to_string()];
        assert!(check(&[with]).iter().all(|x| x.rule != "series-version-bump"));
    }

    #[test]
    fn a_ticket_fix_carrying_formatting_warns_to_split() {
        let mixed = concat!(
            "--- a/libavcodec/x.c\n+++ b/libavcodec/x.c\n",
            "@@ -1,2 +1,2 @@\n-  int a = f(x);\n-    int b = 1;\n+    int a = f(x);\n+    int b = 2;\n"
        );
        let s = [c("a", "avcodec/x: fix a crash", "Fixes #1234", &[], mixed)];
        let f = check(&s);
        assert!(f.iter().any(|x| x.rule == "series-backport-focus"), "{f:?}");

        // The same diff without a ticket reference is not a backport candidate.
        let s2 = [c("a", "avcodec/x: tidy", "", &[], mixed)];
        assert!(check(&s2).iter().all(|x| x.rule != "series-backport-focus"));
    }

    #[test]
    fn the_security_classifier_routes_three_ways() {
        let exploitable = c("a", "avcodec/x: fix heap overflow", "", &[], "");
        assert!(check(&[exploitable])
            .iter()
            .any(|x| x.rule == "series-security-routing" && x.detail.contains("ffmpeg-security")));

        let ub = c("a", "avcodec/x: fix undefined behaviour", "", &[], "");
        assert!(check(&[ub])
            .iter()
            .any(|x| x.rule == "series-security-routing" && x.detail.contains("Forgejo")));

        // Everything else takes the normal path and is not mentioned.
        let normal = c("a", "avcodec/x: add a feature", "", &[], "");
        assert!(check(&[normal]).iter().all(|x| x.rule != "series-security-routing"));
    }

    #[test]
    fn a_fate_test_naming_an_absent_sample_needs_a_note() {
        let diff = "--- a/tests/fate/x.mak\n+++ b/tests/fate/x.mak\n@@ -1,0 +1,1 @@\n+fate-x: CMD = framecrc -i $(TARGET_SAMPLES)/newfmt/clip.bin\n";
        let s = [c("a", "fate/x: add a test", "", &[], diff)];
        let f = check(&s);
        assert!(
            f.iter().any(|x| x.rule == "series-fate-sample" && x.detail.contains("newfmt/clip.bin")),
            "{f:?}"
        );

        // A samples request in the body settles it.
        let s2 = [c("a", "fate/x: add a test", "Samples request sent to the list.", &[], diff)];
        assert!(check(&s2).iter().all(|x| x.rule != "series-fate-sample"));
    }

    #[test]
    fn an_empty_series_yields_nothing() {
        assert!(check(&[]).is_empty());
    }
}
