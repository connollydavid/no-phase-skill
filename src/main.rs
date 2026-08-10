use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process;

use host_lint::{Corpus, Match, LexiconEntry, LexiconScopes, Severity, load_lexicon, normalize_for_restate, restated_comment_sentences, run_docs, scan_text_with_allow_strict, scan_prose_text, escalate_subject_decoration, is_ci_file, is_scannable, path_ignored, parse_lexicon_line, is_strict_directive, parse_jira_keys, validate_lexicon_entry, output_text, output_json};

const LEXICON_FILE: &str = "LEXICON";
const IGNORE_FILE: &str = ".host-lintignore";

// The repo root: the parent of GIT_DIR when set (so hooks resolve correctly),
// else the current directory. Mirrors `run_all_files`.
fn repo_root() -> String {
    env::var("GIT_DIR")
        .ok()
        .and_then(|d| {
            let dir = Path::new(&d);
            // A linked-worktree gitdir (<store>/worktrees/<name>) carries a
            // `gitdir` file naming the worktree's `.git` link; the worktree root
            // is that file's parent. The naive parent of GIT_DIR would land
            // inside the store and drop the ignore list and LEXICON there
            // (host-lint#25).
            match fs::read_to_string(dir.join("gitdir")) {
                Ok(target) => {
                    let t = Path::new(target.trim());
                    // The `gitdir` file may name the worktree's `.git` link with a
                    // RELATIVE path — which is what `software --materialize` writes,
                    // so a bare store stays portable. A relative target is relative
                    // to GIT_DIR, never to the process's working directory; resolving
                    // it against the cwd walked out of the tree entirely, so the
                    // ignore list and the LEXICON were silently not found and every
                    // sanctioned fixture flagged. Only visible inside a hook, because
                    // only git sets GIT_DIR.
                    let abs = if t.is_absolute() { t.to_path_buf() } else { dir.join(t) };
                    abs.parent().map(|p| {
                        fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
                    })
                    .and_then(|p| p.to_str().map(String::from))
                }
                Err(_) => dir.parent().and_then(|p| p.to_str()).map(String::from),
            }
        })
        // A relative GIT_DIR (".git", which `git --git-dir=.git commit` exports)
        // has an empty parent; an empty root would drop the LEXICON and downgrade
        // strict to advisory, so fall through to the working directory instead.
        .filter(|p| !p.is_empty())
        .or_else(|| env::current_dir().ok().and_then(|p| p.to_str().map(String::from)))
        .unwrap_or_default()
}

// `host-lint lexicon <list|add|rm|--check>`: the CRUD that owns every LEXICON
// decision so a weak agent never hand-authors the file (issue #13). `add` runs the
// three guards and refuses a master key, a laundered tell, or an un-cited tracker
// ref — the tool, not the prompt, is the gate. Always exits.
fn run_lexicon(root: &str, args: &[String]) -> ! {
    let path = Path::new(root).join(LEXICON_FILE);
    match args.first().map(String::as_str) {
        Some("list") => {
            let lex = load_lexicon(Path::new(root));
            println!("strict: {}", if lex.strict { "on" } else { "off" });
            for e in &lex.entries {
                match &e.url {
                    Some(u) => println!("{}  ({})", e.phrase, u),
                    None => println!("{}", e.phrase),
                }
            }
            process::exit(0);
        }
        Some("add") => {
            let Some(phrase) = args.get(1).filter(|p| !p.is_empty()) else {
                eprintln!("usage: host-lint lexicon add \"<phrase>\" [--url <url>]");
                process::exit(2);
            };
            let url = parse_url_flag(&args[2..]);
            let entry = LexiconEntry { phrase: phrase.clone(), url };
            let existing = load_lexicon(Path::new(root));
            if let Err(reason) = validate_lexicon_entry(&entry, &existing.jira_keys) {
                eprintln!("host-lint: refused ({reason})");
                process::exit(1);
            }
            // Idempotent: a phrase already present is a no-op, not an error.
            if existing.entries.iter().any(|e| e.phrase.eq_ignore_ascii_case(&entry.phrase)) {
                println!("already present: {}", entry.phrase);
                process::exit(0);
            }
            let line = match &entry.url {
                Some(u) => format!("{} {}\n", entry.phrase, u),
                None => format!("{}\n", entry.phrase),
            };
            let mut content = fs::read_to_string(&path).unwrap_or_default();
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&line);
            if let Err(e) = fs::write(&path, content) {
                eprintln!("host-lint: cannot write {}: {e}", path.display());
                process::exit(2);
            }
            println!("added: {}", entry.phrase);
            process::exit(0);
        }
        Some("rm") => {
            let Some(phrase) = args.get(1) else {
                eprintln!("usage: host-lint lexicon rm \"<phrase>\"");
                process::exit(2);
            };
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("host-lint: no LEXICON at {}", path.display());
                    process::exit(1);
                }
            };
            let mut removed = 0;
            let kept: Vec<&str> = content
                .lines()
                .filter(|line| {
                    let drop = parse_lexicon_line(line)
                        .is_some_and(|e| e.phrase.eq_ignore_ascii_case(phrase));
                    if drop {
                        removed += 1;
                    }
                    !drop
                })
                .collect();
            if removed == 0 {
                eprintln!("host-lint: not in LEXICON: {phrase}");
                process::exit(1);
            }
            let mut out = kept.join("\n");
            out.push('\n');
            if let Err(e) = fs::write(&path, out) {
                eprintln!("host-lint: cannot write {}: {e}", path.display());
                process::exit(2);
            }
            println!("removed: {phrase}");
            process::exit(0);
        }
        Some("--check") => {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut jira_keys: Vec<String> = Vec::new();
            for line in content.lines() {
                if let Some(keys) = parse_jira_keys(line) {
                    jira_keys.extend(keys);
                }
            }
            let (mut total, mut errs) = (0, 0);
            for (i, line) in content.lines().enumerate() {
                if is_strict_directive(line) || parse_jira_keys(line).is_some() {
                    continue;
                }
                let Some(e) = parse_lexicon_line(line) else { continue };
                total += 1;
                if let Err(reason) = validate_lexicon_entry(&e, &jira_keys) {
                    eprintln!("{}:{}: invalid entry ({reason})", LEXICON_FILE, i + 1);
                    errs += 1;
                }
            }
            if errs > 0 {
                eprintln!("host-lint: {errs} invalid of {total} LEXICON entries");
                process::exit(1);
            }
            println!("LEXICON OK ({total} entries)");
            process::exit(0);
        }
        // The network lane (issue #13 guard 3): the offline format-check cannot
        // tell a real `#7` from a phantom `#999`, and a weak agent fabricates URLs,
        // so a network-having lane (CI / opt-in) re-derives liveness. Off the commit
        // hook by design — it needs the network the hook must not.
        Some("--check-urls") => {
            let cited: Vec<LexiconEntry> = load_lexicon(Path::new(root))
                .entries
                .into_iter()
                .filter(|e| e.url.is_some())
                .collect();
            if cited.is_empty() {
                println!("LEXICON: no cited references to check");
                process::exit(0);
            }
            let mut dead = 0;
            for e in &cited {
                let url = e.url.as_deref().unwrap_or_default();
                match url_status(url) {
                    Ok(code) if (200..400).contains(&code) => {
                        println!("ok   {code}  {}  ({url})", e.phrase)
                    }
                    Ok(code) => {
                        eprintln!("DEAD {code}  {}  ({url})", e.phrase);
                        dead += 1;
                    }
                    Err(msg) => {
                        eprintln!("ERR  {}  ({url}): {msg}", e.phrase);
                        dead += 1;
                    }
                }
            }
            if dead > 0 {
                eprintln!("host-lint: {dead} dead/unreachable LEXICON reference(s)");
                process::exit(1);
            }
            println!("LEXICON URLs OK ({} checked)", cited.len());
            process::exit(0);
        }
        _ => {
            eprintln!("usage: host-lint lexicon <list | add \"<phrase>\" [--url <url>] | rm \"<phrase>\" | --check | --check-urls>");
            process::exit(2);
        }
    }
}

// Pull the value of a `--url <value>` flag from a lexicon-subcommand argument tail.
fn parse_url_flag(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--url")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

// Resolve a URL to its final HTTP status by shelling `curl` (following redirects,
// body discarded, 10s cap) and parsing the code in-process — one tool, one parse,
// per the weak-agent thesis. A transport failure (DNS, timeout, no curl) is `Err`.
fn url_status(url: &str) -> Result<u32, String> {
    let out = process::Command::new("curl")
        .args(["-sSL", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "10", url])
        .output()
        .map_err(|e| format!("curl unavailable: {e}"))?;
    if !out.status.success() {
        return Err(format!("unreachable ({})", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let code = String::from_utf8_lossy(&out.stdout);
    code.trim().parse::<u32>().map_err(|_| format!("bad status '{}'", code.trim()))
}

// `host-lint pack <name> [args...]` dispatches to an external pack binary
// (`host-lint-<name>`). A reserved verb, never a bare name: the CLI already
// gives the bare-argument position to file paths, and a pack name can collide
// with a real file (`ffmpeg` names a build artifact at the root of the very
// tree the ffmpeg pack targets — host-lint#23). Resolution is beside the
// running binary first (a hook-installed pair travels together), then PATH.
// The child's exit code passes through unchanged so the 0/1/2/3 verdict
// contract holds across packs, and the core exports HOST_LINT_VERSION so the
// pack can refuse a version skew. Always exits.
fn run_pack(args: &[String]) -> ! {
    let name = match args.first().filter(|n| {
        !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }) {
        Some(n) => n,
        None => {
            eprintln!("usage: host-lint pack <name> [args...]   (runs host-lint-<name>; `host-lint packs` lists those installed)");
            process::exit(2);
        }
    };
    let bin = format!("host-lint-{name}");
    // Beside the running binary first (including the Windows asset name); a
    // plain program name falls through to the OS PATH search in spawn.
    let program: PathBuf = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(&bin)))
        .into_iter()
        .flat_map(|p| [p.with_extension("exe"), p.clone()])
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(&bin));
    match process::Command::new(&program)
        .args(&args[1..])
        .env("HOST_LINT_VERSION", env!("CARGO_PKG_VERSION"))
        .status()
    {
        // A child killed by a signal has no code; fail closed rather than clean.
        Ok(status) => process::exit(status.code().unwrap_or(2)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("host-lint: no pack '{name}': install {bin} beside host-lint or on PATH");
            process::exit(2);
        }
        Err(e) => {
            eprintln!("host-lint: cannot run {bin}: {e}");
            process::exit(2);
        }
    }
}

// `host-lint packs` lists the pack binaries installed beside the running core
// or on PATH, one bare name per line (the `host-lint-` prefix and any `.exe`
// suffix stripped). Discovery only; installing a pack is a release's job.
// Always exits 0.
fn run_packs_list() -> ! {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf());
        }
    }
    if let Some(path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path));
    }
    let mut names: Vec<String> = Vec::new();
    for d in dirs {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let fname = e.file_name();
            let Some(fname) = fname.to_str() else { continue };
            let Some(rest) = fname.strip_prefix("host-lint-") else { continue };
            let name = rest.strip_suffix(".exe").unwrap_or(rest);
            if name.is_empty() || !e.path().is_file() {
                continue;
            }
            // On unix an installed pack is executable; a stray same-named file
            // (a log, a note) is not a pack and is not listed.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if e.metadata().map(|m| m.permissions().mode() & 0o111 == 0).unwrap_or(true) {
                    continue;
                }
            }
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    if names.is_empty() {
        println!("no packs installed (a pack is a host-lint-<name> binary beside host-lint or on PATH)");
    } else {
        for n in &names {
            println!("{n}");
        }
    }
    process::exit(0);
}

// The lexicon comes from the file's own scope rather than from the invocation,
// so a vendored repository inside this one answers to its own vocabulary
// (host-lint#26). `scopes` caches per directory, so a walk reads each LEXICON once.
fn scan_file(path: &Path, scopes: &LexiconScopes, matches: &mut Vec<Match>) {
    if !path.is_file() {
        return;
    }
    if is_ci_file(path.to_string_lossy().as_ref()) {
        return;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !is_scannable(ext) {
        return;
    }
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let lex = scopes.for_file(path);
    scan_text_with_allow_strict(
        &content,
        path.to_string_lossy().as_ref(),
        &lex.phrases_lc,
        &lex.units,
        lex.strict,
        matches,
    );
}

/// Run the tracked-doc prose audit over `root` (host-lint's `run_docs`), extending
/// `matches`. An I/O error walking the tree exits 2 rather than passing it silently.
/// Shared by `--all`, `--docs`, and no-file `--prose` so one repo-wide prose audit
/// backs all three and matches the host-lifecycle prose gate (host-lint#20).
fn audit_tracked_docs(root: &str, scopes: &LexiconScopes, corpus: Corpus, matches: &mut Vec<Match>) {
    match run_docs(Path::new(root), scopes, &load_ignore(root), corpus) {
        Ok(scan) => {
            // A document listed and not read is not a skip. Reporting the rest as clean
            // would be a verdict over a corpus with a hole in it (agentic-host call/0048).
            if !scan.unread.is_empty() {
                for rel in &scan.unread {
                    eprintln!("host-lint: cannot read {rel}: listed by the walk and not audited");
                }
                process::exit(2);
            }
            matches.extend(scan.matches);
        }
        Err(e) => {
            eprintln!("host-lint: {e}");
            process::exit(2);
        }
    }
}

fn run_all_files(root: &str, scopes: &LexiconScopes, ignore: &[String], matches: &mut Vec<Match>) {
    if root.is_empty() {
        // No resolvable repository root: a clean exit here would be a fail-open
        // audit that scanned nothing. Fail closed.
        eprintln!("host-lint: --all needs a repository root (none resolved)");
        process::exit(2);
    }
    // `--all` audits the repo's tracked files (as the README states). Listing them
    // via `git ls-files` respects `.gitignore` by construction: gitignored build
    // output, vendored dependencies, and `.git/` are untracked and never appear, so
    // a naive tree walk's slowness and noise are gone without a hardcoded skip list.
    // `-z` is robust to paths containing spaces or newlines; paths are root-relative
    // and `/`-separated. `.host-lintignore` still filters tracked-but-sanctioned paths.
    let output = match process::Command::new("git")
        .args(["-C", root, "ls-files", "-z"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "host-lint: --all needs a git repository (git ls-files failed: {})",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            process::exit(2);
        }
        Err(e) => {
            eprintln!("host-lint: --all needs git on PATH: {e}");
            process::exit(2);
        }
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for rel in text.split('\0').filter(|s| !s.is_empty()) {
        if path_ignored(rel, ignore) {
            continue;
        }
        let path = Path::new(root).join(rel);
        // git tracks symlinks as symlinks; skip them — following a file symlink would
        // scan its target twice (the target, if tracked, is listed and scanned
        // directly), and a dir symlink (e.g. a cycle) is not a file to scan anyway.
        if fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            continue;
        }
        // scan_file additionally skips non-files (tracked-but-deleted), CI files,
        // and unscannable extensions.
        scan_file(&path, scopes, matches);
    }
}

// Repo-relative paths to exclude from `--all` (`.host-lintignore`, gitignore-lite:
// one pattern per line, `#` comments and blanks ignored). A migration writes this
// to exclude the append-only record; absent file → no exclusions.
fn load_ignore(root: &str) -> Vec<String> {
    if root.is_empty() {
        return Vec::new();
    }
    match fs::read_to_string(Path::new(root).join(IGNORE_FILE)) {
        Ok(content) => content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn run_log(allow: &[String], units: &[String], strict: bool, matches: &mut Vec<Match>) {
    let output = match process::Command::new("git")
        .args(["log", "-z", "--format=%H%n%B"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("host-lint: git log failed: {}", String::from_utf8_lossy(&o.stderr).trim());
            process::exit(2);
        }
        Err(e) => {
            eprintln!("host-lint: failed to run git: {}", e);
            process::exit(2);
        }
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for record in text.split('\0') {
        let record = record.trim_end_matches('\n');
        if record.is_empty() {
            continue;
        }
        let (sha, message) = match record.split_once('\n') {
            Some((s, m)) => (s, m),
            None => (record, ""),
        };
        let label = if sha.len() >= 7 { &sha[..7] } else { sha };
        scan_text_with_allow_strict(message, label, allow, units, strict, matches);
    }
}

// `gather` (the reflective-practice discovery, plan/0035): scan commit subjects
// and markdown headers for a recurring word-then-numeral shape the lane does not
// catch, and report the candidates for the operator to triage. Advisory: it
// surfaces, it never decides, and it exits zero.
fn run_gather(root: &str) -> ! {
    let mut lines: Vec<String> = Vec::new();
    // commit subjects — the richest source of an ordinal-by-position tell
    if let Ok(o) = process::Command::new("git")
        .args(["-C", root, "log", "--format=%s"])
        .output()
    {
        if o.status.success() {
            for l in String::from_utf8_lossy(&o.stdout).lines() {
                lines.push(l.to_string());
            }
        }
    }
    // header lines from tracked markdown docs
    if let Ok(o) = process::Command::new("git")
        .args(["-C", root, "ls-files", "-z"])
        .output()
    {
        if o.status.success() {
            for rel in String::from_utf8_lossy(&o.stdout)
                .split('\0')
                .filter(|s| s.ends_with(".md"))
            {
                if let Ok(content) = fs::read_to_string(Path::new(root).join(rel)) {
                    for l in content.lines() {
                        if l.trim_start().starts_with('#') {
                            lines.push(l.to_string());
                        }
                    }
                }
            }
        }
    }
    let candidates = host_lint::gather_candidates(&lines, 2);
    if candidates.is_empty() {
        println!("gather: no recurring candidate tells — the lane covers this corpus");
        process::exit(0);
    }
    println!(
        "gather: {} candidate tell-shape(s) recurring in commit subjects and headers",
        candidates.len()
    );
    println!("(advisory — triage each: propose it upstream, declare it in the LEXICON, or leave it)");
    for c in &candidates {
        println!("  {:>3}x  {}", c.count, c.word);
        for ex in &c.examples {
            println!("         e.g. {ex}");
        }
    }
    process::exit(0);
}

/// `commit --message <file> [--diff <file>] [--json]`: the commit-time verb
/// (host-lint#28). The stdin path's three message checks, plus the duplication
/// check over the staged diff. Absent `--diff` the message checks still verdict,
/// and the unrun check is disclosed beside them as a note, because a check that
/// could not run must not read as a check that passed.
fn run_commit(root: &str, args: &[String]) -> ! {
    let (mut message, mut diff, mut json) = (None::<String>, None::<String>, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--message" => {
                message = args.get(i + 1).cloned();
                i += 2;
            }
            "--diff" => {
                diff = args.get(i + 1).cloned();
                i += 2;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            other => {
                eprintln!("host-lint: commit does not take `{other}`");
                eprintln!("usage: host-lint commit --message <file> [--diff <file>] [--json]");
                process::exit(2);
            }
        }
    }
    let Some(mpath) = message else {
        eprintln!("usage: host-lint commit --message <file> [--diff <file>] [--json]");
        process::exit(2);
    };
    let input = match fs::read_to_string(&mpath) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("host-lint: cannot read {mpath}: {e}");
            process::exit(2);
        }
    };
    let lex = load_lexicon(Path::new(root));
    let mut matches = Vec::new();
    scan_text_with_allow_strict(&input, "commit-msg", &lex.phrases_lc, &lex.units, lex.strict, &mut matches);
    scan_prose_text(&input, "commit-msg", &lex.phrases_lc, &mut matches);
    escalate_subject_decoration(input.lines().next().unwrap_or(""), &mut matches);
    match &diff {
        Some(dpath) => {
            // A named diff that cannot be read is an error, never a silent skip
            // (the host-lint#23 rule for explicit arguments).
            let dtext = match fs::read_to_string(dpath) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("host-lint: cannot read {dpath}: {e}");
                    process::exit(2);
                }
            };
            let body: String = input.lines().skip(1).collect::<Vec<_>>().join("\n");
            for s in restated_comment_sentences(&dtext, &body) {
                let line = locate_in_body(&input, &s);
                let quoted: String = if s.chars().count() > 80 {
                    s.chars().take(77).collect::<String>() + "..."
                } else {
                    s.clone()
                };
                matches.push(Match {
                    file: "commit-msg".into(),
                    line,
                    col: 0,
                    text: format!("the message restates a comment the commit adds (\"{quoted}\") - trim the sentence from the message; keep the comment"),
                    term: "message-restates-diff".into(),
                    severity: Severity::Warn,
                    cite: String::new(),
                });
            }
        }
        None => {
            matches.push(Match {
                file: "commit-msg".into(),
                line: 0,
                col: 0,
                text: "duplication not checked (no --diff provided)".into(),
                term: "message-restates-diff".into(),
                severity: Severity::Note,
                cite: String::new(),
            });
        }
    }
    if json {
        output_json(&matches);
    } else {
        output_text(&matches);
    }
    process::exit(host_lint::verdict_code(&matches));
}

/// The 1-based message line where the restated sentence's opening words appear.
/// Line 1 (the subject) is never the answer, and an unlocatable sentence reports
/// the whole document (line 0), the prose lane's convention.
fn locate_in_body(message: &str, sentence: &str) -> usize {
    let n = normalize_for_restate(sentence);
    let prefix: String = n.split(' ').take(4).collect::<Vec<_>>().join(" ");
    if prefix.is_empty() {
        return 0;
    }
    for (i, l) in message.lines().enumerate().skip(1) {
        if normalize_for_restate(l).contains(&prefix) {
            return i + 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // `lexicon` is a subcommand (CRUD over the allowlist), not a scan flag.
    if args.get(1).map(String::as_str) == Some("lexicon") {
        run_lexicon(&repo_root(), &args[2..]);
    }

    // `gather` is a discovery subcommand (plan/0035), not a scan flag.
    if args.get(1).map(String::as_str) == Some("gather") {
        run_gather(&repo_root());
    }

    // `pack` and `packs` are the external-pack dispatch and discovery verbs
    // (host-lint#22): reserved names, so a pack can never collide with a file
    // argument, and no bare name ever dispatches.
    if args.get(1).map(String::as_str) == Some("pack") {
        run_pack(&args[2..]);
    }
    if args.get(1).map(String::as_str) == Some("packs") {
        run_packs_list();
    }

    // `commit` is the commit-time verb (host-lint#28): the message and the staged
    // diff as named inputs, because the duplication check needs both and a verdict
    // must reproduce from bytes visible in the invocation.
    if args.get(1).map(String::as_str) == Some("commit") {
        run_commit(&repo_root(), &args[2..]);
    }

    let mut stdin_flag = false;
    let mut json_flag = false;
    let mut all_flag = false;
    let mut log_flag = false;
    let mut prose_flag = false;
    let mut docs_flag = false;
    // `--stdin-as <path>` lints content piped on stdin as if it were the file at
    // <path>: the extension picks the naming semantics and the path drives the
    // ignore rules, so the pre-commit hook can lint the *staged* blob
    // (`git show :path`) rather than the working-tree copy.
    let mut stdin_as: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--stdin" => stdin_flag = true,
            "--json" => json_flag = true,
            "--all" => all_flag = true,
            "--log" => log_flag = true,
            "--prose" => prose_flag = true,
            "--docs" => docs_flag = true,
            "--stdin-as" => {
                i += 1;
                match args.get(i) {
                    Some(p) => stdin_as = Some(p.clone()),
                    None => {
                        eprintln!("host-lint: --stdin-as needs a path");
                        process::exit(2);
                    }
                }
            }
            other => files.push(other.to_string()),
        }
        i += 1;
    }

    // `--all` and `--docs` audit a whole repository, so a bare argument names the tree to
    // audit rather than a file to scan. It used to be pushed onto `files` and then never
    // read, so `host-lint --docs <dir>` returned a confident verdict about whichever tree
    // the working directory resolved to — a check answering about something it was not
    // handed. Honour one directory; refuse anything else rather than guess.
    let root = if all_flag || docs_flag {
        match files.len() {
            0 => repo_root(),
            1 if Path::new(&files[0]).is_dir() => files.remove(0),
            _ => {
                let flag = if all_flag { "--all" } else { "--docs" };
                eprintln!("host-lint: {flag} audits one repository; give it a single directory or none, not a file list");
                process::exit(2);
            }
        }
    } else {
        repo_root()
    };
    // Every scope in the tree, resolved lazily from the file being scanned. The
    // root's own lexicon still governs anything with no file behind it: a commit
    // message, or content piped in unnamed.
    let scopes = LexiconScopes::new(Path::new(&root));
    let lex = load_lexicon(Path::new(&root));
    let allow = lex.phrases_lc.as_slice();
    let units = lex.units.as_slice();
    let strict = lex.strict;
    let mut matches = Vec::new();

    if let Some(path) = &stdin_as {
        // Lint piped content (the staged blob) as the file at `path`: the naming
        // lane only, gated exactly as the per-file scan is — CI files and
        // unscannable extensions produce no naming tells, and `.host-lintignore`
        // applies to the real path so a sanctioned file (a test fixture, the
        // append-only record) is not flagged when committed.
        let mut input = String::new();
        // Drain stdin first (even when we will not scan) so the upstream `git show`
        // completes rather than taking SIGPIPE under the hook's `pipefail`.
        let read = io::stdin().read_to_string(&mut input);
        let rel = path.trim_start_matches(['/', '\\']).replace('\\', "/");
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        if !path_ignored(&rel, &load_ignore(&root)) && !is_ci_file(path) && is_scannable(ext) {
            // A scannable file whose staged content is not valid UTF-8 cannot be
            // scanned: fail closed rather than pass it unseen (plan/0055 cast review).
            if let Err(e) = read {
                eprintln!("host-lint: cannot read staged content of {path} as UTF-8: {e}");
                process::exit(2);
            }
            // The staged blob is scanned as the file it will become, so its
            // scope is that file's, not the invocation's (host-lint#26).
            let lex = scopes.for_file(Path::new(path));
            scan_text_with_allow_strict(&input, path, &lex.phrases_lc, &lex.units, lex.strict, &mut matches);
        }
    } else if stdin_flag {
        let mut input = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut input) {
            // Fail closed rather than scan an empty string over an unreadable input.
            eprintln!("host-lint: cannot read stdin as UTF-8: {e}");
            process::exit(2);
        }
        // A stdin title/draft gets both naming and prose tells.
        scan_text_with_allow_strict(&input, "stdin", allow, units, strict, &mut matches);
        scan_prose_text(&input, "stdin", allow, &mut matches);
        // The subject (first line) becomes a squash-merge subject / gh title; a
        // decoration tell there blocks rather than warns. The body stays advisory.
        escalate_subject_decoration(input.lines().next().unwrap_or(""), &mut matches);
    } else if all_flag {
        // `--all` is the comprehensive repo audit: the naming lane over tracked files
        // plus the prose lane over tracked authored docs, so one repo-wide command
        // matches the host-lifecycle naming + prose gate (host-lint#20). A `--prose`
        // passed alongside `--all` is redundant and folded in here.
        run_all_files(&root, &scopes, &load_ignore(&root), &mut matches);
        audit_tracked_docs(&root, &scopes, Corpus::WorkingTree, &mut matches);
    } else if prose_flag {
        if files.is_empty() {
            // `--prose` with no files audits the tracked authored docs (the repo-wide
            // prose audit that matches the host-lifecycle prose gate, host-lint#20),
            // rather than scanning nothing and exiting clean (a fail-open) or erroring.
            audit_tracked_docs(&root, &scopes, Corpus::WorkingTree, &mut matches);
        } else {
            // `--prose <files>` scans exactly those files; an unreadable one is an
            // error, not a silent skip.
            for f in &files {
                match fs::read_to_string(f) {
                    Ok(content) => {
                        let lex = scopes.for_file(Path::new(f));
                        scan_prose_text(&content, f, &lex.phrases_lc, &mut matches)
                    }
                    Err(e) => {
                        eprintln!("host-lint: cannot read {f}: {e}");
                        process::exit(2);
                    }
                }
            }
        }
    } else if docs_flag {
        audit_tracked_docs(&root, &scopes, Corpus::WorkingTree, &mut matches);
    } else if log_flag {
        run_log(allow, units, strict, &mut matches);
    } else if files.is_empty() {
        eprintln!("Usage: host-lint [--stdin] [--prose] [--docs] [--json] [--all] [--log] [files...]");
        eprintln!("       host-lint commit --message <file> [--diff <file>] [--json]");
        process::exit(2);
    } else {
        // Honor `.host-lintignore` for explicit file args too — the git hook scans
        // per staged file (`host-lint <file>`), so the ignore list must apply here,
        // not only in the `--all` walk. Otherwise a detector's own test fixtures
        // (which must embed the tells they exercise) can never pass the hook, forcing
        // `--no-verify`. Match on the same repo-relative, `/`-separated path.
        let ignore = load_ignore(&root);
        for f in &files {
            let abs = fs::canonicalize(f).unwrap_or_else(|_| Path::new(f).to_path_buf());
            let rel = abs
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| f.clone())
                .trim_start_matches(['/', '\\'])
                .replace('\\', "/");
            if path_ignored(&rel, &ignore) {
                continue;
            }
            // An explicit file argument that cannot be scanned fails closed (exit 2)
            // rather than passing silently: a typo'd path reported clean is a
            // fail-open audit (host-lint#23). The deliberate skips stay silent
            // because they are policy, not errors: an ignored path above, and the
            // CI-file and unscannable-extension skips below. `--all` keeps its own
            // silent non-file skip (tracked-but-deleted), which is likewise policy.
            let path = Path::new(f);
            let meta = match fs::metadata(path) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("host-lint: cannot scan {f}: {e}");
                    process::exit(2);
                }
            };
            if !meta.is_file() {
                eprintln!("host-lint: cannot scan {f}: not a regular file");
                process::exit(2);
            }
            if is_ci_file(f) {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !is_scannable(ext) {
                continue;
            }
            match fs::read_to_string(path) {
                Ok(content) => {
                    {
                        let lex = scopes.for_file(Path::new(f));
                        scan_text_with_allow_strict(&content, f, &lex.phrases_lc, &lex.units, lex.strict, &mut matches)
                    }
                }
                Err(e) => {
                    eprintln!("host-lint: cannot read {f}: {e}");
                    process::exit(2);
                }
            }
        }
    }

    if json_flag {
        output_json(&matches);
    } else {
        output_text(&matches);
    }

    process::exit(host_lint::verdict_code(&matches));
}
