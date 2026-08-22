mod checklist;
mod config;
mod cosmetic;
mod diff;
mod forge;
mod maintainers;
mod mail;
mod msg;
mod receipt;
mod rules;
mod series;
mod sha256;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::io;
use std::process;

// The engine handshake (host-lint#23, version-handshake-fails-open): the
// dispatching core exports HOST_LINT_VERSION, and a pack built against a
// different major/minor refuses to run rather than lint with mismatched
// semantics. A may-warn check would fail open the same way a stale
// hook-copied binary does, so the refusal is strict. A direct invocation
// with no HOST_LINT_VERSION set has nothing to skew against and proceeds.
fn refuse_engine_skew() {
    let Ok(core) = env::var("HOST_LINT_VERSION") else { return };
    let major_minor = |v: &str| {
        let mut parts = v.split('.');
        (
            parts.next().unwrap_or("").to_string(),
            parts.next().unwrap_or("").to_string(),
        )
    };
    if major_minor(&core) != major_minor(host_lint::ENGINE_VERSION) {
        eprintln!(
            "host-lint-ffmpeg: engine version skew: core {core}, pack built against {}; reinstall the pair together",
            host_lint::ENGINE_VERSION
        );
        process::exit(2);
    }
}

/// The message lane: `msg [--signoff] [<file>]`, or stdin. Exits 1 on a mechanical
/// finding, 3 on heuristic findings alone, 0 clean, 2 on a usage or IO error, which
/// is the core's own verdict split so a hook can treat both the same way.
/// `config` shows what the pack resolved and where from, so a surprising mode can be
/// traced to the file that set it rather than guessed at.
/// The forge lane: `forge --title <t> [--body <b>] [--draft]`.
/// The mail lane: `mail <format-patch-dir> [--maintainers <file>]`.
/// The series lane: `series <range> [--repo <dir>]`, e.g. `series HEAD~5..HEAD`.
/// `receipt --record <name>=<passed|failed> ... --base <sha> --head <sha>` writes a
/// build receipt, and `receipt --show <head>` reads one back.
/// `install-hooks [<worktree>]` lands the hooks in the worktree's PRIVATE gitdir.
///
/// Per worktree, not per clone, because the mode is per worktree: one worktree can be
/// a frozen series while another is live work, and a hook in the shared common dir
/// would apply one worktree's mode to the other.
///
/// Nothing lands in the target tree itself, tracked or untracked. A tool that dropped
/// a config file into a working tree would show up in `git status` and eventually in
/// somebody's commit.
fn run_install_hooks(args: &[String]) -> ! {
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| ".".to_string());

    let git = |a: &[&str]| -> Result<String, String> {
        let o = process::Command::new("git")
            .arg("-C").arg(&dir).args(a)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if !o.status.success() {
            return Err(format!("git {:?} failed: {}", a, String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    };

    // The worktree's own gitdir. For a linked worktree this is
    // `<common>/worktrees/<name>`; for the main one it is `<common>` itself.
    //
    // `--git-path hooks` is NOT the way to find a per-worktree hooks directory: it
    // resolves to the SHARED hooks dir even from a linked worktree, so installing in
    // two worktrees wrote the same file twice and the second silently replaced the
    // first. Per-worktree hooks need `core.hooksPath` in the worktree's private
    // config, which is what `extensions.worktreeConfig` unlocks.
    let gitdir = match git(&["rev-parse", "--absolute-git-dir"]) {
        Ok(s) => PathBuf::from(s),
        Err(e) => {
            eprintln!("host-lint-ffmpeg: {dir} is not a git worktree ({e})");
            process::exit(2);
        }
    };
    let hooks = gitdir.join("host-lint-ffmpeg-hooks");

    // Per-worktree config has to be enabled once per clone before `--worktree` works.
    if let Err(e) = git(&["config", "extensions.worktreeConfig", "true"]) {
        eprintln!("host-lint-ffmpeg: cannot enable per-worktree config: {e}");
        process::exit(2);
    }
    if let Err(e) = fs::create_dir_all(&hooks) {
        eprintln!("host-lint-ffmpeg: cannot create {}: {e}", hooks.display());
        process::exit(2);
    }

    let core = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("host-lint")))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "host-lint".to_string());
    let pack = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "host-lint-ffmpeg".to_string());

    // Both versions are stamped together, and the hook refuses a skewed pair rather
    // than linting with mismatched semantics. Written as a raw template with markers
    // substituted, because a format string carrying shell quoting is unreadable and
    // Rust reads `"$CORE"` in one as a string prefix.
    const COMMIT_MSG: &str = r#"#!/usr/bin/env bash
# Installed by host-lint-ffmpeg @PACK_VER@. Core and pack are stamped together, and a
# skewed pair refuses rather than linting with mismatched semantics.
set -u
CORE='@CORE@'
PACK='@PACK@'
PACK_VER='@PACK_VER@'

pack_now=$("$PACK" --pack-version 2>/dev/null)
if [ -n "$pack_now" ] && [ "$pack_now" != "$PACK_VER" ]; then
  echo "commit-msg: pack is $pack_now and this hook was stamped for $PACK_VER; reinstall the pair" >&2
  exit 2
fi

# The core naming scan first, then the pack's message lane. The WORST verdict wins: a
# clean pack lane must never clear a core flag.
rc=0
"$CORE" --stdin < "$1" || rc=$?
prc=0
"$PACK" msg "$1" || prc=$?
for code in 2 1 3; do
  if [ "$rc" -eq "$code" ] || [ "$prc" -eq "$code" ]; then exit "$code"; fi
done
exit 0
"#;

    const PRE_COMMIT: &str = r#"#!/usr/bin/env bash
# Installed by host-lint-ffmpeg @PACK_VER@. Runs the added-line lane over the staged
# diff, so a forbidden tab or trailing space is caught before it lands.
set -u
PACK='@PACK@'
git diff --cached --unified=3 | "$PACK" diff
"#;

    let fill = |tpl: &str| {
        tpl.replace("@CORE@", &core)
            .replace("@PACK@", &pack)
            .replace("@PACK_VER@", env!("CARGO_PKG_VERSION"))
    };
    let commit_msg = fill(COMMIT_MSG);
    let pre_commit = fill(PRE_COMMIT);

    for (name, body) in [("commit-msg", &commit_msg), ("pre-commit", &pre_commit)] {
        let path = hooks.join(name);
        if let Err(e) = fs::write(&path, body) {
            eprintln!("host-lint-ffmpeg: cannot write {}: {e}", path.display());
            process::exit(2);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
        }
        println!("installed {}", path.display());
    }

    // Point this worktree, and only this worktree, at those hooks.
    if let Err(e) = git(&["config", "--worktree", "core.hooksPath", &hooks.display().to_string()]) {
        eprintln!("host-lint-ffmpeg: cannot set this worktree's hooksPath: {e}");
        process::exit(2);
    }

    // The mode this worktree will use, printed so a two-worktree install is legible.
    match config::load(std::path::Path::new(&dir)) {
        Ok(c) => println!("mode {} (from {})", c.mode.as_str(), c.source),
        Err(e) => {
            eprintln!("host-lint-ffmpeg: {e}");
            process::exit(2);
        }
    }
    process::exit(0);
}

fn run_receipt(args: &[String]) -> ! {
    let val = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let repo = val("--repo").unwrap_or_else(|| ".".to_string());
    let common = match process::Command::new("git")
        .arg("-C").arg(&repo).args(["rev-parse", "--git-common-dir"]).output()
    {
        Ok(o) if o.status.success() => {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let pb = PathBuf::from(&p);
            if pb.is_absolute() { pb } else { PathBuf::from(&repo).join(pb) }
        }
        _ => {
            eprintln!("host-lint-ffmpeg: {repo} is not a git repository");
            process::exit(2);
        }
    };

    if let Some(head) = val("--show") {
        let path = receipt::receipt_path(&common, &head);
        match fs::read_to_string(&path) {
            Ok(s) => match receipt::Receipt::parse(&s) {
                Ok(r) => {
                    print!("{}", r.export());
                    // Staleness is the reader's first question, so answer it here.
                    let now = process::Command::new("git")
                        .arg("-C").arg(&repo).args(["rev-parse", "HEAD"]).output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default();
                    if !now.is_empty() && receipt::is_stale(&r, &now) {
                        println!("stale: recorded for {} and HEAD is now {now}", r.head);
                        process::exit(3);
                    }
                    process::exit(0);
                }
                Err(e) => {
                    eprintln!("host-lint-ffmpeg: {}: {e}", path.display());
                    process::exit(2);
                }
            },
            Err(_) => {
                println!("note: no receipt for {head}; the expensive legs have not been run here");
                process::exit(3);
            }
        }
    }

    let (Some(base), Some(head)) = (val("--base"), val("--head")) else {
        eprintln!("usage: host-lint pack ffmpeg receipt --base <sha> --head <sha> [--record <leg>=<passed|failed>]... | --show <head>");
        process::exit(2);
    };
    let mut legs = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if a == "--record" {
            if let Some(spec) = args.get(i + 1) {
                let (name, res) = spec.split_once('=').unwrap_or((spec.as_str(), "unrun"));
                if !receipt::LEGS.contains(&name) {
                    eprintln!("host-lint-ffmpeg: unknown leg {name:?}; known legs: {:?}", receipt::LEGS);
                    process::exit(2);
                }
                legs.push((
                    name.to_string(),
                    match res {
                        "passed" => receipt::LegResult::Passed,
                        "failed" => receipt::LegResult::Failed,
                        _ => receipt::LegResult::Unrun,
                    },
                ));
            }
        }
    }
    let r = receipt::Receipt {
        base,
        head: head.clone(),
        toolchain: val("--toolchain").unwrap_or_else(|| "unrecorded".to_string()),
        config_digest: receipt::config_digest(&[val("--config").unwrap_or_default().as_str()]),
        legs,
    };
    let path = receipt::receipt_path(&common, &head);
    if let Some(dir) = path.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            eprintln!("host-lint-ffmpeg: cannot create {}: {e}", dir.display());
            process::exit(2);
        }
    }
    if let Err(e) = fs::write(&path, r.export()) {
        eprintln!("host-lint-ffmpeg: cannot write {}: {e}", path.display());
        process::exit(2);
    }
    println!("wrote {}", path.display());
    process::exit(0);
}

/// `checklist [--reported <id,id>] [--receipt <head>]` renders the registry.
fn run_checklist(args: &[String]) -> ! {
    let val = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let reported: Vec<String> = val("--reported")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let reported_refs: Vec<&str> = reported.iter().map(String::as_str).collect();

    let rec = val("--receipt").and_then(|head| {
        let repo = val("--repo").unwrap_or_else(|| ".".to_string());
        let out = process::Command::new("git")
            .arg("-C").arg(&repo).args(["rev-parse", "--git-common-dir"]).output().ok()?;
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let pb = PathBuf::from(&p);
        let common = if pb.is_absolute() { pb } else { PathBuf::from(&repo).join(pb) };
        fs::read_to_string(receipt::receipt_path(&common, &head))
            .ok()
            .and_then(|s| receipt::Receipt::parse(&s).ok())
    });

    print!("{}", checklist::format_report(&checklist::render(&reported_refs, rec.as_ref())));
    process::exit(0);
}

fn run_series(args: &[String]) -> ! {
    let val = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let Some(range) = args.iter().find(|a| !a.starts_with("--")).cloned() else {
        eprintln!("usage: host-lint pack ffmpeg series <range> [--repo <dir>]");
        process::exit(2);
    };
    let repo = val("--repo").unwrap_or_else(|| ".".to_string());

    let git = |a: &[&str]| -> Result<String, String> {
        let o = process::Command::new("git")
            .arg("-C").arg(&repo).args(a)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if !o.status.success() {
            return Err(format!("git {:?} failed: {}", a, String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    };

    // Oldest first: the provider-before-consumer checks depend on the order.
    let ids = match git(&["rev-list", "--reverse", "--no-merges", &range]) {
        Ok(s) => s.split_whitespace().map(str::to_string).collect::<Vec<_>>(),
        Err(e) => {
            eprintln!("host-lint-ffmpeg: {e}");
            process::exit(2);
        }
    };
    if ids.is_empty() {
        eprintln!("host-lint-ffmpeg: {range} names no commits");
        process::exit(2);
    }

    let mut commits = Vec::new();
    for id in &ids {
        let subject = git(&["log", "-1", "--format=%s", id]).unwrap_or_default().trim().to_string();
        let body = git(&["log", "-1", "--format=%b", id]).unwrap_or_default();
        let diff = git(&["show", "--format=", "--unified=3", id]).unwrap_or_default();
        let status = git(&["show", "--format=", "--name-status", id]).unwrap_or_default();
        let mut added = Vec::new();
        let mut touched = Vec::new();
        for line in status.lines() {
            let mut it = line.split('\t');
            let Some(kind) = it.next() else { continue };
            let Some(path) = it.next() else { continue };
            if kind.starts_with('A') {
                added.push(path.to_string());
            } else {
                touched.push(path.to_string());
            }
        }
        commits.push(series::Commit {
            id: id[..9.min(id.len())].to_string(),
            subject,
            body,
            added_paths: added,
            touched_paths: touched,
            diff,
        });
    }

    // Files the base tree already provides: the provider rule exempts them,
    // because a series that merely consumes libavutil/mem.h has no ordering
    // obligation about it. The base is the range's start; an unparseable
    // range yields an empty set, which is the old behavior.
    let base_files: std::collections::HashSet<String> = range
        .split_once("..")
        .and_then(|(base, _)| {
            let base = base.trim_end_matches('^');
            if base.is_empty() {
                return None;
            }
            git(&["ls-tree", "-r", "--name-only", base]).ok()
        })
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    let findings = series::check_over(&commits, &base_files);
    let mode = config::load(std::path::Path::new(&repo))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    if findings.is_empty() {
        println!("series: {} commit(s), nothing to report", commits.len());
        process::exit(0);
    }
    let mut blocking = false;
    for f in &findings {
        let label = match f.tier {
            rules::Tier::Mechanical => {
                blocking = true;
                "flag"
            }
            _ => "warn",
        };
        println!("{label}: {}: {} — {}", f.commit, f.rule, f.detail);
    }
    process::exit(verdict(blocking, mode));
}

fn run_mail(args: &[String]) -> ! {
    let val = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let Some(dir) = args.iter().find(|a| !a.starts_with("--")).cloned() else {
        eprintln!("usage: host-lint pack ffmpeg mail <format-patch-dir> [--maintainers <file>]");
        process::exit(2);
    };

    let maint = match val("--maintainers") {
        Some(f) => match fs::read_to_string(&f) {
            Ok(s) => maintainers::parse(&s),
            Err(e) => {
                eprintln!("host-lint-ffmpeg: cannot read {f}: {e}");
                process::exit(2);
            }
        },
        None => Vec::new(),
    };

    let mut files: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("patch"))
            .collect(),
        Err(e) => {
            eprintln!("host-lint-ffmpeg: cannot read {dir}: {e}");
            process::exit(2);
        }
    };
    files.sort();
    if files.is_empty() {
        eprintln!("host-lint-ffmpeg: {dir} holds no .patch files");
        process::exit(2);
    }

    // A file the lane cannot parse is an error, never a clean message: a directory
    // with a stray file in it must not report as a well-formed series.
    let mut msgs = Vec::new();
    let mut touched: Vec<String> = Vec::new();
    for f in &files {
        let name = f.display().to_string();
        let text = match fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("host-lint-ffmpeg: cannot read {name}: {e}");
                process::exit(2);
            }
        };
        for line in text.lines() {
            if let Some(p) = line.strip_prefix("+++ b/") {
                if !touched.iter().any(|t| t == p) {
                    touched.push(p.to_string());
                }
            }
        }
        match mail::parse_message(&name, &text) {
            Ok(m) => msgs.push(m),
            Err(e) => {
                eprintln!("host-lint-ffmpeg: {e}");
                process::exit(2);
            }
        }
    }

    // Thread targeting, when the caller supplies the prior version's message ids.
    // Without them the lane cannot judge, and it says so rather than passing: a
    // missing input reading as a clean result is the shape this pack refuses.
    let prior: Vec<String> = val("--prior-thread")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let prior_refs: Vec<&str> = prior.iter().map(String::as_str).collect();

    let touched_refs: Vec<&str> = touched.iter().map(String::as_str).collect();
    let mut findings = mail::check_series(&msgs);
    for m in &msgs {
        findings.extend(mail::check_message(m, &touched_refs, &maint));
        match mail::thread_target_ok(m, &prior_refs) {
            Some(false) => findings.push(mail::Finding {
                rule: "mail-thread-hijack",
                tier: rules::Tier::Mechanical,
                file: m.file.clone(),
                detail: format!(
                    "In-Reply-To targets {:?}, which is not this series' prior thread; the reviewers of the earlier version never see it",
                    m.in_reply_to.as_deref().unwrap_or("")
                ),
            }),
            Some(true) => {}
            None if m.in_reply_to.is_some() && prior.is_empty() => {
                println!("note: {} threads onto {:?}; pass --prior-thread <ids> to check the target", m.file, m.in_reply_to.as_deref().unwrap_or(""));
            }
            None => {}
        }
    }

    let mode = config::load(std::path::Path::new("."))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    if findings.is_empty() {
        println!("mail: {} message(s) over {} touched path(s), nothing to report", msgs.len(), touched.len());
        process::exit(0);
    }
    let mut blocking = false;
    for f in &findings {
        let label = match f.tier {
            rules::Tier::Mechanical => {
                blocking = true;
                "flag"
            }
            _ => "warn",
        };
        println!("{label}: {}: {} — {}", f.file, f.rule, f.detail);
    }
    process::exit(verdict(blocking, mode));
}

fn run_forge(args: &[String]) -> ! {
    let val = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    let Some(title) = val("--title") else {
        eprintln!("usage: host-lint pack ffmpeg forge --title <title> [--body <body>] [--draft]");
        process::exit(2);
    };
    let body = val("--body").unwrap_or_default();
    let pr = forge::Pr { title: &title, body: &body, draft: args.iter().any(|a| a == "--draft") };

    let mode = config::load(std::path::Path::new("."))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    let mut findings = forge::check(&pr);
    if let Some(f) = forge::rationale_lands_in_commits(&pr, &[]) {
        findings.push(f);
    }
    if findings.is_empty() {
        println!("forge: the pull request metadata satisfies the lane");
        process::exit(0);
    }
    let mut blocking = false;
    for f in &findings {
        let label = match f.tier {
            rules::Tier::Mechanical => {
                blocking = true;
                "flag"
            }
            _ => "warn",
        };
        println!("{label}: {} — {}", f.rule, f.detail);
    }
    process::exit(verdict(blocking, mode));
}

fn run_config(args: &[String]) -> ! {
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| ".".to_string());
    match config::load(std::path::Path::new(&dir)) {
        Err(e) => {
            eprintln!("host-lint-ffmpeg: {e}");
            process::exit(2);
        }
        Ok(c) => {
            println!("source        {}", c.source);
            println!("upstream_ref  {}", c.upstream_ref);
            println!("mode          {} ({})", c.mode.as_str(),
                if c.mode.blocks() { "mechanical findings block" } else { "nothing blocks" });
            println!("branch_prefix {}", c.branch_prefix.as_deref().unwrap_or("(unset)"));
            println!("tag_prefix    {}", c.tag_prefix.as_deref().unwrap_or("(unset)"));
            process::exit(0);
        }
    }
}

/// The verdict a lane exits with, filtered through the project's mode. In advise and
/// frozen modes a mechanical finding still PRINTS as a flag — the finding is what it
/// is — and the exit code drops to the advisory one, because the mode governs
/// consequences rather than truth.
fn verdict(blocking: bool, mode: config::Mode) -> i32 {
    match (blocking, mode.blocks()) {
        (true, true) => 1,
        (true, false) => 3,
        (false, _) => 3,
    }
}

fn run_msg(args: &[String]) -> ! {
    let signoff = args.iter().any(|a| a == "--signoff");
    let tracker = args.iter().any(|a| a == "--require-tracker");
    let file = args.iter().find(|a| !a.starts_with("--"));

    // A rev range runs the lane over every commit message in it, the shape
    // a series review wants; a plain path is the single-message form the
    // hook path uses.
    if let Some(f) = file {
        if f.contains("..") {
            run_msg_range(f, signoff, tracker);
        }
    }

    let text = match file {
        Some(f) => match fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("host-lint-ffmpeg: cannot read {f}: {e}");
                process::exit(2);
            }
        },
        None => {
            let mut s = String::new();
            if let Err(e) = io::Read::read_to_string(&mut io::stdin(), &mut s) {
                eprintln!("host-lint-ffmpeg: cannot read stdin: {e}");
                process::exit(2);
            }
            s
        }
    };

    let mode = config::load(std::path::Path::new("."))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    let findings = msg::check_with(&text, signoff, tracker);
    if findings.is_empty() {
        process::exit(0);
    }
    let mut blocking = false;
    for f in &findings {
        let label = match f.tier {
            rules::Tier::Mechanical => {
                blocking = true;
                "flag"
            }
            _ => "warn",
        };
        println!("{label}: {} — {}", f.rule, f.detail);
    }
    process::exit(verdict(blocking, mode));
}

/// The message lane over a rev range: every commit message, oldest first,
/// with findings labelled by commit.
fn run_msg_range(range: &str, signoff: bool, tracker: bool) -> ! {
    let git = |a: &[&str]| -> Option<String> {
        let o = process::Command::new("git").args(a).output().ok()?;
        if !o.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let Some(ids) = git(&["rev-list", "--reverse", "--no-merges", range]) else {
        eprintln!("host-lint-ffmpeg: {range} names no commits");
        process::exit(2);
    };
    let ids: Vec<&str> = ids.split_whitespace().collect();
    if ids.is_empty() {
        eprintln!("host-lint-ffmpeg: {range} names no commits");
        process::exit(2);
    }
    let mode = config::load(std::path::Path::new("."))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    let mut blocking = false;
    let mut total = 0;
    for id in &ids {
        let Some(message) = git(&["log", "-1", "--format=%B", id]) else {
            continue;
        };
        for f in msg::check_with(&message, signoff, tracker) {
            total += 1;
            let label = match f.tier {
                rules::Tier::Mechanical => {
                    blocking = true;
                    "flag"
                }
                _ => "warn",
            };
            println!("{label}: {}: {} — {}", &id[..9.min(id.len())], f.rule, f.detail);
        }
    }
    if total == 0 {
        println!("msg: {} commit(s), nothing to report", ids.len());
        process::exit(0);
    }
    process::exit(verdict(blocking, mode));
}

/// The added-line lane: `diff [<file>]`, or a unified diff on stdin.
fn run_diff(args: &[String]) -> ! {
    let file = args.iter().find(|a| !a.starts_with("--"));
    let text = match file {
        Some(f) => match fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("host-lint-ffmpeg: cannot read {f}: {e}");
                process::exit(2);
            }
        },
        None => {
            let mut s = String::new();
            if let Err(e) = io::Read::read_to_string(&mut io::stdin(), &mut s) {
                eprintln!("host-lint-ffmpeg: cannot read stdin: {e}");
                process::exit(2);
            }
            s
        }
    };

    let mode = config::load(std::path::Path::new("."))
        .map(|c| c.mode)
        .unwrap_or(config::Mode::Advise);
    let mut findings = diff::check_diff(&text);
    // The mixed cosmetic/functional check reads the whole diff rather than one line,
    // so it joins here rather than in the per-line pass.
    for c in cosmetic::check(&text) {
        findings.push(diff::Finding {
            rule: c.rule,
            tier: c.tier,
            path: String::new(),
            line: 0,
            detail: c.detail,
        });
    }
    if findings.is_empty() {
        process::exit(0);
    }
    let mut blocking = false;
    for f in &findings {
        let label = match f.tier {
            rules::Tier::Mechanical => {
                blocking = true;
                "flag"
            }
            _ => "warn",
        };
        if f.path.is_empty() {
            println!("{label}: {} — {}", f.rule, f.detail);
        } else {
            println!("{label}: {}:{}: {} — {}", f.path, f.line, f.rule, f.detail);
        }
    }
    process::exit(verdict(blocking, mode));
}

/// `branch` checks the current branch and its tags against the project's grammar,
/// and reports whether the series is frozen. Frozen is derived from tags rather than
/// declared, because a declaration goes stale the moment somebody tags.
fn run_branch(args: &[String]) -> ! {
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| ".".to_string());
    let path = std::path::Path::new(&dir);
    let cfg = match config::load(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("host-lint-ffmpeg: {e}");
            process::exit(2);
        }
    };
    let out = process::Command::new("git")
        .arg("-C").arg(&dir).args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();
    let branch = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            eprintln!("host-lint-ffmpeg: cannot read the current branch of {dir}");
            process::exit(2);
        }
    };

    let mut blocking = false;
    if !config::branch_ok(&cfg, &branch) {
        blocking = true;
        println!(
            "flag: branch-grammar — {branch:?} does not start with the configured prefix {:?}",
            cfg.branch_prefix.as_deref().unwrap_or("")
        );
    }
    let tags = process::Command::new("git")
        .arg("-C").arg(&dir).args(["tag", "--merged", &branch])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    for tag in tags.lines().map(str::trim).filter(|t| !t.is_empty()) {
        if !config::tag_ok(&cfg, tag) {
            blocking = true;
            println!(
                "flag: tag-grammar — {tag:?} does not start with the configured prefix {:?}",
                cfg.tag_prefix.as_deref().unwrap_or("")
            );
        }
    }
    if config::is_frozen_branch(path, &cfg, &branch) {
        println!("note: {branch} is a frozen series; findings report and never block");
    }
    if !blocking {
        println!("branch: {branch} satisfies the project grammar");
        process::exit(0);
    }
    process::exit(verdict(blocking, cfg.mode));
}

fn run_rules(args: &[String]) -> ! {
    // `--verify-source <tree>` checks a real FFmpeg checkout against the pinned
    // digests. It reports per section, because "the file changed" is not something
    // an operator can act on and an edit outside a rule-bearing section is not
    // rule drift.
    if args.first().map(String::as_str) == Some("--verify-source") {
        let Some(tree) = args.get(1) else {
            eprintln!("usage: host-lint pack ffmpeg rules --verify-source <ffmpeg-tree>");
            process::exit(2);
        };
        // Which sections state rules is re-derived from the tree and compared with the
        // recorded booleans, because that classification is the corpus's denominator:
        // every completeness claim the registry makes is "over the rule-bearing
        // sections", and it was hand-set and wrong twice while the completeness test
        // stayed green. A disagreement here is not drift in upstream's text, it is the
        // registry describing that text incorrectly.
        let path = std::path::Path::new(tree);
        let mut misjudged = 0usize;
        match rules::classification_disagreements(path) {
            Err(e) => {
                eprintln!("host-lint-ffmpeg: {e}");
                process::exit(2);
            }
            Ok(bad) => {
                for (title, derived, recorded) in &bad {
                    // Recorded inert while the text states rules is the direction that
                    // loses coverage silently: the section maps no rules and its drift
                    // is downgraded to "moved". The opposite over-gates, which is loud.
                    let verdict = if *derived { "MISJUDGED" } else { "over-gated" };
                    println!(
                        "{verdict}  doc/developer.texi: {title} — text derives rule_bearing={derived}, table records {recorded}"
                    );
                    if *derived {
                        misjudged += 1;
                    }
                }
            }
        }
        match rules::drifted_sections(path) {
            Err(e) => {
                eprintln!("host-lint-ffmpeg: {e}");
                process::exit(2);
            }
            Ok(d) if d.is_empty() && misjudged == 0 => {
                println!(
                    "rules: every pinned section matches {tree} at {}, and all {} section(s) classify as recorded ({} rule-bearing)",
                    rules::UPSTREAM_COMMIT,
                    rules::SECTIONS.len(),
                    rules::SECTIONS.iter().filter(|s| s.rule_bearing).count()
                );
                process::exit(0);
            }
            Ok(d) => {
                let gating = d.iter().filter(|x| x.rule_bearing).count();
                for s in d.iter().filter(|x| x.rule_bearing) {
                    println!("DRIFT  {}", s.what);
                }
                for s in d.iter().filter(|x| !x.rule_bearing) {
                    println!("moved  {} (states no rules; reported, not gating)", s.what);
                }
                println!(
                    "-- {gating} rule-bearing section(s) differ from the corpus pinned at {}; {} other section(s) moved; \
                     {misjudged} section(s) state rules the table records as stating none",
                    rules::UPSTREAM_COMMIT,
                    d.len() - gating
                );
                process::exit(if gating > 0 || misjudged > 0 { 1 } else { 0 });
            }
        }
    }

    // `--check-freshness <tree>` asks whether the pin is still the newest commit
    // touching the rule source. Answered from a git checkout rather than the network:
    // the pack has no HTTP client, the operator already has a tree for
    // --verify-source, and a check that needs the internet cannot run in the
    // offline build this project verifies in.
    if args.first().map(String::as_str) == Some("--check-freshness") {
        let Some(tree) = args.get(1) else {
            eprintln!("usage: host-lint pack ffmpeg rules --check-freshness <ffmpeg-git-tree>");
            process::exit(2);
        };
        let out = process::Command::new("git")
            .args(["-C", tree, "log", "-1", "--format=%H", "--", "doc/developer.texi"])
            .output();
        let newest = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Ok(o) => {
                eprintln!(
                    "host-lint-ffmpeg: git could not read {tree}: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                process::exit(2);
            }
            Err(e) => {
                eprintln!("host-lint-ffmpeg: cannot run git: {e}");
                process::exit(2);
            }
        };
        if newest.is_empty() {
            eprintln!("host-lint-ffmpeg: {tree} has no history for doc/developer.texi");
            process::exit(2);
        }
        if newest == rules::UPSTREAM_COMMIT {
            println!("rules: the corpus is pinned at the newest doc/developer.texi commit ({newest})");
            process::exit(0);
        }
        println!("STALE  corpus pinned at {}", rules::UPSTREAM_COMMIT);
        println!("       newest doc/developer.texi commit is {newest}");
        println!("-- re-encode against the newer tree before trusting a clean rules verdict");
        process::exit(1);
    }

    let json = args.first().map(String::as_str) == Some("--json");
    if json {
        println!("{{");
        println!("  \"upstream_commit\": \"{}\",", rules::UPSTREAM_COMMIT);
        println!("  \"sources\": [");
        for (i, s) in rules::SOURCES.iter().enumerate() {
            let comma = if i + 1 == rules::SOURCES.len() { "" } else { "," };
            println!("    {{\"path\": \"{}\", \"sha256\": \"{}\"}}{comma}", s.path, s.sha256);
        }
        println!("  ],");
        println!("  \"rules\": [");
        for (i, r) in rules::RULES.iter().enumerate() {
            let comma = if i + 1 == rules::RULES.len() { "" } else { "," };
            let rate = match r.measured_rate {
                Some(v) => format!("{v}"),
                None => "null".to_string(),
            };
            println!(
                "    {{\"id\": \"{}\", \"section\": {}, \"subheading\": {}, \"tier\": \"{}\", \"lane\": \"{}\", \"measured_rate\": {rate}, \"summary\": {}}}{comma}",
                r.id,
                json_str(r.section),
                json_str(r.subheading),
                r.tier.as_str(),
                r.lane.as_str(),
                json_str(r.summary)
            );
        }
        println!("  ]");
        println!("}}");
        process::exit(0);
    }

    // The corpus checks itself before reporting. A registry that listed rules while
    // a rule-bearing section sat unmapped would be presenting an incomplete corpus
    // as the corpus, which is the failure the completeness test exists to catch —
    // and a test only catches it in CI, while this catches it wherever it runs.
    let unmapped = rules::unmapped_sections();
    let orphans = rules::orphan_rules();
    if !unmapped.is_empty() || !orphans.is_empty() {
        for s in &unmapped {
            eprintln!("host-lint-ffmpeg: rule-bearing section with no rule: {s}");
        }
        for r in &orphans {
            eprintln!("host-lint-ffmpeg: rule naming no known section: {r}");
        }
        eprintln!("host-lint-ffmpeg: the corpus is incomplete; refusing to present it as complete");
        process::exit(2);
    }

    println!("FFmpeg rule corpus, pinned at {}", rules::UPSTREAM_COMMIT);
    println!();
    for r in rules::RULES {
        let rate = match r.measured_rate {
            Some(v) => format!("{v:.2}"),
            None => "unmeasured".to_string(),
        };
        println!("{:<32} {:<11} {:<7} {}", r.id, r.tier.as_str(), r.lane.as_str(), rate);
        println!("    {}", r.summary);
        println!("    {} / {}", r.section, r.subheading);
    }
    println!();
    println!("Project conventions, which upstream does NOT document:");
    for (id, why) in rules::PROJECT_RULES {
        println!("  {id:<32} {why}");
    }
    println!();
    // The denominator is stated, not just the count. "45 rules over 18 sections" is
    // unfalsifiable on its face; "18 of 35, classified by @subheading or two or more
    // normative prose statements" invites the reader to doubt the 18 — which is what
    // three wrong values of it needed and never got.
    println!(
        "-- {} rule(s) over {} of {} section(s) classified rule-bearing (an @subheading, or two or \
         more normative statements outside code blocks; re-derived by `rules --verify-source`); \
         {} carry a measured rate (CALIBRATION.md)",
        rules::RULES.len(),
        rules::SECTIONS.iter().filter(|s| s.rule_bearing).count(),
        rules::SECTIONS.len(),
        rules::RULES.iter().filter(|r| r.measured_rate.is_some()).count()
    );
    process::exit(0);
}

/// Minimal JSON string escaping: the corpus carries quotes and backslashes in rule
/// text, and emitting them raw would produce output nothing can parse.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() {
    refuse_engine_skew();
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--pack-version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        process::exit(0);
    }
    match args.first().map(String::as_str) {
        Some("rules") => run_rules(&args[1..]),
        Some("msg") => run_msg(&args[1..]),
        Some("diff") => run_diff(&args[1..]),
        Some("config") => run_config(&args[1..]),
        Some("branch") => run_branch(&args[1..]),
        Some("forge") => run_forge(&args[1..]),
        Some("mail") => run_mail(&args[1..]),
        Some("series") => run_series(&args[1..]),
        Some("receipt") => run_receipt(&args[1..]),
        Some("checklist") => run_checklist(&args[1..]),
        Some("install-hooks") => run_install_hooks(&args[1..]),
        _ => {}
    }
    // Lanes that run today are listed with their usage; the ones still on
    // the build sequence say so. A bare invocation is a usage error and
    // never exits 0, so it cannot report a clean verdict it did not earn.
    eprintln!("host-lint-ffmpeg <lane> [args]");
    eprintln!("  rules                      the rule registry (RULES.md is generated from it)");
    eprintln!("  msg <file|range> [--signoff] [--require-tracker]");
    eprintln!("                             commit-message lane; a range checks every commit");
    eprintln!("  diff [<file>]              added-line lane; a unified diff on stdin without a file");
    eprintln!("  series <range> [--repo d]  series lane; ordering, version bumps, registration obligations");
    eprintln!("  mail <dir> [--maintainers f]  mailing-list lane over format-patch output");
    eprintln!("  checklist                  the submission checklist");
    eprintln!("  receipt --base <sha> --head <sha> [--record leg=result]... | --show <head>");
    eprintln!("  config | branch | forge | install-hooks");
    process::exit(2);
}
