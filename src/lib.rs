use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

// flag tier: the high-centrality words for a unit of iterative project work,
// the agentic ordinal-naming tell the gate exists to block. The set is grounded in
// measured corpus data (plan/0055): each either has proven in-project tells
// (`stage` named six work units in this repo's own history) or near-zero
// false-positive exposure in real code. Domain-heavy words (section, round, step,
// epoch, ...) live in the advisory tier below, not here.
pub const FLAG_TERMS: &[&str] = &[
    "phase", "stage", "iteration", "sprint", "cycle", "increment", "wave",
    "episode", "instalment", "leg", "lap",
    // Positional references to a milestone checklist item (host#16): the
    // "box N" / "boxes N-M" / "steps N-M" shape, the same ordinal-by-position
    // tell aimed at the "[ ]"/"[x]" marks. Plurals are listed explicitly
    // because the scan matches a whole whitespace token.
    "box", "boxes", "steps",
];

// The checklist-position subset of FLAG_TERMS: these block the arabic checklist
// form (a checklist noun plus a numeral or a range) but NOT a spelled ordinal,
// because "steps one to six" is ordinary descriptive prose (the XP-persona
// process, cast/), not a filing-system tell. Only the phase-synonym work-unit
// nouns above take the spelled-ordinal shape (a phase-synonym noun plus a spelled
// ordinal). Grounded in the repo corpus: the sole flag-noun-plus-spelled hit
// across tracked docs is a legitimate "steps one to six".
const CHECKLIST_TERMS: &[&str] = &["box", "boxes", "steps"];

// warn tier: words whose ordinal use is overwhelmingly domain vocabulary, not
// the naming of a work unit. Measured against ~35.5k real .rs files (plan/0055,
// call/0037): `round` (cipher rounds), `level` (log/DTD levels), `step` (tutorial
// steps), `pass` (compiler passes), `part`, `section` (RFC/doc sections — 2785
// hits, the largest source), `chapter` (book chapters), `epoch` (ML training),
// `batch` (jobs), `era`/`period` (time) all collide with ordinary code even at
// immediate adjacency, and each is a complete flag the LEXICON cannot escape. They
// warn rather than block; strict still escalates an undeclared occurrence to a
// flag, and the gather lane still surfaces it.
pub const WARN_ORDINAL_TERMS: &[&str] = &[
    "pass", "round", "step", "level", "part",
    "section", "chapter", "epoch", "batch", "era", "period",
];

const REVIEW_CODE_TERMS: &[&str] = &["review", "finding", "blocker"];

// warn tier: filing-system code nouns whose numbered label is a milestone
// code used as a name. Warned, not flagged, because the same nouns have
// ordinary uses ("see item 5 in the list"). `pub` so property tests can exclude
// these from the "safe designator" generator (a warn-noun like "WI" is not safe).
pub const WARN_NOUNS: &[&str] = &["work-item", "workitem", "wi"];

// warn tier: a bare "N.N" code immediately preceded by one of these is a
// version string or a cross-reference, not a milestone code — skip it.
const PREV_SKIP: &[&str] = &[
    "v", "version", "ver", "python", "node", "rust", "go", "java", "ruby",
    "php", "gcc", "clang", "llvm", "figure", "fig", "table", "eq", "equation",
    "page", "chapter", "ch", "appendix",
];

// warn tier: a bare "N.N" code immediately followed by one of these units
// is a quantity, not a milestone code — skip it.
const UNITS: &[&str] = &[
    "s", "sec", "secs", "second", "seconds", "ms", "min", "mins", "minute",
    "minutes", "h", "hr", "hrs", "hour", "hours", "day", "days",
    "gb", "mb", "kb", "tb",
];

// `gather` (discovery): common words that legitimately precede a numeral and are
// not position labels ("in 2024", "see 3", "line 42"). Kept small; the gather is
// recall-biased and the operator triages the residue.
const GATHER_STOP: &[&str] = &[
    "the", "a", "an", "of", "in", "on", "at", "to", "for", "by", "from", "with",
    "and", "or", "is", "are", "was", "were", "be", "as", "it", "this", "that",
    "about", "over", "under", "up", "all", "see", "line", "lines", "item",
    "items", "issue", "issues", "commit", "rev", "port", "row", "col", "len",
];

const CI_PATTERNS: &[&str] = &[
    ".github/workflows",
    ".gitlab-ci",
    "jenkinsfile",
    "dockerfile",
    "docker-compose",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// A confirmed tell: blocks (exit 1).
    Flag,
    /// A bare-numeral degenerate form: advisory, asks the author to reconsider (exit 3).
    Warn,
    /// A non-locatable, whole-document prose diagnosis (density, anaphora, bullet
    /// patterns): informational only, never gates (exit 0). There is no single span to
    /// edit, so it sits outside the clean-to-zero bar.
    Note,
}

pub struct Match {
    pub file: String,
    pub line: usize,
    /// 1-based character column of the tell on its line; 0 when the tell is line-level
    /// (a naming tell) or non-locatable (an advisory whole-document prose diagnosis).
    pub col: usize,
    pub text: String,
    pub term: String,
    pub severity: Severity,
    /// Citation for a prose tell (the tropes.fyi catalog name + rhetoric term);
    /// empty for a naming tell, which is self-explanatory.
    pub cite: String,
}

pub fn is_ci_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    CI_PATTERNS.iter().any(|p| lower.contains(p))
}

/// The process exit code a set of matches settles to: `1` if any match is a blocking
/// `Flag`, else `3` if any is an advisory `Warn`, else `0` (a `Note` never gates).
/// This is the verdict-lifecycle aggregation the CLI exits on — flag beats warn, a
/// warn-only set never reaches the blocking code. Defined here as a testable unit so
/// the verdict obligations are discharged by a test that exercises the aggregation,
/// not a single-line classifier (plan/0055).
pub fn verdict_code(matches: &[Match]) -> i32 {
    if matches.iter().any(|m| m.severity == Severity::Flag) {
        1
    } else if matches.iter().any(|m| m.severity == Severity::Warn) {
        3
    } else {
        0
    }
}

/// The engine version an external pack matches at runtime: the dispatching core
/// exports it as `HOST_LINT_VERSION`, the pack compares it against the version it
/// was built with, and a major/minor skew refuses to run (host-lint#23: a
/// may-warn handshake fails open the way a stale hook-copied binary does).
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

// === The reporting surface: how a set of matches renders (host-lint#22) ===
//
// Lives in the lib so the core binary and an external pack render findings
// identically: one location format, one severity vocabulary, one JSON shape.

/// A mechanical rewrite hint for the tells a weak agent can fix by a known edit,
/// keyed on the tell id (and the matched character for decoration). Judgement
/// tropes carry no hint, so only mechanically-fixable tells get one.
pub fn fix_hint(term: &str, text: &str) -> Option<&'static str> {
    match term {
        "decoration" => Some(match text {
            "—" | "–" => "replace with a comma, period, or parentheses",
            "“" | "”" | "‘" | "’" => "use a straight quote",
            "→" => "replace with a word (to, then, leads to)",
            _ => "rewrite as plain punctuation",
        }),
        _ => None,
    }
}

/// Render matches to stderr, one line each: `file:line[:col]: [severity:] text
/// (term[ — cite])[ [fix: hint]]`. The human-facing form the CLI prints; a pack
/// binary prints through this so its findings read identically.
pub fn output_text(matches: &[Match]) {
    for m in matches {
        let loc = if m.col > 0 {
            format!("{}:{}:{}", m.file, m.line, m.col)
        } else {
            format!("{}:{}", m.file, m.line)
        };
        let tag = if m.cite.is_empty() {
            m.term.clone()
        } else {
            format!("{} — {}", m.term, m.cite)
        };
        let fix = fix_hint(&m.term, &m.text)
            .map(|f| format!(" [fix: {}]", f))
            .unwrap_or_default();
        match m.severity {
            Severity::Warn => eprintln!("{}: warning: {} ({}){}", loc, m.text, tag, fix),
            Severity::Flag => eprintln!("{}: {} ({}){}", loc, m.text, tag, fix),
            Severity::Note => eprintln!("{}: note: {} ({})", loc, m.text, tag),
        }
    }
}

/// Render matches as a JSON array on stdout, the machine-facing form.
pub fn output_json(matches: &[Match]) {
    println!("{}", matches_json(matches));
}

/// The JSON array itself, for an embedder that routes it somewhere other than
/// stdout.
pub fn matches_json(matches: &[Match]) -> String {
    let mut out = String::from("[\n");
    for (i, m) in matches.iter().enumerate() {
        out.push_str("  {");
        out.push_str(&format!("\"file\": \"{}\", ", escape_json(&m.file)));
        out.push_str(&format!("\"line\": {}, ", m.line));
        out.push_str(&format!("\"col\": {}, ", m.col));
        out.push_str(&format!("\"text\": \"{}\", ", escape_json(&m.text)));
        out.push_str(&format!("\"term\": \"{}\", ", escape_json(&m.term)));
        let severity = match m.severity {
            Severity::Warn => "warn",
            Severity::Flag => "flag",
            Severity::Note => "note",
        };
        out.push_str(&format!("\"severity\": \"{}\"", severity));
        if let Some(f) = fix_hint(&m.term, &m.text) {
            out.push_str(&format!(", \"fix\": \"{}\"", escape_json(f)));
        }
        if !m.cite.is_empty() {
            out.push_str(&format!(", \"cite\": \"{}\"", escape_json(&m.cite)));
        }
        out.push('}');
        if i < matches.len() - 1 {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');
    out
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Any other control character (0x00-0x1F) must be escaped, or a line
            // carrying a raw ESC/NUL byte produces invalid JSON.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Re-exported from `host-grammar` so the checker (here) and the generator
/// (`host-lifecycle`) share one definition of a numeral.
pub use host_grammar::is_numeral;

// A bare dotted code: exactly one decimal point, digits on both sides ("5.5").
fn is_dotted_code(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

// A slashed unit token ("t/s", "ms/token", "tok/s"): a unit, not a name. Used by
// the bare-dotted-code rule to recognise a quantity whose unit is a compound a
// slash joins (host-lint#21). Alphanumeric on both sides of the slash; a bare slash
// or a trailing path is not a unit.
fn is_compound_unit(tok: &str) -> bool {
    let Some(slash) = tok.find('/') else {
        return false;
    };
    tok[..slash].chars().any(|c| c.is_alphanumeric())
        && tok[slash + 1..].chars().any(|c| c.is_alphanumeric())
}

pub fn is_review_code(word: &str) -> bool {
    // "#7" (issue-style number) or "b1" (a severity letter + number). A bare
    // numeral ("3") is NOT a code, so "review 3 files" stays clean.
    if let Some(digits) = word.strip_prefix('#') {
        return !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {
            let rest = chars.as_str();
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

// A checklist range ("4-8"): two non-empty all-digit parts joined by a single
// hyphen (ASCII or a typographic en/em-dash), each at most three digits, and
// strictly ascending. A positional reference spans a contiguous run of checklist
// items, which `is_numeral` does not accept. The three-digit bound keeps a
// four-digit (year) side out; the ascending requirement keeps a date ("12-07"),
// a time span ("9-5"), and a degenerate run ("1-1", "0-0") out — a real checklist
// span counts up (plan/0055 cast review).
fn is_num_range(word: &str) -> bool {
    let normalized = word.replace(['–', '—'], "-");
    match normalized.split_once('-') {
        Some((a, b)) => {
            !a.is_empty()
                && !b.is_empty()
                && a.len() <= 3
                && b.len() <= 3
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
                && a.parse::<u32>().ok().zip(b.parse::<u32>().ok()).is_some_and(|(x, y)| x < y)
        }
        None => false,
    }
}

// The integer value of a canonical Roman numeral (lowercased input), or None if a
// character is not a Roman digit. The caller has already established canonicity via
// `is_numeral`, so this only bounds the value.
fn roman_value(s: &str) -> Option<u32> {
    let mut total: i64 = 0;
    let mut prev: i64 = 0;
    for c in s.chars().rev() {
        let v: i64 = match c {
            'i' => 1, 'v' => 5, 'x' => 10, 'l' => 50, 'c' => 100, 'd' => 500, 'm' => 1000,
            _ => return None,
        };
        if v < prev { total -= v; } else { total += v; }
        prev = v;
    }
    u32::try_from(total).ok()
}

// The largest Roman value that still reads as a plausible ordinal position (XXXIX).
// A real phase/stage/sprint ordinal in Roman never exceeds this; every ordinary
// uppercase abbreviation that is also canonical Roman carries C/D/M or is a large
// two-letter form, so it exceeds it (DC=600, CM=900, MM=2000, MD=1500, DIV=504,
// XL=40, XC=90, LIV=54). The bound is what keeps an uppercase IV after "Phase" blocking while "phase
// DC" / "wave XL" do not.
const MAX_BLOCKING_ROMAN: u32 = 39;

// Whether the token immediately after a tell-noun reads as a *blocking* positional
// numeral: an arabic integer or single decimal ("2", "5.5"), a checklist range
// ("4-8"), or a Roman numeral written uppercase whose value is a plausible ordinal
// (<= XXXIX). Roman is bounded, not dropped, so a roman-numbered phase tell
// (an uppercase IV or XII after the tell noun) still blocks and cannot smuggle past the gate, while
// the ordinary uppercase abbreviations that happen to be canonical Roman (DC, CM,
// MM, MD, DIV, XL, ...) do not false-flag in a tell noun's home domain — they all
// exceed the ordinal bound. A single letter (I, V, X) is the pronoun/identifier
// collision (excluded by the length check), and a lowercase token ("mix", "dc",
// "iv") is an ordinary word, not a label (plan/0055 cast review).
fn is_blocking_numeral(lower: &str, orig: &str) -> bool {
    if lower.is_empty() {
        return false;
    }
    if is_num_range(lower) {
        return true;
    }
    // Arabic integer or single decimal (mirrors host-grammar's is_numeral arabic
    // branch): at most two non-empty all-digit parts.
    let parts: Vec<&str> = lower.split('.').collect();
    if parts.len() <= 2 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return true;
    }
    // A multi-letter Roman numeral, uppercase in the source, of plausible ordinal
    // value.
    is_numeral(lower)
        && lower.chars().count() >= 2
        && orig.chars().any(|c| c.is_alphabetic())
        && orig.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_ascii_uppercase())
        && roman_value(lower).is_some_and(|v| v <= MAX_BLOCKING_ROMAN)
}

// A closed set of spelled-out positional numbers, cardinal ("one".."twenty") and
// ordinal ("first".."twentieth"). The set is closed and small on purpose: it
// covers the ordinal-band range a plan author reaches for while a larger open
// recogniser would collide with ordinary prose. Only a phase-synonym tell noun
// immediately followed by one of these blocks, so the spelled band name is caught
// the same way the arabic one is; the checklist nouns and the domain-heavy warn
// nouns are excluded, since their spelled forms ("steps one to six", "round one",
// "part one") are ordinary English. Lowercase membership only; a spelled number
// carries no pronoun or abbreviation collision, so no case constraint is needed.
const SPELLED_ORDINALS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
    "eighteen", "nineteen", "twenty",
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth",
    "ninth", "tenth", "eleventh", "twelfth", "thirteenth", "fourteenth",
    "fifteenth", "sixteenth", "seventeenth", "eighteenth", "nineteenth", "twentieth",
];

/// Public so a property can exclude the spelled band the way it already excludes
/// the arabic one. It was private, and the property that asserts "a flag term
/// followed by a non-numeral is clean" guarded on `is_numeral` alone — which does
/// not know "six". The property therefore passed only while its `[a-z]{3,10}`
/// generator missed a forty-word set, and failed the first time it did not.
pub fn is_spelled_ordinal(word: &str) -> bool {
    SPELLED_ORDINALS.contains(&word)
}

pub fn check_line(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let orig_words: Vec<&str> = line.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        if FLAG_TERMS.contains(&clean) {
            // A tell-noun immediately followed by a blocking positional numeral.
            // Only the immediately following token counts: a numeral two words
            // away ("step into 3", "port the pass to C") is ordinary English, not
            // a positional reference (plan/0055 dropped the two-word window). The
            // glued form (the noun joined to a numeral by a hyphen) is out of
            // scope: a legitimate glued term has no numeral-free LEXICON prefix to
            // declare, so it could not be escaped, and it is the same class as a
            // noun-glued numeral. The original-case token lets a Roman numeral
            // require uppercase (IV after "Phase" blocks, "phase iv" does not).
            if let Some(next) = words.get(i + 1) {
                let next_clean = next.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                let next_orig = orig_words
                    .get(i + 1)
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
                    .unwrap_or(next_clean);
                if is_blocking_numeral(next_clean, next_orig)
                    || (!CHECKLIST_TERMS.contains(&clean) && is_spelled_ordinal(next_clean))
                {
                    return Some(clean.to_string());
                }
            }
        }
        // Sibling tell: an internal tracking code used as a name (a noun from
        // REVIEW_CODE_TERMS immediately followed by a hash+digits or
        // letter+digits label). The trim keeps a leading "#" so issue-style
        // codes survive; GitHub refs never match because closes/fixes are
        // not in the noun set.
        if REVIEW_CODE_TERMS.contains(&clean) {
            if let Some(next) = words.get(i + 1) {
                let code = next.trim_matches(|c: char| !c.is_alphanumeric() && c != '#');
                if is_review_code(code) {
                    return Some(clean.to_string());
                }
            }
        }
    }

    None
}

// flag tier: a bare numeral used as a label prefix at the start of a
// subject line, header, or comment ("5.5: exec tools", "// 5.5: ..."). The
// colon must be followed by whitespace or end-of-line so a clock time
// ("5:30 standup") does not match.
pub fn check_label_prefix(line: &str) -> Option<String> {
    let mut s = line.trim_start();
    loop {
        let stripped = s
            .strip_prefix("///")
            .or_else(|| s.strip_prefix("//!"))
            .or_else(|| s.strip_prefix("//"))
            .or_else(|| s.strip_prefix("/**"))
            .or_else(|| s.strip_prefix("/*"))
            .or_else(|| s.strip_prefix("--"))
            .or_else(|| s.strip_prefix("*"))
            .or_else(|| s.strip_prefix("#"));
        match stripped {
            Some(rest) => s = rest.trim_start(),
            None => break,
        }
    }
    let code: String = s.chars().take_while(|&c| c.is_ascii_digit() || c == '.').collect();
    if code.is_empty() || !is_numeral(&code) {
        return None;
    }
    // A bare integer of three or more digits reads as a status code or numeric
    // key ("200: OK", "404: not found"), not a milestone label. The dotted form
    // ("5.5:") and short ordinals ("3:") still flag (plan/0055).
    if !code.contains('.') && code.len() >= 3 {
        return None;
    }
    let mut after = s[code.len()..].chars();
    if after.next() == Some(':') {
        match after.next() {
            None => return Some(code),
            Some(c) if c.is_whitespace() => return Some(code),
            _ => {}
        }
    }
    None
}

// warn tier: the bare-numeral degenerate form with the noun elided — a
// filing-system code noun followed by a numeral, or a bare dotted code used as
// a name outside version/quantity contexts. Advisory only. The `_with_units`
// form consults a project's declared domain units (host-lint#21); the plain
// form is the no-units baseline the tests and embedders use.
pub fn check_warn(line: &str) -> Option<String> {
    check_warn_with_units(line, &[])
}

pub fn check_warn_with_units(line: &str, units: &[String]) -> Option<String> {
    let lower = line.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let orig: Vec<&str> = line.split_whitespace().collect();

    for i in 0..words.len() {
        let word = words[i];
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        // ordinal-noun rule: a demoted verb/measurement noun immediately followed by a
        // blocking positional numeral ("pass 2", "round 2", "level 3"). Advisory,
        // because the noun's ordinary verb/measurement use is indistinguishable
        // (plan/0055, call/0037). Immediate adjacency only, so "step into 3" and
        // "port the pass to C" stay clean.
        if WARN_ORDINAL_TERMS.contains(&clean) {
            if let Some(next) = words.get(i + 1) {
                let nc = next.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                let no = orig
                    .get(i + 1)
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
                    .unwrap_or(nc);
                if is_blocking_numeral(nc, no) {
                    return Some(clean.to_string());
                }
            }
        }
        // filing-code-noun rule: a filing-code noun followed by a numeral (within two words).
        if WARN_NOUNS.contains(&clean) {
            for k in 1..=2 {
                if let Some(next) = words.get(i + k) {
                    let nc = next.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                    if is_numeral(nc) {
                        return Some(clean.to_string());
                    }
                }
            }
        }
        // bare-dotted-code rule: a bare dotted code ("5.5") used as a name. A token carrying a
        // letter ("v2.1") is a version string and is left alone.
        let ow = if i < orig.len() { orig[i] } else { word };
        if ow.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let code = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        if is_dotted_code(code) {
            if ow.ends_with('%') {
                continue;
            }
            // A dotted code inside a markdown table cell is a data value, not a
            // name (host-lint#21): the column header that carries the unit sits on
            // another line the token-local rule cannot see. A numeral fused to a
            // pipe or flanked by standalone pipe tokens is structurally a cell, so
            // it is skipped. The noun-gated flag tier is unaffected, so a real
            // tell still flags inside a cell.
            if ow.contains('|')
                || (i > 0 && words[i - 1] == "|")
                || matches!(words.get(i + 1), Some(&"|"))
            {
                continue;
            }
            // A dotted code carrying an approximation or comparison operator is a
            // quantity, not a name (host-lint#21). The operator may be fused to the
            // code or stand as a token before it; a trailing multiplier marks a
            // quantity too.
            const QTY_LEAD: &[char] = &['≈', '~', '>', '<', '≥', '≤', '±', '='];
            if ow.chars().next().is_some_and(|c| QTY_LEAD.contains(&c))
                || ow.ends_with('×')
                || (i > 0
                    && !words[i - 1].is_empty()
                    && words[i - 1].chars().all(|c| QTY_LEAD.contains(&c)))
                || words.get(i + 1).is_some_and(|n| n.starts_with('×'))
            {
                continue;
            }
            if let Some(next) = words.get(i + 1) {
                let nc = next.trim_matches(|c: char| !c.is_alphanumeric());
                if UNITS.contains(&nc) || is_compound_unit(next) || units.iter().any(|u| u.as_str() == nc) {
                    continue;
                }
            }
            if i > 0 {
                let pc = words[i - 1].trim_matches(|c: char| !c.is_alphanumeric());
                if PREV_SKIP.contains(&pc) {
                    continue;
                }
                // A version/product designator in all-caps ("NT 3.1", "SDK 2.1",
                // "DOS 6.2") reads as a version string, not a milestone code.
                // Title-case nouns ("Decision 2.1") and ordinary lowercase words
                // ("in 2.1") still warn — only an all-uppercase acronym is skipped.
                if let Some(prev) = orig.get(i - 1) {
                    let po = prev.trim_matches(|c: char| !c.is_alphanumeric());
                    if po.len() >= 2 && po.chars().all(|c| c.is_ascii_uppercase()) {
                        continue;
                    }
                }
            }
            return Some(code.to_string());
        }
    }

    None
}

// warn tier: a bare review-code (one letter + digits, e.g. "F1", "B2")
// used as a leading label — the first non-bullet token of a line, immediately
// followed by a label delimiter (an em/en-dash token, or a trailing colon on
// the code itself). This is the section-5 code-as-name tell in its bare leading
// form, where no review/finding/blocker noun precedes the code (a PR body that
// structures its fixes as "F1 — …", "F2 — …"). Warned, not flagged: a
// multi-letter device noun ("COM1") is already excluded by the one-letter code
// shape, but a single-letter hardware reference designator ("R1 — 10kΩ
// resistor") fits the same shape, so the call is advisory rather than blocking.
pub fn check_code_label_prefix(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    // The first token with an alphanumeric core — skips leading bullets and
    // emphasis markers ("-", "*", "**", "//", "#") so a markdown list item
    // ("- **F1** — …") is read the same as a bare "F1 — …".
    let start = tokens
        .iter()
        .position(|t| t.chars().any(|c| c.is_alphanumeric()))?;
    let raw = tokens[start];
    // "F2:" — the code carries its own label colon, no following dash needed.
    if let Some(stem) = raw.strip_suffix(':') {
        let core = stem.trim_matches(|c: char| !c.is_alphanumeric());
        if is_review_code(core) {
            return Some(core.to_string());
        }
    }
    let core = raw.trim_matches(|c: char| !c.is_alphanumeric());
    if !is_review_code(core) {
        return None;
    }
    // "F1 —" — a bare dash delimiter as the next whitespace token.
    match tokens.get(start + 1).copied() {
        Some("—") | Some("–") | Some("-") => Some(core.to_string()),
        _ => None,
    }
}

pub fn check_bare_numeral_header(line: &str) -> Option<String> {
    let t = line.trim();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = t[hashes..].trim();
    let parts: Vec<&str> = rest.split('.').collect();
    if parts.len() > 2 {
        // version-like heading (1.2.3), not a bare ordinal
        return None;
    }
    if parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        // A four-digit (or longer) component reads as a year, not a bare ordinal:
        // a changelog "## 2024" or "## 2024.01" heading is not a position label.
        if parts.iter().any(|p| p.len() >= 4) {
            return None;
        }
        // A bare integer heading of three or more digits reads as a status code or
        // numeric key ("## 404", "## 200", "## 500"), not a bare ordinal section.
        // Mirrors the label-prefix status-code guard; a short ordinal ("## 3",
        // "## 12") and the dotted milestone code ("## 5.5") still flag (plan/0055).
        if parts.len() == 1 && rest.len() >= 3 {
            return None;
        }
        return Some(rest.to_string());
    }
    None
}

/// A heading that is nothing but a noun and a cardinal: `## Check 1`, `## Part 2`.
///
/// `FLAG_TERMS` is a vocabulary, and a vocabulary is a proxy for the tell rather than
/// the tell itself. Renaming a heading from a listed noun to an unlisted one (`## Check 1`)
/// passed the linter and kept the tell entirely — still a heading naming a *position*
/// instead of content (host-lint#24). This catches the shape, so a noun-swap cannot
/// evade it. The example is given unlisted-side only, because writing the listed spelling
/// here would flag this very comment.
///
/// Deliberately narrow, because a generic rule earns its false positives: the WHOLE
/// heading must be one word and one integer. `## MCP tool surface passes` is untouched,
/// and so is anything carrying more words. A dotted number is a version, and a long
/// integer is a year or a status code, both already judged by the bare-numeral rule
/// beside this one; a genuine `Windows 11` heading is what the `LEXICON` is for.
pub fn check_ordinal_scaffold_header(line: &str) -> Option<String> {
    let t = line.trim();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = t[hashes..].trim().trim_end_matches([':', '.', '—', '-']).trim();
    let mut words = rest.split_whitespace();
    let (Some(noun), Some(number), None) = (words.next(), words.next(), words.next()) else {
        return None;
    };
    // The noun must be a word, not a code or a numeral: `## v2 3` is not this shape.
    if !noun.chars().all(|c| c.is_alphabetic()) || noun.len() < 2 {
        return None;
    }
    // A plain integer only. A dotted or long number is a version, a year, or a status
    // code, and those are the bare-numeral rule's business rather than this one's.
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) || number.len() >= 3 {
        return None;
    }
    // The vocabulary already flags its own nouns; this rule exists for the ones it misses,
    // so it does not double-report.
    if FLAG_TERMS.contains(&noun.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(rest.to_string())
}

// Classify a single line, preferring the most severe outcome: a confirmed tell
// (flag) wins over the bare-numeral degenerate form (warn).
/// Co-Authored-By is a discretionary attribution trailer: a co-author's name
/// or a tool's version string (e.g. "Claude Opus 4.8") is the author's to set,
/// not ours to police. Respect it — never flag or warn on this line.
fn is_coauthor_trailer(line: &str) -> bool {
    let key = "co-authored-by:";
    line.trim_start()
        .get(..key.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(key))
}

pub fn classify_line(line: &str, markdown: bool) -> Option<(Severity, String)> {
    classify_line_with_units(line, markdown, &[])
}

pub fn classify_line_with_units(line: &str, markdown: bool, units: &[String]) -> Option<(Severity, String)> {
    if is_coauthor_trailer(line) {
        return None;
    }
    if let Some(t) = check_line(line) {
        return Some((Severity::Flag, t));
    }
    if let Some(t) = check_label_prefix(line) {
        return Some((Severity::Flag, t));
    }
    if markdown {
        if let Some(t) = check_bare_numeral_header(line) {
            return Some((Severity::Flag, t));
        }
        // The shape rather than the vocabulary, so a noun-swap cannot evade the
        // lexical set (host-lint#24). Heading context only, where a bare
        // `<noun> <cardinal>` is a position label rather than ordinary prose.
        // Warn, not flag: unlike the vocabulary rules, this shape admits genuine
        // designators (`## Windows 11`), so an undeclared hit asks the author to
        // confirm and declare it in the LEXICON; strict escalates it with that
        // remedy, and a declared designator is masked before this runs.
        if let Some(t) = check_ordinal_scaffold_header(line) {
            return Some((Severity::Warn, t));
        }
    }
    if let Some(t) = check_warn_with_units(line, units) {
        return Some((Severity::Warn, t));
    }
    if let Some(t) = check_code_label_prefix(line) {
        return Some((Severity::Warn, t));
    }
    None
}

// Blank out every word-boundaried, case-insensitive occurrence of a sanctioned
// phrase so the classifier never sees it. The boundary requirement (a
// non-alphanumeric neighbour or a string edge on each side) is what keeps an
// allow entry specific: a sanctioned phrase masks only its exact occurrence, not
// a longer tell that merely shares its prefix, so allow-listing one occurrence
// cannot silently clear another. `allow_lc` entries are pre-lowercased (ASCII) by
// the caller; ASCII-only folding keeps byte indices aligned between the search
// copy and the original.
fn mask_allowed(line: &str, allow_lc: &[String]) -> String {
    if allow_lc.is_empty() {
        return line.to_string();
    }
    let lower = line.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut out = line.as_bytes().to_vec();
    for p in allow_lc {
        if p.is_empty() {
            continue;
        }
        let mut start = 0;
        while let Some(rel) = lower[start..].find(p.as_str()) {
            let at = start + rel;
            let end = at + p.len();
            let left_ok = at == 0 || !lb[at - 1].is_ascii_alphanumeric();
            let right_ok = end == lb.len() || !lb[end].is_ascii_alphanumeric();
            if left_ok && right_ok {
                for b in &mut out[at..end] {
                    *b = b' ';
                }
            }
            start = end;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

// === LEXICON: the provenance-enforced contextual allowlist (issue #13) ===
//
// A LEXICON file is the sole source of truth for tell-shaped tokens that are
// legitimate vocabulary in a project (`Windows 3.1`, `COM1`) or cited tracker
// references (`#7 https://…`). Each entry is the *full contextual phrase* that is
// masked before detection; a bare numeral is never an entry. Because a sound,
// declarable escape now exists, the naming-warn tier can escalate WARN -> ERROR
// under the committed `strict` directive. The guards below keep a weak agent (or
// a careless hand-edit) from abusing the escape.

/// One parsed LEXICON entry: the contextual `phrase` masked before detection, and
/// the optional cited `url` recorded as provenance (a tracker reference must carry
/// one). The URL is metadata only — it is never masked, only the phrase is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexiconEntry {
    pub phrase: String,
    pub url: Option<String>,
}

/// The `# host-lint: strict` directive that turns on warn->error escalation. A
/// comment-shaped line so it is invisible to the phrase parser, explicit so it is
/// auditable in the committed file.
pub fn is_strict_directive(line: &str) -> bool {
    line.trim()
        .strip_prefix('#')
        .is_some_and(|r| r.trim() == "host-lint: strict")
}

/// The `root` directive: this LEXICON is the top of its own scope, so the search
/// that found it stops here rather than continuing to an enclosing repository.
///
/// It exists because a repository can contain another one. A vendored template,
/// a submodule, a subtree: its documents are governed by its own vocabulary, and
/// inheriting the enclosing project's would legitimize a token there that the
/// inner repository never declared. Declaring `root` in the inner LEXICON keeps
/// the two apart, and the outer project needs to know nothing about the inner.
pub fn is_root_directive(line: &str) -> bool {
    line.trim()
        .strip_prefix('#')
        .is_some_and(|r| r.trim() == "host-lint: root")
}

/// Parse one LEXICON line into an entry, or `None` for a blank, comment, or
/// directive line. A comment is `#` followed by a non-digit (so `# note` and
/// `## heading` are comments, but `#7 …` is a hash-number entry — this is what
/// keeps the comment marker from colliding with the `#N` reference shape). A
/// trailing `http(s)://…` whitespace token is split off as the cited URL.
pub fn parse_lexicon_line(line: &str) -> Option<LexiconEntry> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('#') {
        // `#` then a non-digit (or nothing) is a comment/directive, not an entry.
        if !rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    if let Some((head, last)) = t.rsplit_once(char::is_whitespace) {
        if last.starts_with("http://") || last.starts_with("https://") {
            return Some(LexiconEntry {
                phrase: head.trim_end().to_string(),
                url: Some(last.to_string()),
            });
        }
    }
    Some(LexiconEntry { phrase: t.to_string(), url: None })
}

/// A jira-key project key (`PROJ`, `TEAM2`): an uppercase letter then uppercase
/// letters or digits. A LEXICON opts a key into citation-gating via the directive
/// `# host-lint: jira-key <KEY>`.
fn is_jira_key(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Parse a `# host-lint: jira-key <KEY> [<KEY>...]` directive into its declared
/// project keys, or `None` for any other line. Comment-shaped, so the phrase
/// parser ignores it — the same idiom as the strict directive.
pub fn parse_jira_keys(line: &str) -> Option<Vec<String>> {
    let body = line.trim().strip_prefix('#')?.trim();
    let rest = body.strip_prefix("host-lint: jira-key")?;
    if !rest.is_empty() && !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let keys: Vec<String> = rest.split_whitespace().filter(|k| is_jira_key(k)).map(String::from).collect();
    if keys.is_empty() { None } else { Some(keys) }
}

/// Parse a `# host-lint: unit <TOKEN>` directive into the declared unit token
/// (lowercased), or `None` for any other line. A unit declares a domain measurement
/// token (GFLOPS, ppm, dB) so the bare-dotted-code rule treats a numeral followed by
/// one as a quantity, not a name (host-lint#21). Comment-shaped, so the phrase parser
/// ignores it — the same idiom as jira-key. A unit is a single whitespace-free token.
pub fn parse_unit_directive(line: &str) -> Option<String> {
    let body = line.trim().strip_prefix('#')?.trim();
    let rest = body.strip_prefix("host-lint: unit")?;
    if !rest.is_empty() && !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let tok = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(tok.to_ascii_lowercase())
}

/// A bare tracker reference whose only provenance is a URL: `#N`, `owner/repo#N`,
/// or an opted-in jira-key `PROJ-NNNN`. The offline matcher cannot tell a real
/// `#7` from a phantom `#999`, so an entry of this shape must carry a URL.
/// `PROJ-NNNN` is gated ONLY for a project key the LEXICON declares (`# host-lint:
/// jira-key PROJ`) — opt-in, because the shape is identical to standards tokens the
/// host writes (`RFC-2119`, `UTF-8`), which must stay plain vocabulary by default.
fn is_tracker_ref(phrase: &str, jira_keys: &[String]) -> bool {
    if let Some(d) = phrase.strip_prefix('#') {
        return !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit());
    }
    if let Some((path, num)) = phrase.split_once('#') {
        let segs: Vec<&str> = path.split('/').collect();
        if segs.len() == 2
            && segs.iter().all(|s| !s.is_empty())
            && !num.is_empty()
            && num.bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    if let Some((key, num)) = phrase.split_once('-') {
        if jira_keys.iter().any(|k| k == key)
            && !num.is_empty()
            && num.bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

/// Validate one entry for registration. `Ok(())` means it may be trusted to mask;
/// `Err(reason)` is a human-actionable rejection. Three guards, all reusing the
/// detection engine rather than inventing new tell logic:
///   - **citation gate** — a bare tracker ref (`#N`, `owner/repo#N`, or an opted-in
///     jira-key `PROJ-NNNN`) must carry a URL. `jira_keys` are the project keys the
///     LEXICON declared; empty = no jira-key gating, so `RFC-2119` stays vocabulary.
///   - **master-key guard** — a non-reference phrase must hold at least one letter, so a
///     bare `5.5` (which would silently clear every occurrence tree-wide) is refused.
///   - **no-laundering guard** — a phrase that is *itself* a flag-tier tell (a phase-synonym
///     label, say) is refused: you rename a real tell, you do not allow-list it. A phrase
///     that merely *carries* a position noun as a standalone word (`phase`, `step`, `review`)
///     is refused for the same reason: masking it would blank that noun out of a real
///     `<noun> N` tell, silencing the whole class and defeating strict (plan/0055). A mere
///     warn-tier phrase with no such noun (`Windows 3.1`, `Decision 2.1`) is the legitimate
///     case, accepted.
pub fn validate_lexicon_entry(e: &LexiconEntry, jira_keys: &[String]) -> Result<(), String> {
    if e.phrase.is_empty() {
        return Err("empty phrase".to_string());
    }
    if is_tracker_ref(&e.phrase, jira_keys) {
        let url = match &e.url {
            None => {
                return Err(format!(
                    "'{}' is a tracker reference with no URL — register it as '{} <url>' so the link is provenance, not a phantom",
                    e.phrase, e.phrase
                ))
            }
            Some(u) => u,
        };
        // Offline provenance: the cited URL must actually reference the same number,
        // so a phantom '#999' cited to an unrelated link cannot mask a real '#999'
        // sitting after the review noun (plan/0055). Liveness (the link resolves) still needs the
        // explicit `lexicon --check-urls` lane; a network fetch does not gate by default.
        let number: String = e.phrase.chars().filter(|c| c.is_ascii_digit()).collect();
        if !number.is_empty() && !url.contains(&number) {
            return Err(format!(
                "'{}' cites '{}', which does not reference {} — cite the URL that actually points to the tracker item",
                e.phrase, url, number
            ));
        }
        return Ok(());
    }
    if !e.phrase.chars().any(|c| c.is_ascii_alphabetic()) {
        return Err(format!(
            "'{}' is a bare numeral/code — a master key that would clear every occurrence; add the legitimizing word (e.g. 'Windows {}') or rename the work",
            e.phrase, e.phrase
        ));
    }
    if let Some((Severity::Flag, term)) = classify_line(&e.phrase, false) {
        return Err(format!(
            "'{}' is itself a tell ({}) — rename the work after its content; the lexicon legitimizes vocabulary, it does not silence real tells",
            e.phrase, term
        ));
    }
    // A phrase that carries a position noun OR a bare review code as a standalone
    // word would, when masked, blank that token out of a real tell — a "<noun> N"
    // flag, or a "review <code>" flag — silencing the whole class repo-wide and
    // defeating strict (the masked line never produces the warn strict escalates).
    // Refuse it (plan/0055). The review-code half matters because the no-laundering
    // guard must close both the noun and the code path: registering "F1" launders
    // every "review F1"/"finding F1". A cited tracker ref ("#7 <url>") is allowed
    // above; this catches the un-cited bare codes. Over-strict by design: a
    // legitimate multiword phrase carrying such a token is rephrased; safety beats
    // permissiveness here.
    if let Some(token) = e.phrase.split_whitespace().find_map(|w| {
        let t = w
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
            .to_ascii_lowercase();
        (FLAG_TERMS.contains(&t.as_str())
            || WARN_ORDINAL_TERMS.contains(&t.as_str())
            || REVIEW_CODE_TERMS.contains(&t.as_str())
            || WARN_NOUNS.contains(&t.as_str())
            || is_review_code(&t))
        .then_some(t)
    }) {
        return Err(format!(
            "'{}' carries the position noun or tracking code '{}' as a word — masking it would blank that token out of a real tell ('{} N' or 'review {}'); rename the work after its content rather than allow-list the tell shape",
            e.phrase, token, token, token
        ));
    }
    Ok(())
}

/// The repo's LEXICON (issue #13): the validated allowlist phrases (lowercased for
/// case-insensitive masking), the committed `strict` flag, the declared tracker keys,
/// and the parsed entries (for the `lexicon` subcommand). Lives in the shared engine so
/// host-lint's binary and an in-process embedder (host-lifecycle) load and mask the same
/// declared phrases identically (host-lifecycle#2).
pub struct Lexicon {
    pub phrases_lc: Vec<String>,
    pub strict: bool,
    pub jira_keys: Vec<String>,
    pub units: Vec<String>,
    pub entries: Vec<LexiconEntry>,
    /// This file declared `host-lint: root`, so a search that reached it stops
    /// rather than continuing into an enclosing repository.
    pub is_root: bool,
}

/// Read and validate the repo's `LEXICON` file (at `root`). An invalid entry — a master
/// key, a tracker ref with no URL, a laundered tell — is reported to stderr and dropped:
/// it never masks, so soundness does not depend on the file being hand-edited correctly.
/// A missing file yields an empty lexicon (the feature is opt-in per repo). The single
/// loader both the CLI and an embedder call, so the prose/`--docs` lane masks the same
/// declared phrases everywhere.
pub fn load_lexicon(root: &Path) -> Lexicon {
    let mut lex = Lexicon { phrases_lc: Vec::new(), strict: false, jira_keys: Vec::new(), units: Vec::new(), entries: Vec::new(), is_root: false };
    if root.as_os_str().is_empty() {
        return lex;
    }
    let content = match fs::read_to_string(root.join("LEXICON")) {
        Ok(c) => c,
        Err(_) => return lex,
    };
    // Directives first (strict, jira-key, unit), collected before any entry so an
    // entry's validation sees every declared key regardless of line order.
    for line in content.lines() {
        if is_strict_directive(line) {
            lex.strict = true;
        } else if is_root_directive(line) {
            lex.is_root = true;
        } else if let Some(keys) = parse_jira_keys(line) {
            lex.jira_keys.extend(keys);
        } else if let Some(u) = parse_unit_directive(line) {
            lex.units.push(u);
        }
    }
    // Then the entries, validated against the collected directives.
    for line in content.lines() {
        if is_strict_directive(line)
            || is_root_directive(line)
            || parse_jira_keys(line).is_some()
            || parse_unit_directive(line).is_some()
        {
            continue;
        }
        let Some(entry) = parse_lexicon_line(line) else { continue };
        if let Err(reason) = validate_lexicon_entry(&entry, &lex.jira_keys) {
            eprintln!("host-lint: LEXICON entry ignored ({reason})");
            continue;
        }
        lex.phrases_lc.push(entry.phrase.to_ascii_lowercase());
        lex.entries.push(entry);
    }
    lex
}

/// Resolve the lexicon governing `dir`, nearest first, stopping at `stop_at` or
/// at the first LEXICON declaring `host-lint: root`.
///
/// One repository can contain another, so "the repo's LEXICON" is not a single
/// file. The search starts beside the document being scanned and walks outward,
/// which is what makes a vendored subtree answer to its own vocabulary rather
/// than to whichever directory the command happened to run from. Before this,
/// the lexicon came from the invocation root, so linting an inner repository's
/// document from the outer one masked it with the outer project's phrases and
/// reported a defect that belonged to neither.
///
/// Phrases, tracker keys and units accumulate outward, because an allowlist is
/// additive: a nearer file adds to what encloses it rather than replacing it.
/// `strict` holds if any file in the chain declares it, so an inner scope can
/// never quietly relax an outer escalation. A scope that wants none of the
/// enclosing vocabulary says so with `root`, which is the only way to stop
/// inheriting and is visible in the file that chooses it.
pub fn resolve_lexicon(dir: &Path, stop_at: &Path) -> Lexicon {
    let mut merged = Lexicon {
        phrases_lc: Vec::new(),
        strict: false,
        jira_keys: Vec::new(),
        units: Vec::new(),
        entries: Vec::new(),
        is_root: false,
    };
    let mut here = Some(dir.to_path_buf());
    while let Some(d) = here {
        if d.join("LEXICON").exists() {
            let lex = load_lexicon(&d);
            merged.phrases_lc.extend(lex.phrases_lc);
            merged.jira_keys.extend(lex.jira_keys);
            merged.units.extend(lex.units);
            merged.entries.extend(lex.entries);
            merged.strict |= lex.strict;
            if lex.is_root {
                merged.is_root = true;
                break;
            }
        }
        // `stop_at` bounds the walk so a scan never reads a LEXICON from outside
        // the tree under audit, which would let a file above the repository
        // decide what this one may say.
        if d == stop_at {
            break;
        }
        here = d.parent().map(Path::to_path_buf);
    }
    merged.phrases_lc.sort_unstable();
    merged.phrases_lc.dedup();
    merged
}

/// A `resolve_lexicon` that remembers, so a walk of many documents reads each
/// directory's LEXICON once rather than once per file.
pub struct LexiconScopes {
    stop_at: PathBuf,
    seen: RefCell<HashMap<PathBuf, Rc<Lexicon>>>,
}

impl LexiconScopes {
    pub fn new(stop_at: &Path) -> Self {
        LexiconScopes { stop_at: stop_at.to_path_buf(), seen: RefCell::new(HashMap::new()) }
    }

    /// The lexicon governing `file`, which is the one resolved from its parent
    /// directory. A path with no parent falls back to the bounding root.
    pub fn for_file(&self, file: &Path) -> Rc<Lexicon> {
        // A bare filename's parent is the empty path, not `None`, and an empty
        // root reads no lexicon at all. Both spellings mean the same directory,
        // so both resolve to the bounding root rather than one silently
        // returning nothing.
        let dir = match file.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => self.stop_at.clone(),
        };
        if let Some(hit) = self.seen.borrow().get(&dir) {
            return Rc::clone(hit);
        }
        let lex = Rc::new(resolve_lexicon(&dir, &self.stop_at));
        self.seen.borrow_mut().insert(dir, Rc::clone(&lex));
        lex
    }
}


pub fn scan_text(input: &str, source: &str, matches: &mut Vec<Match>) {
    scan_text_with_allow(input, source, &[], matches);
}

// As `scan_text`, but a repo's sanctioned phrases (the LEXICON, ASCII-lowercased
// by the caller) are masked out of each line before classification. A line still
// flags on any tell the mask leaves behind, and the reported `text` is the
// original line so the author sees real context. Non-strict: the warn tier stays
// advisory (this is the entry point external callers keep).
pub fn scan_text_with_allow(
    input: &str,
    source: &str,
    allow_lc: &[String],
    matches: &mut Vec<Match>,
) {
    scan_text_with_allow_strict(input, source, allow_lc, &[], false, matches);
}

// The info string of a markdown fence line (the text after ``` / ~~~), or `None`
// if `line` is not a fence. A fence is opened by 3+ backticks or tildes with at
// most 3 leading spaces; 4+ spaces is an indented code block, not a fence. A bare
// fence (empty info) closes a block; `host-lint:ignore` as the info opens a region
// the naming scan skips (call/0019). Used only for markdown sources.
fn fence_info(line: &str) -> Option<(char, usize, &str)> {
    if line.chars().take_while(|c| *c == ' ').count() >= 4 {
        return None;
    }
    let t = line.trim_start();
    let marker = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = t.chars().take_while(|c| *c == marker).count();
    if run < 3 {
        return None;
    }
    Some((marker, run, t[run..].trim()))
}

// The full scan: under `strict`, a naming-warn the mask did not clear escalates to
// a blocking flag (issue #13 — the LEXICON makes a sound escape declarable, so an
// *un*-declared tell-shaped token is now a hard signal, not merely advisory). The
// escalated match carries a remedy in `cite` so the audience can act. Prose tells
// (host-grammar) are a different tier and are not escalated here.
pub fn scan_text_with_allow_strict(
    input: &str,
    source: &str,
    allow_lc: &[String],
    units: &[String],
    strict: bool,
    matches: &mut Vec<Match>,
) {
    let markdown = source.to_lowercase().ends_with(".md");
    // A `host-lint:ignore` fenced block quarantines literal reference content (the
    // retired-ordinal dictionary, archived citations) — its lines are skipped, fences
    // included (call/0019). Markdown only; a regular code block and inline backticks
    // stay linted, so a tell cannot be laundered by inline-quoting it.
    // The open ignore fence's marker char and run length, or None when outside a
    // block. Closing requires a bare fence of the *same* marker at least as long
    // (CommonMark), so an inner code sample with a shorter fence does not leak the
    // quarantine (plan/0055), and a longer outer fence can wrap it.
    let mut ignore_fence: Option<(char, usize)> = None;
    let mut last_line = 0usize;
    for (i, line) in input.lines().enumerate() {
        last_line = i + 1;
        if markdown {
            if let Some((mch, mlen)) = ignore_fence {
                if let Some((c, len, info)) = fence_info(line) {
                    if info.is_empty() && c == mch && len >= mlen {
                        ignore_fence = None;
                    }
                }
                continue;
            }
            if let Some((c, len, info)) = fence_info(line) {
                if info == "host-lint:ignore" {
                    ignore_fence = Some((c, len));
                    continue;
                }
            }
        }
        let scanned = mask_allowed(line, allow_lc);
        if let Some((mut severity, term)) = classify_line_with_units(&scanned, markdown, units) {
            let mut cite = String::new();
            if strict && severity == Severity::Warn {
                severity = Severity::Flag;
                cite = "not in LEXICON; rename or run: host-lint lexicon add".to_string();
            }
            matches.push(Match {
                file: source.to_string(),
                line: i + 1,
                col: 0,
                text: line.trim().to_string(),
                term,
                severity,
                cite,
            });
        }
    }
    // An ignore fence left open at end of file silently skipped every line after it
    // (the fail-open the loop's `continue` produced). Fail loud: report it as a flag
    // so the file is never reported clean over content it never scanned (plan/0055).
    if ignore_fence.is_some() {
        matches.push(Match {
            file: source.to_string(),
            line: last_line,
            col: 0,
            text: "unclosed host-lint:ignore fence".to_string(),
            term: "unclosed-ignore-fence".to_string(),
            severity: Severity::Flag,
            cite: "close the ```host-lint:ignore block with a bare fence; an unclosed block skips the rest of the file".to_string(),
        });
    }
}

// Push a prose tell as a Match — a free helper so the occurrence-mapping loop reads
// cleanly.
#[allow(clippy::too_many_arguments)]
fn push_tell(
    matches: &mut Vec<Match>,
    source: &str,
    line: usize,
    col: usize,
    text: &str,
    term: &str,
    severity: Severity,
    cite: &str,
) {
    matches.push(Match {
        file: source.to_string(),
        line,
        col,
        text: text.to_string(),
        term: term.to_string(),
        severity,
        cite: cite.to_string(),
    });
}

// The 1-based (line, column) of byte offset `off` in `input`; the column counts
// characters from the line start.
fn line_col(input: &str, off: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in input.char_indices() {
        if i >= off {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// Best-effort line of a multi-word excerpt the markdown extractor may have normalised
// out of the raw source: the 1-based line containing the excerpt's first few words, or
// None when even that is absent (a synthetic whole-document diagnosis). Both arguments
// are already ascii-lowercased, so the match is case-insensitive.
fn probe_line(input_lc: &str, needle_lc: &str) -> Option<usize> {
    let probe = needle_lc.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    if probe.is_empty() {
        return None;
    }
    input_lc.lines().position(|l| l.contains(&probe)).map(|i| i + 1)
}

// Find `needle` in `haystack` at or after byte offset `from`, returning its
// absolute offset. When the needle is a single alphanumeric word it must sit on
// word boundaries, so a short tell ("delve") does not map onto a longer word that
// merely contains it ("delved"); a multi-word or punctuation excerpt matches as-is
// (plan/0055). Both arguments are already ascii-lowercased.
fn find_tell(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let single_word = !needle.is_empty()
        && !needle.contains(char::is_whitespace)
        && needle.chars().all(|c| c.is_alphanumeric());
    let mut search = from;
    while let Some(rel) = haystack.get(search..).and_then(|s| s.find(needle)) {
        let off = search + rel;
        let end = off + needle.len();
        if !single_word {
            return Some(off);
        }
        let left_ok = haystack[..off].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let right_ok = haystack[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if left_ok && right_ok {
            return Some(off);
        }
        search = end.max(off + 1);
    }
    None
}

// === The commit verb's duplication check (host-lint#28) ===
//
// A sentence appearing in both an added comment and the message body is recorded
// twice, and the message copy goes stale when the comment is next edited. The
// check is a pure comparison over the two texts the caller hands in; nothing here
// reads a repository. The sentence granularity, the eight-word bar, and the
// body-only boundary are calibrated settings (host-lint#28).

/// The comment text of an added unified-diff line, or `None` for every other line.
/// The gate is the added line's own leading shape (`#`, `//` and its doc forms,
/// `/*`, a block continuation `* `, `--` with a following space, `<!--`); a
/// heading in added markdown is comment-shaped by this gate.
pub fn added_comment_text(line: &str) -> Option<String> {
    let added = line.strip_prefix('+')?;
    if added.starts_with("++") {
        return None; // the +++ file header of the diff, not content
    }
    let t = added.trim_start();
    let rest = if let Some(r) = t.strip_prefix("<!--") {
        r
    } else if t.starts_with("//") {
        let r = t.trim_start_matches('/');
        r.strip_prefix('!').unwrap_or(r)
    } else if let Some(r) = t.strip_prefix("/*") {
        r.trim_start_matches('*')
    } else if t.starts_with('#') {
        t.trim_start_matches('#')
    } else if t.starts_with("--") && t.chars().nth(2).is_some_and(|c| c.is_whitespace()) {
        t.trim_start_matches('-')
    } else {
        t.strip_prefix("* ")?
    };
    Some(rest.strip_prefix(' ').unwrap_or(rest).trim_end().to_string())
}

/// ASCII-lowercased, punctuation replaced by spaces, whitespace collapsed to
/// single spaces: the comparison form for both sides of the restatement check.
pub fn normalize_for_restate(s: &str) -> String {
    const PUNCT: &[char] = &[
        '`', '*', '_', '\'', '"', '“', '”', '‘', '’', '(', ')', '.', ',', ';', ':',
        '!', '?', '[', ']', '{', '}', '<', '>', '#', '=', '|', '-',
    ];
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if PUNCT.contains(&c) || c.is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Sentences split on a run of `.`/`!`/`?` at a whitespace-or-end boundary, so a
/// version number (`v3.5 is`) never splits mid-token.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '.' | '!' | '?') {
            let mut run = String::from(c);
            while let Some(&p) = chars.peek() {
                if matches!(p, '.' | '!' | '?') {
                    run.push(p);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek().map(|p| p.is_whitespace()).unwrap_or(true) {
                let s = cur.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
                cur.clear();
            } else {
                cur.push_str(&run);
            }
        } else {
            cur.push(c);
        }
    }
    let s = cur.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
    out
}

/// The distinct sentences of eight-plus normalized words from a diff's added
/// comment runs whose normalized form appears inside the normalized `body`. The
/// caller passes the message BODY, never the whole message: a subject that
/// matches an added title is the record-title convention.
pub fn restated_comment_sentences(diff: &str, body: &str) -> Vec<String> {
    let nbody = normalize_for_restate(body);
    if nbody.is_empty() {
        return Vec::new();
    }
    let mut runs: Vec<String> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for line in diff.lines() {
        match added_comment_text(line) {
            Some(t) => cur.push(t),
            None => {
                if !cur.is_empty() {
                    runs.push(cur.join(" "));
                    cur.clear();
                }
            }
        }
    }
    if !cur.is_empty() {
        runs.push(cur.join(" "));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for run in &runs {
        for s in split_sentences(run) {
            let n = normalize_for_restate(&s);
            if !n.is_empty() && n.split(' ').count() >= 8 && nbody.contains(&n) && seen.insert(n) {
                out.push(s);
            }
        }
    }
    out
}

/// Scan `input` as prose for agentic tells (the host-grammar engine), pushing
/// each as an advisory `Warn` match, plus one document-level match when the tell
/// density crosses the threshold. Used for titles, comments, and `--prose` docs;
/// never blocks (Warn = exit 3), so a flagged draft still lands.
pub fn scan_prose_text(input: &str, source: &str, allow_lc: &[String], matches: &mut Vec<Match>) {
    // A markdown source is scanned structurally (code blocks excluded, headings
    // not counted as paragraphs); anything else as plain prose.
    let markdown = source.to_lowercase().ends_with(".md");
    // Mask the sanctioned LEXICON phrases before the density engine sees them, the
    // same pre-detection blank-out the naming lane performs (issue #16): a declared
    // domain phrase like `rehost harness` clears the trope on `harness` within that
    // phrase, while a standalone occurrence still flags. `mask_allowed` is
    // byte-length-preserving, so an offset into `masked` indexes `input` unchanged
    // (the reported excerpt stays the author's own text), and the masked text feeds
    // both the per-tell scan and the document density score, so a declared phrase
    // contributes to neither.
    let masked = mask_allowed(input, allow_lc);
    // The markdown path must not mask before the parse. Blanking a phrase declared at
    // column one leaves four or more leading spaces, which the extractor reads as an
    // indented code block, and the whole line is dropped along with every unrelated
    // tell on it — silently, with nothing in the verdict saying a line went missing.
    // Block structure is a property of what the author wrote, so it is decided from
    // the unmasked input and the mask is applied only inside the prose the structural
    // pass kept (host-lint#26). `masked` is still what the offset map below reads,
    // and `mask_allowed` is byte-length preserving, so the two stay in step.
    let tells = if markdown {
        host_grammar::scan_prose_markdown_masked(input, &|s| mask_allowed(s, allow_lc))
    } else {
        host_grammar::scan_prose_parallel(&masked)
    };
    // Locate each tell. The engine emits one tell per occurrence, but in the markdown
    // path tells of the same (id, excerpt) arrive grouped, and a first-occurrence line
    // lookup collapses them all onto one line (ten em-dashes → ten records at line 12).
    // Occurrence-map instead: assign the k-th tell of an (id, excerpt) to the k-th
    // literal occurrence of its excerpt, yielding a precise line:col. A multi-word
    // excerpt the markdown extractor normalised away falls back to a probe line (no
    // column); one that never appears at all is a non-locatable whole-document
    // diagnosis — advisory, emitted once.
    let input_lc = masked.to_ascii_lowercase();
    let mut cursor: std::collections::HashMap<(&str, &str), usize> =
        std::collections::HashMap::new();
    for t in &tells {
        // Match case-insensitively: the engine returns lowercased lexeme phrases, but a
        // sentence-initial tell ("Let's unpack") is capitalised in the source. Ascii
        // lowercasing is byte-length-preserving, so an offset in `input_lc` is valid in
        // `input`, and the original-case substring is what the author actually wrote.
        let needle = t.excerpt.to_ascii_lowercase();
        let key = (t.id, t.excerpt.as_str());
        let from = *cursor.get(&key).unwrap_or(&0);
        // A key whose cursor reached the sentinel already fell back to a probe/Note
        // once; drop its repeats rather than re-map them.
        if from == usize::MAX {
            continue;
        }
        if let Some(off) = find_tell(&input_lc, &needle, from) {
            let end = off + needle.len();
            cursor.insert(key, end.max(off + 1));
            let (line, col) = line_col(input, off);
            // `mask_allowed` blanks each byte of a multibyte char with a space, so an
            // offset valid in `input_lc` can land mid-char in `input`; guard the slice
            // and fall back to the engine's excerpt rather than panic (plan/0055).
            let text = if input.is_char_boundary(off) && input.is_char_boundary(end) {
                &input[off..end]
            } else {
                t.excerpt.as_str()
            };
            push_tell(matches, source, line, col, text, t.id, Severity::Warn, t.cite);
        } else {
            // No literal occurrence at or after the cursor. Probe the region past the
            // cursor for the excerpt's first words: a hit is a real occurrence the
            // markdown extractor normalised (a soft line wrap), which the literal find
            // misses — emit it rather than drop it (plan/0055). Nothing past the
            // cursor is a phantom surplus or an exhausted repeat. A first miss with no
            // probe hit is a synthetic whole-document diagnosis (advisory Note). After
            // any fallback, drop further repeats of this key.
            let region = input_lc.get(from..).unwrap_or("");
            let base = input_lc[..from].matches('\n').count();
            cursor.insert(key, usize::MAX);
            match probe_line(region, &needle) {
                Some(rel_line) => {
                    push_tell(matches, source, base + rel_line, 0, &t.excerpt, t.id, Severity::Warn, t.cite)
                }
                None if from == 0 => {
                    push_tell(matches, source, 1, 0, &t.excerpt, t.id, Severity::Note, t.cite)
                }
                None => {}
            }
        }
    }
    let score = if markdown {
        host_grammar::tell_score_markdown(&masked)
    } else {
        host_grammar::tell_score(&masked)
    };
    if score.over_threshold {
        matches.push(Match {
            file: source.to_string(),
            line: 1,
            col: 0,
            text: format!(
                "agentic-tell density {:.2} across {} sentences ({} tells)",
                score.density, score.sentences, score.tells
            ),
            term: "tell-density".to_string(),
            severity: Severity::Note,
            cite: "tropes.fyi: many devices together".to_string(),
        });
    }
}

/// `--docs` is the repo-wide prose lane — the counterpart to the naming `--all`.
/// Scope determines type: naming tells hide in any file, but prose tropes are a
/// property of authored narrative, so `--docs` walks `.md` only and never runs the
/// prose engine over `.rs`/`.toml`/`.sh` (which would flag decoration in code
/// Which documents an audit judges. **The hook scans the working tree; the gate judges
/// the record** (agentic-host call/0048). A hook run wants the untracked draft, because
/// catching a document before it is staged is its job (host-lint#17). A gate run must
/// not: reddening over an uncommitted note asserts something about a record that does
/// not contain the note, which nobody can re-derive from any clone, and it blocks a
/// release over a file the release does not ship.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Corpus {
    /// Tracked and staged, plus untracked-but-not-ignored. The hook and the CLI sweep.
    WorkingTree,
    /// Tracked and staged only. What a gate may judge.
    Record,
}

/// What a documents audit found, and what it could not read.
#[derive(Default)]
pub struct DocScan {
    pub matches: Vec<Match>,
    /// Documents the walk listed and could not open. Never silently dropped: a caller
    /// that gates must refuse rather than report clean over them.
    pub unread: Vec<String>,
}

/// comments and string literals, with a meaningless clean-to-zero bar over source).
/// It walks the corpus the caller names, filtered by `.host-lintignore` (e.g. the
/// append-only `MEMORY.md`); `--exclude-standard` keeps gitignored output, vendored
/// deps, untracked worktrees, and submodules out of the working-tree corpus. Prose
/// tells are advisory (warn, exit 3), as elsewhere; the `verify` gate's recheck treats
/// that non-zero as a regression. Returns the matches and the unreadable paths, or an
/// error string — the binary prints it and exits 2; an in-process embedder surfaces it
/// as it chooses. The shared walk, so host-lint and host-lifecycle audit docs through
/// one engine (host-lifecycle#2).
pub fn run_docs(
    root: &Path,
    scopes: &LexiconScopes,
    ignore: &[String],
    corpus: Corpus,
) -> Result<DocScan, String> {
    let mut scan = DocScan::default();
    if root.as_os_str().is_empty() {
        // A clean return here would be a fail-open docs audit over nothing. Fail closed.
        return Err("--docs needs a repository root (none resolved)".to_string());
    }
    let root_str = root.to_string_lossy();
    let tracked = git_paths(root_str.as_ref(), &["ls-files", "-z"])?;
    // The working-tree corpus adds untracked-but-not-ignored files. The two sets are
    // disjoint (an entry is either in the index or not), so no dedup is needed.
    let untracked = match corpus {
        Corpus::WorkingTree => {
            git_paths(root_str.as_ref(), &["ls-files", "--others", "--exclude-standard", "-z"])?
        }
        Corpus::Record => Vec::new(),
    };
    for rel in tracked.iter().chain(untracked.iter()) {
        if !rel.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        if path_ignored(rel, ignore) {
            continue;
        }
        let path = root.join(rel);
        if fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            continue;
        }
        if !path.is_file() {
            continue;
        }
        // A document the walk listed and could not read is recorded, never dropped: a
        // verdict over a corpus with a hole in it is a claim the run cannot make
        // (agentic-host call/0048). The caller decides what to do about it; every
        // caller that gates must refuse.
        match fs::read_to_string(&path) {
            Ok(content) => {
                // Resolved from the document's own directory, so a document inside
                // a vendored repository is read against that repository's lexicon
                // rather than this one's (host-lint#26).
                let lex = scopes.for_file(&path);
                scan_prose_text(&content, rel, &lex.phrases_lc, &mut scan.matches)
            }
            Err(_) => scan.unread.push(rel.clone()),
        }
    }
    Ok(scan)
}

/// Run a `git ls-files`-family command under `-C <root>` and split its NUL-delimited
/// output into repo-relative paths. A non-zero exit (not a git repo) becomes the `--docs`
/// diagnostic the caller surfaces.
fn git_paths(root: &str, extra: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(extra.iter().copied())
        .output()
        .map_err(|e| format!("--docs needs git on PATH: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "--docs needs a git repository (git {} failed: {})",
            extra.first().copied().unwrap_or("ls-files"),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect())
}

/// Escalate decoration tells on the commit subject to blocking `Flag`. The first
/// line of a commit message — or a gh issue/PR title piped on stdin — becomes the
/// squash-merge subject / front-door text, so it is held to the same no-decoration
/// bar as the front-door docs: an em-dash, arrow, or smart quote there blocks
/// rather than warns. Body prose and other tells keep their advisory `Warn`. A
/// A `decoration` match on the first line is the subject's. The match carries its
/// line, so escalate by location, not by substring: a body decoration keeps its
/// advisory `Warn` even when the same character also appears in the subject
/// (plan/0055 — the old substring test escalated every body occurrence of a
/// character the subject happened to use).
pub fn escalate_subject_decoration(_subject: &str, matches: &mut [Match]) {
    for m in matches.iter_mut() {
        if m.term == "decoration" && m.line == 1 && m.col > 0 {
            m.severity = Severity::Flag;
        }
    }
}

/// True if `rel` (a repo-relative, `/`-separated path) matches any ignore
/// pattern. Patterns are gitignore-lite: an exact path (`MEMORY.md`), a `*`
/// glob that matches within a single path segment (`plan/*/README.md`), or a
/// trailing-slash directory prefix (`archive/`) ignoring everything beneath it.
/// `--all` honours these so a migrated project can exclude its append-only
/// record from the audit without the engine learning any methodology policy.
pub fn path_ignored(rel: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        if let Some(dir) = p.strip_suffix('/') {
            !dir.is_empty() && (rel == dir || rel.starts_with(&format!("{dir}/")))
        } else {
            glob_path(p, rel)
        }
    })
}

fn glob_path(pat: &str, path: &str) -> bool {
    let pp: Vec<&str> = pat.split('/').collect();
    let tp: Vec<&str> = path.split('/').collect();
    pp.len() == tp.len() && pp.iter().zip(&tp).all(|(p, t)| seg_glob(p.as_bytes(), t.as_bytes()))
}

// Wildcard match within one path segment: `*` matches any run of characters
// (two-pointer glob with backtracking).
fn seg_glob(pat: &[u8], s: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut mark): (Option<usize>, usize) = (None, 0);
    while t < s.len() {
        if p < pat.len() && pat[p] == b'*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if p < pat.len() && pat[p] == s[t] {
            p += 1;
            t += 1;
        } else if let Some(sp) = star {
            p = sp + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == b'*' {
        p += 1;
    }
    p == pat.len()
}

pub fn is_scannable(ext: &str) -> bool {
    matches!(ext, "" | "md" | "txt" | "rst" | "py" | "rs" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp" | "rb" | "sh" | "yaml" | "yml" | "toml" | "json" | "xml" | "html" | "css" | "sql" | "r" | "lua" | "swift" | "kt" | "scala" | "ex" | "exs" | "clj" | "hs" | "ml" | "vim" | "ps1" | "bat" | "cmake" | "makefile")
}

/// A candidate emergent tell surfaced by `gather`: a word recurring in the tell
/// shape (a word then a numeral) that the lane does not yet catch.
pub struct Candidate {
    pub word: String,
    pub count: usize,
    pub examples: Vec<String>,
}

/// Scan a corpus (commit subjects, markdown headers) for candidate emergent
/// tells. A candidate is a word in the word-then-numeral shape that is not
/// already a flag term, a warn noun, a known-legitimate context, or a stop
/// word, and whose numeral is neither a four-digit year nor a unit-bearing
/// quantity. Returns candidates recurring at least `min_count` times, ranked by
/// count then name. This is the inverse of the flag scan: the residue the
/// grammar misses, for the operator to triage (propose, declare, or leave).
pub fn gather_candidates(lines: &[String], min_count: usize) -> Vec<Candidate> {
    use std::collections::HashMap;
    let mut seen: HashMap<String, (usize, Vec<String>)> = HashMap::new();
    for line in lines {
        let lower = line.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if clean.len() < 3 || !clean.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            if FLAG_TERMS.contains(&clean)
                || WARN_NOUNS.contains(&clean)
                || PREV_SKIP.contains(&clean)
                || GATHER_STOP.contains(&clean)
            {
                continue;
            }
            let Some(next) = words.get(i + 1) else { continue };
            // a "#7" issue or PR reference is not an ordinal label
            if next.starts_with('#') {
                continue;
            }
            let nc = next.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if !(is_numeral(nc) || is_num_range(nc) || is_spelled_ordinal(nc)) {
                continue;
            }
            // four or more digits read as a year, a hash, or a quantity, not the
            // small ordinal a positional label uses
            if nc.len() >= 4 && nc.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            // a word then a number then a unit is a quantity, not a position
            if let Some(after) = words.get(i + 2) {
                let ac = after.trim_matches(|c: char| !c.is_alphanumeric());
                if UNITS.contains(&ac) {
                    continue;
                }
            }
            let entry = seen.entry(clean.to_string()).or_insert((0, Vec::new()));
            entry.0 += 1;
            if entry.1.len() < 3 {
                entry.1.push(line.trim().to_string());
            }
        }
    }
    let mut out: Vec<Candidate> = seen
        .into_iter()
        .filter(|(_, (count, _))| *count >= min_count)
        .map(|(word, (count, examples))| Candidate { word, count, examples })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    out
}

// Kani proof harnesses (the host-prove `kani-conformance` lane). `#[cfg(kani)]`
// keeps them out of `cargo build`/`cargo test`, so the release artifact stays
// byte-identical — they run only under `cargo kani`. Targets are chosen to be
// CBMC-tractable: char- and byte-level predicates, NOT `str::split`/`Vec`/`String`,
// which pull in `memchr` + heap modeling and blow CBMC up. Each proves a contract
// for ALL byte values at a bounded length — a stronger discharge than an example
// test. Dispositioned `kani:<harness>` in host-lint.obligations.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    // "#<digit>" is an internal code label (rule-success.DetectInternalCodeAsName).
    // is_review_code is char-based (strip_prefix + chars) — no split, no memchr.
    #[kani::proof]
    #[kani::unwind(4)]
    fn verify_review_code_accepts_hash_digit() {
        let d: u8 = kani::any();
        kani::assume(d.is_ascii_digit());
        let bytes = [b'#', d];
        let word = core::str::from_utf8(&bytes).unwrap();
        assert!(is_review_code(word));
    }

    // A two-letter word (e.g. a device noun like "NT") is NOT a code label
    // (rule-failure.DetectInternalCodeAsName.1): a letter prefix needs digits after.
    #[kani::proof]
    #[kani::unwind(4)]
    fn verify_review_code_rejects_two_letters() {
        let a: u8 = kani::any();
        let b: u8 = kani::any();
        kani::assume(a.is_ascii_alphabetic() && b.is_ascii_alphabetic());
        let bytes = [a, b];
        let word = core::str::from_utf8(&bytes).unwrap();
        assert!(!is_review_code(word));
    }

    // The '*' segment-glob wildcard matches any segment — a pure byte-index matcher
    // (no heap, no memchr): the Kani-ideal shape. Proves wildcard semantics and
    // panic-freedom for every 4-byte segment. (Extra code-correctness proof; the
    // .host-lintignore glob matcher has no allium obligation of its own.)
    #[kani::proof]
    #[kani::unwind(6)]
    fn verify_seg_glob_star_matches_any() {
        let s: [u8; 4] = kani::any();
        assert!(seg_glob(b"*", &s));
    }
}
