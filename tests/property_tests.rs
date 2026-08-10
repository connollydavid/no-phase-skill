use host_lint::{
    added_comment_text, check_bare_numeral_header, check_code_label_prefix, check_label_prefix,
    check_line, check_warn, check_warn_with_units, classify_line, gather_candidates,
    escalate_subject_decoration, is_numeral, is_spelled_ordinal, is_strict_directive, parse_jira_keys,
    parse_lexicon_line, parse_unit_directive, path_ignored, normalize_for_restate,
    restated_comment_sentences, scan_prose_text, scan_text,
    scan_text_with_allow, scan_text_with_allow_strict, split_sentences, validate_lexicon_entry,
    LexiconEntry, Severity, FLAG_TERMS, WARN_NOUNS, WARN_ORDINAL_TERMS,
};
use proptest::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command;

// host#16: a positional reference to a milestone checklist item (box/boxes/steps
// + a numeral, a range, or a glued hyphen-digit form) is the ordinal-by-position
// tell and flags.
#[test]
fn positional_checklist_references_flag() {
    for line in [
        "plan/0001: box 7 [x]",
        "boxes 4-8 blocked",
        "box 3 root cause localized",
        "plan steps 3-5 updated",
        "steps 3-5 closed",
    ] {
        assert!(check_line(line).is_some(), "should flag: {line}");
    }
}

// host-lifecycle#4: a phase-synonym work-unit noun immediately followed by a
// SPELLED ordinal ("wave one", "phase two") is the same band-name tell as the
// arabic form ("wave 1") and blocks. This is the shape plan/0049 used to evade
// the gate.
#[test]
fn spelled_ordinal_after_phase_synonym_flags() {
    for line in [
        "Wave one shipped",
        "phase two of the work",
        "sprint three planning",
        "cycle four review",
        "iteration five",
        "increment one",
        "episode two",
        "leg first",
        "wave twentieth",
    ] {
        assert!(check_line(line).is_some(), "should flag: {line}");
    }
}

// The checklist nouns (steps/box/boxes) and the domain-heavy warn nouns
// (part/chapter/round/level/section/step) do NOT take the spelled-ordinal shape,
// because "steps one to six" and "part one" are ordinary English, not band tells.
#[test]
fn spelled_ordinal_after_checklist_or_warn_noun_stays_clean() {
    for line in ["steps one to six", "box one", "boxes two closed"] {
        assert!(check_line(line).is_none(), "checklist noun should not block: {line}");
    }
    for line in [
        "part one of the book",
        "chapter one",
        "round one of boxing",
        "level one",
        "section one",
        "step one of the tutorial",
    ] {
        assert!(check_line(line).is_none(), "warn noun should not block on spelled: {line}");
        assert!(check_warn(line).is_none(), "warn noun should not warn on spelled: {line}");
    }
}

// The gather lane surfaces a novel noun used with a spelled ordinal as an
// emergent-tell candidate (plan/0035), the same as it does for the arabic form.
#[test]
fn gather_surfaces_a_novel_spelled_ordinal_band_noun() {
    let lines: Vec<String> = (0..4).map(|_| "cadence one landed".to_string()).collect();
    let cands = gather_candidates(&lines, 3);
    assert!(
        cands.iter().any(|c| c.word == "cadence"),
        "gather should surface 'cadence': {:?}",
        cands.iter().map(|c| &c.word).collect::<Vec<_>>()
    );
}

// host#16 boundaries: the literal checklist mark, the disposition verb, and a
// content-named reference carry no noun-plus-numeral, so they stay clean.
#[test]
fn checklist_mark_verb_and_content_name_stay_clean() {
    for line in [
        "- [x] deploy path landed",
        "1. [x] native MSVC build verified",
        "box an irreducible citation in a fence",
        "the deploy-path box landed",
        "what is in the box",
    ] {
        assert!(check_line(line).is_none(), "should be clean: {line}");
    }
}

// plan/0035: `gather` surfaces a recurring word-then-numeral shape the lane does
// not catch, and skips flag terms, legitimate contexts, quantities, years, and
// one-offs.
#[test]
fn gather_surfaces_recurring_novel_shape_and_skips_noise() {
    let lines: Vec<String> = [
        "plan/0001: widget 7 done",
        "widget 3 root cause localized",
        "phase 2 of auth refactor",
        "see figure 3 for details",
        "wait 5 seconds for the retry",
        "released in 2024 at last",
        "gizmo 9 one-off only",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let words: Vec<String> = gather_candidates(&lines, 2)
        .into_iter()
        .map(|c| c.word)
        .collect();
    assert!(words.iter().any(|w| w == "widget"), "widget recurs + is novel: {words:?}");
    assert!(!words.iter().any(|w| w == "phase"), "phase is already a flag term");
    assert!(!words.iter().any(|w| w == "figure"), "figure is a legitimate context");
    assert!(!words.iter().any(|w| w == "wait"), "a unit-bearing quantity is not a position");
    assert!(!words.iter().any(|w| w == "gizmo"), "gizmo is a one-off below the threshold");
}

proptest! {
    #[test]
    fn flag_term_followed_by_arabic_numeral_is_detected(
        term in "phase|stage|iteration|sprint|cycle|increment|wave|episode|instalment|leg|lap|box|boxes|steps",
        numeral in "[0-9]+"
    ) {
        let line = format!("{} {}", term, numeral);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn flag_term_followed_by_ordinal_roman_is_detected(
        term in "phase|stage|sprint|wave|cycle",
        // An uppercase Roman of plausible ordinal value (<= XXXIX) is a real phase
        // tell and blocks — it must not be smuggleable past the gate (plan/0055).
        roman in "II|III|IV|VI|VIII|IX|XI|XII|XV|XX|XXIV|XXXIX"
    ) {
        let line = format!("{} {} ships", term, roman);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn flag_term_followed_by_roman_acronym_or_lowercase_does_not_block(
        term in "phase|stage|sprint|wave|cycle",
        // A single-letter Roman (pronoun/letter collision), a lowercase token that
        // merely parses as Roman ("mix"/"iv"), and an uppercase abbreviation whose
        // Roman value exceeds an ordinal (DC=600, CM=900, MM=2000, XL=40, XC=90,
        // CD=400, MD=1500, DIV=504, LIV=54) all stay clean (plan/0055 cast review).
        token in "I|V|X|C|D|M|iv|mix|dc|div|DC|CM|MM|MD|MI|XL|XC|CD|DIV|MIX|LIV"
    ) {
        let line = format!("{} {} done", term, token);
        prop_assert!(check_line(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn flag_term_without_numeral_is_not_detected(
        term in "phase|stage|sprint|wave",
        word in "[a-z]{3,10}"
    ) {
        let line = format!("{} {}", term, word);
        // The spelled band blocks exactly as the arabic one does, so "phase six" is a
        // detection and not a miss. Guarding on `is_numeral` alone made this property
        // true only while the generator avoided forty words; it eventually did not.
        if !is_numeral(&word) && !is_spelled_ordinal(&word) {
            prop_assert!(check_line(&line).is_none(), "line: {}", line);
        }
    }

    #[test]
    fn ordinal_words_are_not_numerals(
        ordinal in "first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth"
    ) {
        prop_assert!(!is_numeral(&ordinal), "ordinal: {}", ordinal);
    }

    #[test]
    fn descriptive_prose_with_flag_term_is_not_detected(
        prefix in "the|this|that|my|your",
        term in "phase|stage|sprint|wave",
        suffix in "over|through|across|into"
    ) {
        let line = format!("{} {} {} the array", prefix, term, suffix);
        prop_assert!(check_line(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn flag_term_with_intermediate_word_does_not_block(
        term in "phase|stage|sprint|wave",
        intermediate in "of|in|to|into|over",
        numeral in "[0-9]+"
    ) {
        // plan/0055 dropped the two-word window: a numeral two tokens after the
        // noun ("phase of 2", "step into 3") is not a positional reference.
        let line = format!("{} {} {}", term, intermediate, numeral);
        prop_assert!(check_line(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn conventional_commit_with_phase_synonym_is_detected(
        prefix in "feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert",
        term in "phase|stage|sprint",
        numeral in "[0-9]+"
    ) {
        let line = format!("{}: {} {} of refactor", prefix, term, numeral);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn case_insensitive_detection(
        term in "Phase|PHASE|phase|PhAsE|sTaGe|STAGE",
        numeral in "[0-9]+"
    ) {
        let line = format!("## {}: {}", term, numeral);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn markdown_headers_are_detected(
        level in 1..7usize,
        term in "phase|stage|sprint|wave",
        numeral in "[0-9]+"
    ) {
        let hashes = "#".repeat(level);
        let line = format!("{}{} {}: setup", hashes, term, numeral);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn code_comments_are_detected(
        prefix in r"//|#|--|\*",
        term in "phase|stage|sprint",
        numeral in "[0-9]+"
    ) {
        let line = format!("{} {} {}: tokenize", prefix, term, numeral);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn bare_numeral_headers_are_detected(
        level in 1..7usize,
        // 1-2 digit ordinals only: a bare integer of 3+ digits ("## 404") reads as a
        // status code / numeric key and is skipped (plan/0055), so it is not a
        // should-flag input here.
        major in 0..100u32,
        minor in proptest::option::of(0..100u32)
    ) {
        let rest = match minor {
            Some(m) => format!("{}.{}", major, m),
            None => format!("{}", major),
        };
        let line = format!("{} {}", "#".repeat(level), rest);
        prop_assert!(check_bare_numeral_header(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn version_headings_are_not_bare_numeral_headers(
        level in 1..7usize,
        a in 0..100u32, b in 0..100u32, c in 0..100u32
    ) {
        let line = format!("{} {}.{}.{}", "#".repeat(level), a, b, c);
        prop_assert!(check_bare_numeral_header(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn named_headers_are_not_bare_numeral_headers(
        level in 1..7usize,
        word in "[A-Za-z][a-z]{2,10}"
    ) {
        let line = format!("{} {}", "#".repeat(level), word);
        prop_assert!(check_bare_numeral_header(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn bare_numeral_headers_only_flagged_in_markdown_sources(
        major in 0..100u32
    ) {
        let input = format!("# {}", major);
        let mut md_matches = Vec::new();
        scan_text(&input, "PLAN.md", &mut md_matches);
        prop_assert!(!md_matches.is_empty(), "input: {}", input);
        let mut rs_matches = Vec::new();
        scan_text(&input, "main.rs", &mut rs_matches);
        prop_assert!(rs_matches.is_empty(), "input: {}", input);
    }

    #[test]
    fn review_noun_followed_by_letter_digit_code_is_detected(
        term in "review|finding|blocker",
        letter in "[a-z]",
        digits in "[0-9]{1,3}"
    ) {
        let line = format!("fix the guard regex ({} {}{})", term, letter, digits);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn review_noun_followed_by_issue_code_is_detected(
        term in "review|finding|blocker",
        // brace-free quantifier: the strict-discharge scanner mis-parses a braced
        // repetition in a proptest param regex when it resolves an exercises= link.
        digits in "[0-9]+"
    ) {
        let line = format!("addresses {} #{}", term, digits);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn review_noun_followed_by_bare_numeral_is_clean(
        term in "review|finding|blocker",
        digits in "[0-9]{1,3}"
    ) {
        let line = format!("{} {} files", term, digits);
        prop_assert!(check_line(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn github_issue_refs_are_clean(
        verb in "closes|fixes",
        digits in "[0-9]{1,4}"
    ) {
        let line = format!("{} #{}", verb, digits);
        prop_assert!(check_line(&line).is_none(), "line: {}", line);
    }

    // --- flag tier: decimal numerals after a flag noun ---

    #[test]
    fn flag_term_followed_by_decimal_numeral_is_detected(
        term in "phase|stage|sprint|wave",
        major in 0..100u32,
        minor in 0..100u32
    ) {
        let line = format!("entry point ({} {}.{})", term, major, minor);
        prop_assert!(check_line(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn single_decimal_is_a_numeral_but_version_is_not(
        major in 0..100u32, minor in 0..100u32, patch in 0..100u32
    ) {
        let dec = format!("{}.{}", major, minor);
        let ver = format!("{}.{}.{}", major, minor, patch);
        prop_assert!(is_numeral(&dec), "dec: {}", dec);
        prop_assert!(!is_numeral(&ver), "ver: {}", ver);
    }

    // --- flag tier: leading label prefix (flag) ---

    #[test]
    fn leading_numeral_label_prefix_is_flagged(
        marker in r"|// |/// |//! |# |## |-- |\* ",
        major in 0..100u32,
        minor in proptest::option::of(0..100u32)
    ) {
        let code = match minor {
            Some(m) => format!("{}.{}", major, m),
            None => format!("{}", major),
        };
        let line = format!("{}{}: exec tools", marker, code);
        prop_assert!(check_label_prefix(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn clock_time_is_not_a_label_prefix(
        h in 0..24u32, m in 10..60u32
    ) {
        // colon followed by a digit, not whitespace -> a time, not a label
        let line = format!("{}:{} standup notes", h, m);
        prop_assert!(check_label_prefix(&line).is_none(), "line: {}", line);
    }

    // --- warn tier: warn ---

    #[test]
    fn bare_dotted_code_in_prose_warns(
        major in 0..100u32, minor in 0..100u32
    ) {
        let line = format!("as decided in {}.{}", major, minor);
        prop_assert!(check_warn(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn version_with_letter_does_not_warn(
        major in 0..100u32, minor in 0..100u32
    ) {
        let line = format!("bump to v{}.{}", major, minor);
        prop_assert!(check_warn(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn decimal_quantity_does_not_warn(
        major in 0..100u32, minor in 0..100u32,
        unit in "seconds|ms|hours|gb|mb"
    ) {
        let line = format!("elapsed {}.{} {}", major, minor, unit);
        prop_assert!(check_warn(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn allcaps_designator_before_decimal_does_not_warn(
        designator in "[A-Z]{2,5}",
        major in 0..100u32, minor in 0..100u32
    ) {
        // "NT 3.1", "SDK 2.1": an all-caps product/version designator, not a code.
        // The generator draws arbitrary all-caps tokens and the vocabulary matches
        // case-insensitively, so every list the tool knows has to be assumed out, not
        // just the filing-system warn-nouns (e.g. "WI" == "wi"). `[A-Z]{2,5}` also
        // reaches "BOX", "LEG", "LAP", "WAVE" in FLAG_TERMS and "ERA", "EPOCH",
        // "BATCH" in WARN_ORDINAL_TERMS, each of which warns by design. Excluding
        // only WARN_NOUNS left the property asserting that real vocabulary is silent,
        // so it failed whenever proptest happened to draw one — a red suite on a
        // released version, found by drawing "ERA" (host-lint#26 sweep). The tool was
        // right; the property was wrong.
        let lc = designator.to_ascii_lowercase();
        prop_assume!(!WARN_NOUNS.contains(&lc.as_str()));
        prop_assume!(!FLAG_TERMS.contains(&lc.as_str()));
        prop_assume!(!WARN_ORDINAL_TERMS.contains(&lc.as_str()));
        let line = format!("runs on {} {}.{}", designator, major, minor);
        prop_assert!(check_warn(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn titlecase_noun_before_decimal_still_warns(
        noun in "Decision|Milestone|Item|Round",
        major in 0..100u32, minor in 0..100u32
    ) {
        // A Title-case noun is not a designator — the milestone-code register warns.
        let line = format!("see {} {}.{}", noun, major, minor);
        prop_assert!(check_warn(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn filing_noun_with_numeral_warns(
        noun in "work-item|workitem|wi",
        major in 0..100u32,
        minor in proptest::option::of(0..100u32)
    ) {
        let code = match minor {
            Some(m) => format!("{}.{}", major, m),
            None => format!("{}", major),
        };
        let line = format!("implements {} {}", noun, code);
        prop_assert!(check_warn(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn leading_code_label_warns(
        bullet in r"|- |\* |// |# |-- ",
        letter in "[A-Za-z]",
        digits in 1..1000u32,
        dash in "—|–|-"
    ) {
        // "F1 — …", "- **B2** – …": a one-letter+digits code as a leading label.
        let line = format!("{}{}{} {} the fix", bullet, letter, digits, dash);
        prop_assert!(check_code_label_prefix(&line).is_some(), "line: {}", line);
    }

    #[test]
    fn multi_letter_device_noun_is_not_a_code_label(
        digits in 1..1000u32
    ) {
        // "COM1 open — …": a three-letter device noun is not a one-letter code,
        // and is followed by a word, not a delimiter.
        let line = format!("COM{} open — seed the DCB", digits);
        prop_assert!(check_code_label_prefix(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn code_without_a_delimiter_is_not_a_label(
        letter in "[A-Za-z]", digits in 1..1000u32
    ) {
        // "F1 key handler": a code followed by an ordinary word, no label dash.
        let line = format!("{}{} key handler", letter, digits);
        prop_assert!(check_code_label_prefix(&line).is_none(), "line: {}", line);
    }

    #[test]
    fn flag_wins_over_warn_on_the_same_line(
        term in "phase|stage|sprint",
        a in 0..100u32, b in 0..100u32
    ) {
        // a flag noun plus a stray dotted code -> classified as a flag
        let line = format!("{} {} touched the {}.{} surface", term, a, a, b);
        prop_assert_eq!(classify_line(&line, false).map(|(s, _)| s), Some(Severity::Flag), "line: {}", line);
    }
}

// --- Deterministic cases from issue #10 ---

#[test]
fn issue_10_flag_cases() {
    // worded noun + decimal, mid-line parenthetical
    assert!(check_line("entry point (Phase 5.0).").is_some());
    // leading bare-numeral label prefix
    assert!(check_label_prefix("5.5: exec/pty tools").is_some());
    assert!(check_label_prefix("// 5.5: the pty exec tool advertises").is_some());
    assert!(check_label_prefix("## 5.5: error handling").is_some());
}

#[test]
fn issue_10_warn_cases() {
    for line in [
        "as decided in 2.1",
        "exec tools (5.5)",
        "the peek/poke tools arrive in 5.3",
        "* mem_ops.h - work-item 5.3",
    ] {
        assert_eq!(
            classify_line(line, false).map(|(s, _)| s),
            Some(Severity::Warn),
            "expected warn for: {}",
            line
        );
    }
}

// plan/0055: the verdict-lifecycle aggregation, discharged by a test that
// exercises verdict_code over a match set rather than a single classified line.
#[test]
fn verdict_code_aggregates_severities() {
    let m = |severity| host_lint::Match {
        file: "x".into(),
        line: 1,
        col: 0,
        text: String::new(),
        term: String::new(),
        severity,
        cite: String::new(),
    };
    assert_eq!(host_lint::verdict_code(&[]), 0, "empty is clean");
    assert_eq!(host_lint::verdict_code(&[m(Severity::Note)]), 0, "a note never gates");
    assert_eq!(host_lint::verdict_code(&[m(Severity::Warn)]), 3, "warn-only is advisory");
    assert_eq!(host_lint::verdict_code(&[m(Severity::Flag)]), 1, "a flag blocks");
    assert_eq!(host_lint::verdict_code(&[m(Severity::Warn), m(Severity::Flag)]), 1, "flag beats warn");
    // The negative (the RecordFlag.1 property): a warn-only set never reaches the
    // blocking code — saw_flag stays false.
    assert_ne!(host_lint::verdict_code(&[m(Severity::Warn), m(Severity::Note)]), 1);
}

// plan/0055: an entity-creation obligation should assert the produced Match's
// shape (its severity and term), not merely that a detector returned Some.
#[test]
fn phase_synonym_match_is_a_flag() {
    let (sev, term) = classify_line("## Phase 2: setup", true).expect("a phase synonym is a tell");
    assert_eq!(sev, Severity::Flag, "a phase synonym produces a blocking flag");
    assert_eq!(term, "phase", "the matched term is the tell noun");
}

// plan/0055: VOCABULARY.md is the rule source; its canonical term lists must
// equal the code consts, so the document cannot silently drift from what ships.
#[test]
fn vocabulary_term_lists_match_the_code() {
    let doc = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/VOCABULARY.md"))
        .expect("read VOCABULARY.md");
    let canonical = |prefix: &str| -> std::collections::BTreeSet<String> {
        doc.lines()
            .find(|l| l.trim_start().starts_with(prefix))
            .unwrap_or_else(|| panic!("VOCABULARY.md is missing the '{prefix}' canonical line"))
            .trim_start()
            .strip_prefix(prefix)
            .unwrap()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    let code = |terms: &[&str]| -> std::collections::BTreeSet<String> {
        terms.iter().map(|s| s.to_string()).collect()
    };
    assert_eq!(canonical("flag:"), code(host_lint::FLAG_TERMS), "VOCABULARY.md flag list != FLAG_TERMS");
    assert_eq!(canonical("warn:"), code(host_lint::WARN_ORDINAL_TERMS), "VOCABULARY.md warn list != WARN_ORDINAL_TERMS");
}

// plan/0055: the blocking-tier precision recut. The criticals (Roman pronoun/
// letter false-flags, the LEXICON laundering covered elsewhere), the verb-term
// demotion, and the year/status guards.
#[test]
fn plan_0055_blocking_tier_precision_recut() {
    // An uppercase Roman of ordinal value (<= XXXIX) is a real phase tell and
    // blocks — it must not smuggle past the gate (plan/0055, operator review).
    for flag in ["Phase IV ships the parser", "Stage VIII review", "Sprint XII backlog"] {
        assert!(check_line(flag).is_some(), "roman phase tell should block: {flag}");
    }
    // A single-letter Roman (pronoun/letter collision), a lowercase roman-word, and
    // an uppercase abbreviation whose Roman value exceeds an ordinal stay clean:
    // the abbreviation collisions (DC, CM, MM, XL, DIV) live in a tell noun's home
    // domain and must not false-flag (plan/0055 cast review).
    for clean in [
        "this phase I shipped the fix",
        "the next wave C landed",
        "phase mix in the daw",
        "phase iv intravenous line",     // lowercase: not a label
        "phase DC offset rejection",     // EE abbreviation, phase's home domain
        "boxes MM apart on the board",   // millimetres
        "wave XL of the rollout",        // size
        "box DIV layout",                // HTML
        "stage MD review",
    ] {
        assert_eq!(check_line(clean), None, "should be clean: {clean}");
    }

    // Demotion (data-grounded, plan/0055): the verb/measurement terms and the
    // domain-heavy terms warn, never block. round/level/step/part/pass plus the six
    // measured as domain-heavy in real code (section, chapter, epoch, batch, era,
    // period) are advisory; their two-word-window false flags stay clean.
    for warn in [
        "pass 2 arguments to the helper",
        "round 2 decimal places",
        "level 3 cache eviction",
        "part 2 of the file",
        "see section 3 of the spec",
        "chapter 2 of the book",
        "train for epoch 0 then stop",
        "batch 2 of the jobs",
        "era 3 of the migration",
        "period 4 review window",
    ] {
        assert_eq!(check_line(warn), None, "domain term must not block: {warn}");
        assert_eq!(
            classify_line(warn, false).map(|(s, _)| s),
            Some(Severity::Warn),
            "domain term should warn: {warn}"
        );
    }
    // The high-centrality work-unit words still block.
    for flag in ["wave 2 of rollout", "sprint 3 backlog", "cycle 4 review", "increment 2 shipped"] {
        assert!(check_line(flag).is_some(), "work-unit term should flag: {flag}");
    }
    for clean in [
        "step into 3 dimensions of design",
        "port the pass to C",
        "in this pass I fixed it",
    ] {
        assert_eq!(classify_line(clean, false), None, "verb collision clean: {clean}");
    }

    // a year, status-code, or numeric-key markdown heading is not a bare-ordinal tell.
    for clean in ["## 2024", "## 2024.01", "## 404", "## 200", "## 500"] {
        assert_eq!(check_bare_numeral_header(clean), None, "code/year heading clean: {clean}");
    }
    assert!(check_bare_numeral_header("## 3").is_some(), "## 3 should flag");
    assert!(check_bare_numeral_header("## 12").is_some(), "## 12 should flag");
    assert!(check_bare_numeral_header("## 3.5").is_some(), "## 3.5 should flag");

    // only an ascending short range is a checklist range; a date, a time span, a
    // year range, and a degenerate run are not.
    for clean in ["release wave 2024-01 shipped", "wave 12-07 release", "wave 9-5 hours", "phase 1-1 noop"] {
        assert_eq!(check_line(clean), None, "non-ascending/year range clean: {clean}");
    }
    assert!(check_line("wave 4-8 closed").is_some(), "ascending short range still flags");

    // a status-code or numeric-key label is not a milestone label.
    for clean in ["// 200: OK response handler", "// 404: not found"] {
        assert_eq!(check_label_prefix(clean), None, "status-code label clean: {clean}");
    }
    assert!(check_label_prefix("5.5: exec tools").is_some(), "5.5: still flags");
    assert!(check_label_prefix("3: do the thing").is_some(), "3: still flags");

    // a plain arabic tell still blocks
    assert!(check_line("Phase 2 ships the new parser").is_some(), "Phase 2 should flag");
}

#[test]
fn leading_code_label_warn_cases() {
    // PR #22 structured its fixes as bare "F1 — …" leading labels: the
    // section-5 code-as-name tell with no preceding review/finding/blocker noun.
    for line in [
        "F1 — PE version stamp 3.10",
        "- **F2** — SetHandleInformation routed through a feat.c probe",
        "* B3 – lstrcpynA swapped at 50 sites",
        "F4: the durable name follows the colon",
    ] {
        assert_eq!(
            classify_line(line, false).map(|(s, _)| s),
            Some(Severity::Warn),
            "expected warn for: {}",
            line
        );
    }
}

#[test]
fn leading_code_label_clean_cases() {
    // A multi-letter device noun, a code with no label delimiter, and a real
    // GitHub ref must not be read as a leading code label.
    for line in [
        "COM1 open — GetCommState-first DCB seeding",
        "the F1 key opens help",
        "fixes #18",
        "review 3 files",
    ] {
        assert!(
            check_code_label_prefix(line).is_none(),
            "expected no code-label match for: {}",
            line
        );
    }
}

#[test]
fn allcaps_designator_clean_cases() {
    // An all-caps product/version designator before a decimal is a version
    // string, not a milestone code — it must not even warn.
    for line in [
        "Make the device load and wire-respond on the Windows NT 3.1 floor",
        "targets NT 3.1",
        "ships the SDK 2.1 headers",
        "DOS 6.2 compatibility",
    ] {
        assert_eq!(classify_line(line, false), None, "expected clean for: {}", line);
    }
    // …but a Title-case milestone noun before a decimal still warns.
    assert_eq!(
        classify_line("see Decision 2.1", false).map(|(s, _)| s),
        Some(Severity::Warn),
        "Title-case noun should still warn",
    );
}

#[test]
fn issue_10_clean_cases() {
    // version strings and quantities must not even warn
    for line in [
        "bump to v2.1",
        "requires Python 3.11",
        "5.5 seconds elapsed",
        "increased by 2.1%",
        "review 3 files",
    ] {
        assert_eq!(classify_line(line, false), None, "expected clean for: {}", line);
    }
}

// --- Sanctioned-token allowlist (the LEXICON, masked before detection) ---

// A helper: scan one line under an allow-list, return the matches.
fn scan_one(line: &str, source: &str, allow: &[&str]) -> Vec<host_lint::Match> {
    let allow_lc: Vec<String> = allow.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut m = Vec::new();
    scan_text_with_allow(line, source, &allow_lc, &mut m);
    m
}

#[test]
fn allowed_phrase_suppresses_its_own_flag() {
    // "wave 1" is a flag (wave is a position noun); masking the phrase clears it.
    // (Such a bare-flag phrase is not a registerable LEXICON entry — the no-laundering guard refuses a
    // position noun — but mask_allowed is exercised here as the masking mechanism.)
    assert!(!scan_one("see wave 1 of the rollout", "doc.md", &[]).is_empty());
    assert!(scan_one("see wave 1 of the rollout", "doc.md", &["wave 1"]).is_empty());
}

#[test]
fn allowed_phrase_suppresses_a_dotted_code_warn() {
    // A milestone-style dotted code ("Decision 2.1") trips the advisory warn; an
    // allow entry clears it. (An all-caps designator like "NT 3.1" no longer
    // warns at all — it is recognised as a version string by the engine.)
    assert!(!scan_one("see Decision 2.1 here", "README.md", &[]).is_empty());
    assert!(scan_one("see Decision 2.1 here", "README.md", &["Decision 2.1"]).is_empty());
}

#[test]
fn table_cell() {
    // host-lint#21: a bare dotted code inside a markdown table cell is a data
    // value; the column header carries the unit on another line. A numeral
    // fused to a pipe or flanked by standalone ones is structurally a cell.
    for line in [
        "| A100 | 11.34 | 10.00 | 67.58 |",
        "| latency_ms | 25.27 |",
        "11.34 | 10.00",
        "|11.34|",
    ] {
        assert!(check_warn(line).is_none(), "table cell should not warn: {line}");
    }
    // A bare dotted code in prose (no pipe) still warns: the skip is structural,
    // not a blanket silence.
    for line in ["as decided in 2.1", "exec tools (5.5)"] {
        assert!(check_warn(line).is_some(), "prose dotted code should still warn: {line}");
    }
    // Precision: a real tell inside a table cell still flags, because the
    // noun-gated flag tier (check_line) is location-independent.
    for line in ["| Phase 2.1: auth | done |", "Sprint 3 | backlog"] {
        assert!(check_line(line).is_some(), "noun-gated tell must still flag in a cell: {line}");
    }
}

#[test]
fn quantity_operators() {
    // host-lint#21: a dotted code marked by an approximation or comparison
    // operator, or a trailing multiplier, is a quantity, not a name.
    for line in [
        "NMSE ≈ 1.0",
        "speedup ~2.2× over baseline",
        "accuracy > 67.5",
        "loss < 0.05",
        "cost = 5.5 ms",
        "≈1.0 here",
        "ratio 2.2 ×",
    ] {
        assert!(check_warn(line).is_none(), "operator-marked quantity should not warn: {line}");
    }
    for line in ["as decided in 2.1", "exec tools (5.5)"] {
        assert!(check_warn(line).is_some(), "plain dotted code should still warn: {line}");
    }
}

#[test]
fn compound_units() {
    // host-lint#21: a dotted code followed by a slashed unit is a quantity.
    for line in [
        "throughput 11.34 t/s",
        "cost 3.2 ms/token",
        "latency 5.5 tok/s,",
    ] {
        assert!(check_warn(line).is_none(), "slashed-unit quantity should not warn: {line}");
    }
    let line = "as decided in 2.1";
    assert!(check_warn(line).is_some(), "plain dotted code should still warn: {line}");
}

#[test]
fn declared_units() {
    // host-lint#21: a domain unit declared in the LEXICON marks a following
    // numeral as a quantity, not a name.
    let gflops = "gflops".to_string();
    assert!(
        check_warn_with_units("312.4 GFLOPS sustained", std::slice::from_ref(&gflops)).is_none(),
        "a numeral before a declared unit should not warn"
    );
    // With no units declared, the same line warns: GFLOPS is not a built-in unit.
    assert!(
        check_warn_with_units("312.4 GFLOPS sustained", &[]).is_some(),
        "an undeclared domain unit should still warn"
    );
}

#[test]
fn parse_unit_directive_reads_a_declared_unit() {
    assert_eq!(parse_unit_directive("# host-lint: unit GFLOPS"), Some("gflops".to_string()));
    assert_eq!(parse_unit_directive("# host-lint: unit dB"), Some("db".to_string()));
    assert_eq!(parse_unit_directive("GFLOPS"), None, "not a directive");
    assert_eq!(parse_unit_directive("# host-lint: unit"), None, "no token");
    assert_eq!(parse_unit_directive("# host-lint: unit a b"), None, "two tokens rejected");
}

#[test]
fn allow_is_case_insensitive() {
    assert!(scan_one("Built for DOS 6.22 hosts", "doc.md", &["dos 6.22"]).is_empty());
}

#[test]
fn allow_does_not_clear_a_different_tell_on_the_same_line() {
    // Allow-listing "wave 1" must not mask the separate "phase 4" tell.
    let m = scan_one("wave 1 covers phase 4 work", "doc.md", &["wave 1"]);
    assert_eq!(m.len(), 1, "phase 4 must still flag");
    assert_eq!(m[0].term, "phase");
}

#[test]
fn allow_respects_word_boundaries() {
    // "phase 1" allow-listed must NOT clear the longer tell "phase 12".
    assert!(!scan_one("phase 12 begins", "doc.md", &["phase 1"]).is_empty());
}

#[test]
fn empty_allow_list_is_unchanged_behaviour() {
    let with_empty = scan_one("## Phase 2: setup", "PLAN.md", &[]);
    let mut baseline = Vec::new();
    scan_text("## Phase 2: setup", "PLAN.md", &mut baseline);
    assert_eq!(with_empty.len(), baseline.len());
    assert_eq!(with_empty.len(), 1);
}

// --- Prose lane consults the LEXICON (issue #16) ---

// A helper: scan text as prose under an allow-list, return the matches.
fn prose_one(input: &str, source: &str, allow: &[&str]) -> Vec<host_lint::Match> {
    let allow_lc: Vec<String> = allow.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut m = Vec::new();
    scan_prose_text(input, source, &allow_lc, &mut m);
    m
}

#[test]
fn prose_lexicon_masks_a_trope_within_a_declared_phrase() {
    // `tapestry` is a grandiose-noun prose trope, and also the name of a real Java
    // web framework; a project that runs on Apache Tapestry declares the phrase, and
    // the prose lane masks the trope within it, the same pre-detection blank-out the
    // naming lane already performs. The declared phrase sits off column one on
    // purpose: masking it at the line start would indent the line into a markdown
    // code block and clear every tell on it (connollydavid/host-lint#26).
    let line = "The Apache Tapestry app logs to disk; a second tapestry runs nightly.";
    let undeclared = prose_one(line, "doc.md", &[]);
    assert_eq!(undeclared.len(), 2, "both tapestry occurrences flag with no LEXICON");
    let masked = prose_one(line, "doc.md", &["apache tapestry"]);
    assert_eq!(masked.len(), 1, "the occurrence inside the declared phrase is cleared");
    // The surviving flag is the standalone `tapestry`, not the one inside the phrase:
    // it sits at the second occurrence's column (surgical at the word boundary).
    assert_eq!(masked[0].col, undeclared[1].col);
}

// The regression the sibling test was written to sidestep. A declaration is scoped
// to a phrase, so its blast radius must be that phrase; before the fix, a phrase at
// column one blanked to four leading spaces, the markdown extractor read that as an
// indented code block, and the whole line was dropped — the standalone occurrence
// with it, silently (connollydavid/host-lint#26). The fixture that caught this in
// the field only differed by opening with "The ".
#[test]
fn prose_lexicon_at_column_one_masks_the_phrase_not_the_line() {
    let line = "Apache Tapestry logs to disk; a second tapestry runs nightly.";
    let undeclared = prose_one(line, "doc.md", &[]);
    assert_eq!(undeclared.len(), 2, "both occurrences flag with no LEXICON");

    let masked = prose_one(line, "doc.md", &["apache tapestry"]);
    assert_eq!(
        masked.len(),
        1,
        "declaring the column-one phrase must not clear the standalone occurrence"
    );
    assert_eq!(
        masked[0].col, undeclared[1].col,
        "the survivor is the standalone `tapestry`, at its own column"
    );
}

// Plain (non-markdown) prose never had the indent problem, because there is no block
// structure to corrupt. Pinned so the markdown fix cannot regress the plain path.
#[test]
fn prose_lexicon_at_column_one_is_unchanged_for_plain_text() {
    let line = "Apache Tapestry logs to disk; a second tapestry runs nightly.";
    assert_eq!(prose_one(line, "stdin", &[]).len(), 2);
    assert_eq!(prose_one(line, "stdin", &["apache tapestry"]).len(), 1);
}

#[test]
fn prose_empty_or_irrelevant_allow_leaves_all_flags() {
    let line = "The Apache Tapestry app logs to disk; a second tapestry runs nightly.";
    assert_eq!(prose_one(line, "doc.md", &[]).len(), 2, "no LEXICON masks nothing");
    // An entry that does not occur in the text masks nothing (unchanged behaviour).
    assert_eq!(prose_one(line, "doc.md", &["windows 3.1"]).len(), 2);
}

// --- Path ignore (.host-lintignore) ---

#[test]
fn path_ignore_exact_glob_and_dir() {
    let pats = vec![
        "MEMORY.md".to_string(),
        "plan/*/README.md".to_string(),
        "archive/".to_string(),
    ];
    // exact root-level file
    assert!(path_ignored("MEMORY.md", &pats));
    // single-segment glob
    assert!(path_ignored("plan/0004-command-execution/README.md", &pats));
    // directory prefix ignores everything beneath
    assert!(path_ignored("archive/old/notes.md", &pats));
    assert!(path_ignored("archive", &pats));
    // not matched: live index, wrong depth, non-root MEMORY
    assert!(!path_ignored("plan/PLAN.md", &pats));
    assert!(!path_ignored("plan/0004/extra/README.md", &pats));
    assert!(!path_ignored("docs/MEMORY.md", &pats));
}

#[test]
fn empty_ignore_matches_nothing() {
    assert!(!path_ignored("MEMORY.md", &[]));
}

#[test]
fn coauthored_by_trailers_are_exempt() {
    // A discretionary attribution trailer is skipped entirely, neither flagged
    // nor warned — even a phase-like co-author name. Dealer's choice field.
    for line in [
        "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>",
        "co-authored-by: someone 2.1",
        "  Co-Authored-By: Phase 2 Bot <bot@example.com>",
    ] {
        assert_eq!(classify_line(line, false), None, "expected exempt for: {}", line);
        assert_eq!(classify_line(line, true), None, "expected exempt (md) for: {}", line);
    }
}

#[test]
fn prose_tells_are_advisory_warns() {
    // Prose tells are advisory — never Flag — so a title or comment with a tell warns
    // (locatable, exit 3) or notes (a non-locatable whole-document diagnosis, exit 0)
    // but never blocks a commit.
    let mut m = Vec::new();
    scan_prose_text("Let's unpack this. We delve into the rich tapestry.", "stdin", &[], &mut m);
    assert!(!m.is_empty(), "expected prose tells");
    assert!(m.iter().all(|x| x.severity != Severity::Flag), "prose never blocks");
    assert!(m.iter().any(|x| x.term == "pedagogical-hook"));
    assert!(m.iter().all(|x| !x.cite.is_empty()), "prose tells carry a citation");
}

#[test]
fn decoration_occurrences_map_to_distinct_lines_and_columns() {
    // The headline plan/0031 fix: em-dashes on different lines report at their own
    // line:col, not all collapsed onto the first occurrence's line (the call/0019 defect
    // where ten dashes all reported at line 12).
    let input = "Alpha — one.\nBeta line here all.\nGamma — two — three.\n";
    let mut m = Vec::new();
    scan_prose_text(input, "doc.md", &[], &mut m);
    let dec: Vec<_> = m.iter().filter(|x| x.term == "decoration").collect();
    assert_eq!(dec.len(), 3, "three em-dashes → three records");
    let lines: Vec<usize> = dec.iter().map(|x| x.line).collect();
    assert!(lines.contains(&1) && lines.contains(&3), "on their real lines, got {lines:?}");
    assert!(dec.iter().all(|x| x.col > 0), "each decoration carries a column");
    let mut seen = std::collections::HashSet::new();
    assert!(
        dec.iter().all(|x| seen.insert((x.line, x.col))),
        "distinct line:col per occurrence"
    );
}

#[test]
fn structural_diagnoses_are_advisory_notes() {
    // A whole-document diagnosis (density, anaphora, a self-answered question) has no
    // single editable span, so it is advisory (Note, exit 0) — outside the clean-to-zero
    // gate — while locatable tropes stay Warn.
    let input = "Let's unpack this. It's not a tweak, it's a revolution. We delve. \
                 We leverage. We harness. The result? Pure synergy. Fast, clean, and robust.";
    let mut m = Vec::new();
    scan_prose_text(input, "stdin", &[], &mut m);
    assert!(
        m.iter().any(|x| x.severity == Severity::Note),
        "a structural diagnosis is advisory"
    );
    assert!(
        m.iter().filter(|x| x.severity == Severity::Note).all(|x| x.col == 0),
        "advisory diagnoses have no column"
    );
    assert!(
        m.iter().any(|x| x.severity == Severity::Warn),
        "locatable tropes still warn"
    );
}

#[test]
fn subject_decoration_escalates_to_flag() {
    // A decoration tell on the commit subject (first line) / a gh title blocks.
    let input = "classify: refuse adoption — print the case";
    let mut m = Vec::new();
    scan_prose_text(input, "stdin", &[], &mut m);
    escalate_subject_decoration(input.lines().next().unwrap(), &mut m);
    assert!(m.iter().any(|x| x.term == "decoration" && x.severity == Severity::Flag));
}

#[test]
fn body_decoration_stays_advisory() {
    // Decoration confined to the body keeps its Warn — only the subject blocks.
    let input = "classify: refuse adoption to print the case\n\nIt prints the case — unless the target is software.";
    let mut m = Vec::new();
    scan_prose_text(input, "stdin", &[], &mut m);
    escalate_subject_decoration(input.lines().next().unwrap(), &mut m);
    assert!(m.iter().any(|x| x.term == "decoration"), "expected a body decoration tell");
    assert!(m.iter().filter(|x| x.term == "decoration").all(|x| x.severity == Severity::Warn));
}

#[test]
fn body_decoration_with_same_char_in_subject_stays_advisory() {
    // plan/0055: the subject and the body both carry an em-dash. The subject
    // one blocks; the body one must keep its Warn (the old substring test escalated
    // every body occurrence of a character the subject happened to use).
    let input = "classify: refuse adoption — print the case\n\nThe body also leans on a dash — right here.";
    let mut m = Vec::new();
    scan_prose_text(input, "stdin", &[], &mut m);
    escalate_subject_decoration(input.lines().next().unwrap(), &mut m);
    let decos: Vec<_> = m.iter().filter(|x| x.term == "decoration").collect();
    assert!(decos.iter().any(|x| x.line == 1 && x.severity == Severity::Flag), "subject decoration should flag");
    assert!(decos.iter().any(|x| x.line != 1), "expected a body decoration tell");
    assert!(
        decos.iter().filter(|x| x.line != 1).all(|x| x.severity == Severity::Warn),
        "body decoration should stay advisory"
    );
}

#[test]
fn unclosed_ignore_fence_fails_loud() {
    // plan/0055: a host-lint:ignore fence with no closing fence used to skip
    // every later line silently. It must surface an unclosed-fence flag instead.
    let text = "intro line\n```host-lint:ignore\ncited Phase 1 reference\nPhase 2 ships here\n";
    let mut m = Vec::new();
    scan_text_with_allow_strict(text, "doc.md", &[], &[], false, &mut m);
    assert!(
        m.iter().any(|x| x.term == "unclosed-ignore-fence" && x.severity == Severity::Flag),
        "unclosed ignore fence should fail loud: {:?}",
        m.iter().map(|x| &x.term).collect::<Vec<_>>()
    );
}

#[test]
fn longer_ignore_fence_wraps_an_inner_code_fence() {
    // plan/0055: an inner bare fence shorter than the opening ignore fence does
    // not close the quarantine, so a tell inside a fenced sample within the citation
    // does not leak back to the linter. The longer outer fence closes it.
    let text = "````host-lint:ignore\nExample:\n```\nPhase 2 was the cleanup\n```\n````\nclean tail\n";
    let mut m = Vec::new();
    scan_text_with_allow_strict(text, "doc.md", &[], &[], false, &mut m);
    assert!(
        m.is_empty(),
        "quarantined content leaked: {:?}",
        m.iter().map(|x| (x.term.clone(), x.line)).collect::<Vec<_>>()
    );
}

#[test]
fn clean_prose_emits_no_tells() {
    // Ordinary technical prose stays silent — no Warn, no density gate.
    let mut m = Vec::new();
    scan_prose_text(
        "The parser reads each line and reports the first tell it finds. \
         A missing allow-list file means no phrases are masked.",
        "stdin",
        &[],
        &mut m,
    );
    assert!(m.is_empty(), "clean prose tripped: {:?}", m.iter().map(|x| &x.term).collect::<Vec<_>>());
}

#[test]
fn dense_prose_crosses_the_density_gate() {
    let mut m = Vec::new();
    scan_prose_text(
        "Let's unpack this. It's not a tweak, it's a revolution. We delve. \
         We leverage. We harness. The result? Pure synergy. Fast, clean, and robust.",
        "stdin",
        &[],
        &mut m,
    );
    assert!(m.iter().any(|x| x.term == "tell-density"), "expected the density summary");
}

// --- LEXICON: parsing, the strict directive, and the three guards (issue #13) ---

fn entry(phrase: &str, url: Option<&str>) -> LexiconEntry {
    LexiconEntry { phrase: phrase.to_string(), url: url.map(String::from) }
}

#[test]
fn lexicon_parse_skips_blanks_and_comments() {
    assert_eq!(parse_lexicon_line(""), None);
    assert_eq!(parse_lexicon_line("   "), None);
    assert_eq!(parse_lexicon_line("# a note"), None);
    // A markdown-style "## heading" is a comment (hash + non-digit), not an entry.
    assert_eq!(parse_lexicon_line("## heading"), None);
    // The strict directive is comment-shaped, so the phrase parser ignores it.
    assert_eq!(parse_lexicon_line("# host-lint: strict"), None);
}

#[test]
fn lexicon_parse_keeps_hash_number_entries() {
    // "#7" is an entry (hash + digit), NOT a comment — this is the carve-out that
    // stops the comment marker colliding with the hash-number reference shape.
    assert_eq!(parse_lexicon_line("#7"), Some(entry("#7", None)));
}

#[test]
fn lexicon_parse_splits_a_trailing_url() {
    let e = parse_lexicon_line("#7 https://github.com/o/r/issues/7").unwrap();
    assert_eq!(e.phrase, "#7");
    assert_eq!(e.url.as_deref(), Some("https://github.com/o/r/issues/7"));
    // A phrase with no URL keeps every token (including internal spaces).
    assert_eq!(parse_lexicon_line("Windows 3.1"), Some(entry("Windows 3.1", None)));
    // A trailing non-URL token is part of the phrase, not a URL.
    assert_eq!(parse_lexicon_line("Windows 3.1 beta"), Some(entry("Windows 3.1 beta", None)));
}

#[test]
fn lexicon_strict_directive_recognised() {
    assert!(is_strict_directive("# host-lint: strict"));
    assert!(is_strict_directive("#host-lint: strict"));
    assert!(!is_strict_directive("# some other comment"));
    assert!(!is_strict_directive("Windows 3.1"));
}

#[test]
fn lexicon_guard_accepts_legitimate_vocabulary() {
    // A warn-tier phrase (a dotted code with a legitimizing word) is the intended
    // case: it carries a letter and is not a flag-tier tell.
    assert!(validate_lexicon_entry(&entry("Windows 3.1", None), &[]).is_ok());
    assert!(validate_lexicon_entry(&entry("Decision 2.1", None), &[]).is_ok());
    assert!(validate_lexicon_entry(&entry("COM1", None), &[]).is_ok());
    // PROJ-NNNN-shaped standards tokens are vocabulary unless a jira-key is declared.
    assert!(validate_lexicon_entry(&entry("RFC-2119", None), &[]).is_ok());
}

#[test]
fn lexicon_master_key_guard_rejects_a_bare_numeral() {
    // A bare numeral/dotted code with no legitimizing word would clear every
    // occurrence tree-wide — refused.
    assert!(validate_lexicon_entry(&entry("5.5", None), &[]).is_err());
    assert!(validate_lexicon_entry(&entry("2.1", None), &[]).is_err());
}

#[test]
fn lexicon_no_laundering_guard_rejects_a_complete_flag() {
    // The phrase is itself a flag-tier tell — you rename it, you do not allow-list
    // it. (The 4B test tried exactly this: `lexicon add "Phase 5.5"`.)
    assert!(validate_lexicon_entry(&entry("Phase 5.5", None), &[]).is_err());
    assert!(validate_lexicon_entry(&entry("Step 3", None), &[]).is_err());
}

// plan/0055: the worst laundering case the guard exists to stop — a bare
// position noun, or a phrase carrying one, masks that noun out of every real
// "<noun> N" tell repo-wide (and defeats strict). It must be refused even though
// the bare noun is not itself a complete flag.
#[test]
fn lexicon_no_laundering_guard_rejects_a_position_noun() {
    for laundering in [
        "phase",        // bare flag noun: masks every "phase 2"
        "step",         // bare warn-ordinal noun: masks every "step 2" (and its strict flag)
        "review",       // bare review noun: masks every "review #7"
        "the phase",    // carries the noun: masks "the phase 2"
        "wi",           // bare filing-code noun
        "F1",           // bare review code: masks every "review F1" (plan/0055 cast review)
        "B2",           // bare review code
        "R1",           // bare review code (also a hardware designator; rephrase or fence it)
    ] {
        assert!(
            validate_lexicon_entry(&entry(laundering, None), &[]).is_err(),
            "must refuse a phrase carrying a bare position noun or review code: {laundering}"
        );
    }
    // A warn-tier phrase with no standalone position noun or code is still legitimate.
    assert!(validate_lexicon_entry(&entry("Decision 2.1", None), &[]).is_ok());
    assert!(validate_lexicon_entry(&entry("cross-section view", None), &[]).is_ok());
    assert!(validate_lexicon_entry(&entry("COM1", None), &[]).is_ok()); // device noun, not a 1-letter code
}

#[test]
fn lexicon_guard_citation_gates_tracker_refs() {
    // A bare tracker ref must carry a URL (provenance), else it is a phantom.
    assert!(validate_lexicon_entry(&entry("#7", None), &[]).is_err());
    assert!(validate_lexicon_entry(&entry("#7", Some("https://github.com/o/r/issues/7")), &[]).is_ok());
    assert!(validate_lexicon_entry(&entry("connollydavid/host#7", None), &[]).is_err());
    assert!(validate_lexicon_entry(
        &entry("connollydavid/host#7", Some("https://github.com/connollydavid/host/issues/7")),
        &[]
    )
    .is_ok());
    // plan/0055: a phantom '#999' cited to an unrelated URL that does not
    // reference 999 is refused — the URL must be provenance, not any link.
    assert!(validate_lexicon_entry(&entry("#999", Some("https://example.com/unrelated")), &[]).is_err());
    assert!(validate_lexicon_entry(&entry("#999", Some("https://github.com/o/r/issues/999")), &[]).is_ok());
}

#[test]
fn lexicon_jira_key_gating_is_opt_in() {
    let proj = vec!["PROJ".to_string()];
    // Without a declared key, PROJ-NNNN is plain vocabulary (no URL required) —
    // this is what keeps standards tokens (RFC-2119, UTF-8) from being forced to cite.
    assert!(validate_lexicon_entry(&entry("PROJ-1234", None), &[]).is_ok());
    // With PROJ declared, PROJ-1234 is a citation-gated tracker ref: URL required.
    assert!(validate_lexicon_entry(&entry("PROJ-1234", None), &proj).is_err());
    assert!(validate_lexicon_entry(&entry("PROJ-1234", Some("https://jira.example/PROJ-1234")), &proj).is_ok());
    // A different key is unaffected: declaring PROJ does NOT gate RFC-2119.
    assert!(validate_lexicon_entry(&entry("RFC-2119", None), &proj).is_ok());
}

#[test]
fn lexicon_jira_directive_parses_declared_keys() {
    assert_eq!(parse_jira_keys("# host-lint: jira-key PROJ"), Some(vec!["PROJ".to_string()]));
    assert_eq!(
        parse_jira_keys("# host-lint: jira-key PROJ TEAM2"),
        Some(vec!["PROJ".to_string(), "TEAM2".to_string()])
    );
    // Lowercase / non-key tokens are dropped; an empty declaration is None.
    assert_eq!(parse_jira_keys("# host-lint: jira-key"), None);
    assert_eq!(parse_jira_keys("# host-lint: strict"), None);
    assert_eq!(parse_jira_keys("PROJ-1"), None);
}

#[test]
fn lexicon_strict_escalates_an_undeclared_warn_to_a_flag() {
    // A bare dotted code warns by default, blocks under strict, and is silenced by
    // a LEXICON entry that masks the full phrase.
    let scan = |strict: bool, allow: &[&str]| {
        let allow_lc: Vec<String> = allow.iter().map(|s| s.to_ascii_lowercase()).collect();
        let mut m = Vec::new();
        scan_text_with_allow_strict("see Decision 2.1 here", "README.md", &allow_lc, &[], strict, &mut m);
        m
    };
    assert_eq!(scan(false, &[]).first().map(|m| m.severity), Some(Severity::Warn));
    assert_eq!(scan(true, &[]).first().map(|m| m.severity), Some(Severity::Flag));
    assert!(scan(true, &["Decision 2.1"]).is_empty(), "an allowed phrase is not escalated");
}

#[test]
fn lexicon_masking_clears_a_cited_tracker_ref() {
    // "finding #7" is a flag (review noun + code); masking the cited "#7" phrase
    // leaves "finding " with no code, so the line is clean.
    let allow = vec!["#7".to_string()];
    let mut m = Vec::new();
    scan_text_with_allow_strict("see finding #7 in the log", "doc.md", &allow, &[], true, &mut m);
    assert!(m.is_empty(), "cited #7 should mask the review-code flag: {:?}", m.iter().map(|x| &x.term).collect::<Vec<_>>());
}

// --- host-lint:ignore fenced blocks (call/0019, plan/0027) ---

#[test]
fn host_lint_ignore_block_skips_naming_tells_in_markdown() {
    let scan = |text: &str, src: &str| {
        let mut m = Vec::new();
        scan_text_with_allow_strict(text, src, &[], &[], false, &mut m);
        m
    };
    // Inside a host-lint:ignore block: skipped (the literal-citation quarantine).
    assert!(scan("```host-lint:ignore\nPhase 1 was the bootstrap\n```", "doc.md").is_empty());
    // The same tell in prose: still flags.
    assert!(!scan("Phase 1 was the bootstrap", "doc.md").is_empty());
    // In a REGULAR fenced block: still flags — only host-lint:ignore is skipped.
    assert!(!scan("```\nPhase 1 was the bootstrap\n```", "doc.md").is_empty());
    // A different language tag is not the ignore tag: still flags.
    assert!(!scan("```text\nPhase 1 was the bootstrap\n```", "doc.md").is_empty());
    // Inline backtick: still flags — only blocks are skipped, never inline.
    assert!(!scan("see `Phase 1` here", "doc.md").is_empty());
    // Non-markdown (a commit message): the fence is literal text, the tell flags.
    assert!(!scan("```host-lint:ignore\nPhase 1\n```", "stdin").is_empty());
    // Detection resumes after the block closes.
    assert!(!scan("```host-lint:ignore\nold Phase 1\n```\nnow Phase 2 here", "doc.md").is_empty());
    // An indented code block is not a fence — its `Phase 1` line is scanned and flags.
    assert!(!scan("    ```host-lint:ignore\n    Phase 1\n    ```", "doc.md").is_empty());
}

// Regression for the plan/0022 design-review boxing: a host-lint:ignore region,
// then a REGULAR fenced code block that must still be scanned, then a second
// host-lint:ignore region. The bare fence closing the code block must not be read
// as an ignore boundary, and only a bare fence (never an info-string fence) closes
// an ignore region.
#[test]
fn host_lint_ignore_regions_flank_a_still_scanned_code_block() {
    let scan = |text: &str, src: &str| {
        let mut m = Vec::new();
        scan_text_with_allow_strict(text, src, &[], &[], false, &mut m);
        m
    };
    // ignore(Phase 1) | regular rust(Phase 2) | ignore(Phase 3).
    let doc = "```host-lint:ignore\ncite Phase 1 here\n```\n\n```rust\n// Phase 2 in real code\n```\n\n```host-lint:ignore\ncite Phase 3 here\n```";
    let m = scan(doc, "doc.md");
    assert_eq!(m.len(), 1, "only the regular code block's tell flags: {:?}",
        m.iter().map(|x| (x.line, x.text.clone())).collect::<Vec<_>>());
    assert!(m[0].text.contains("Phase 2"), "flagged line is the code-block tell: {:?}", m[0].text);
    // An info-string fence (```rust) inside an ignore region does NOT close it —
    // the whole region stays quarantined, fences and all.
    assert!(scan("```host-lint:ignore\nPhase 1\n```rust\nPhase 2\n```", "doc.md").is_empty(),
        "an info-string fence must not close an ignore region");
}

// host-lifecycle#2: the LEXICON loader and the `--docs` walk live in the shared engine,
// so an in-process embedder (host-lifecycle's prose recheck) masks exactly the phrases
// the CLI does. These two tests pin that contract through the public API.

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?} failed");
}

#[test]
fn load_lexicon_returns_validated_lowercased_phrases() {
    let dir = std::env::temp_dir().join(format!("hl-lex-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // Two declared phrases (mixed case), plus a master-key entry that must be dropped.
    fs::write(dir.join("LEXICON"), "Wdm-Harness\nthe harness\n*\n").unwrap();
    let lex = host_lint::load_lexicon(&dir);
    assert!(lex.phrases_lc.contains(&"wdm-harness".to_string()), "phrases are ASCII-lowercased");
    assert!(lex.phrases_lc.contains(&"the harness".to_string()));
    assert!(
        !lex.phrases_lc.iter().any(|p| p == "*"),
        "a master-key entry is dropped — it never masks"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_docs_masks_a_lexicon_declared_prose_tell() {
    let dir = std::env::temp_dir().join(format!("hl-docs-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    // "landscape" is a grandiose-noun term; two occurrences in one doc trip the
    // density warn. The declared phrase is hyphenated on purpose: a hyphen is a
    // non-word character inside the phrase, so it exercises the masker's boundary
    // handling, which a space-separated phrase (the sibling test's "apache tapestry")
    // does not reach. A fitness-landscape is genuine optimization vocabulary.
    fs::write(
        dir.join("doc.md"),
        "# Title\n\nThe fitness-landscape drives the lane. The landscape emits a verdict.\n",
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "doc"]);
    // Undeclared: the prose tell fires.
    let bare = host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::WorkingTree).unwrap();
    assert!(
        bare.matches.iter().any(|m| m.severity == Severity::Warn),
        "undeclared, the grandiose-noun term warns in the --docs walk"
    );
    // Declared: the same phrases are masked before detection, so the warn clears —
    // the in-process embedder gets the identical verdict to standalone `host-lint --docs`.
    // Declared through a real LEXICON, since run_docs resolves the scope of each
    // document it reads rather than taking a list from its caller.
    fs::write(dir.join("LEXICON"), "fitness-landscape\nthe landscape\n").unwrap();
    let masked =
        host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::WorkingTree)
            .unwrap();
    assert!(
        !masked.matches.iter().any(|m| m.severity == Severity::Warn),
        "a LEXICON-declared phrase clears the prose tell in the shared --docs walk"
    );
    let _ = fs::remove_dir_all(&dir);
}

// host-lint#17: `--docs` walks the authored working tree (tracked + staged + untracked
// non-ignored), so a brand-new doc is audited before it is staged and a pre-commit run is
// never silently clean over a file it skipped — while gitignored generated output stays out.
#[test]
fn run_docs_scans_untracked_authored_but_not_gitignored() {
    let dir = std::env::temp_dir().join(format!("hl-docs-untracked-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    // A decoration trope (an em dash) in three kinds of file.
    let trope = "# T\n\nWe shipped it \u{2014} and it works.\n";
    fs::write(dir.join("tracked.md"), trope).unwrap();
    // A gitignored generated tree that must NEVER be scanned.
    fs::write(dir.join(".gitignore"), "/gen/\n").unwrap();
    fs::create_dir_all(dir.join("gen")).unwrap();
    fs::write(dir.join("gen/page.md"), trope).unwrap();
    git(&dir, &["add", "tracked.md", ".gitignore"]);
    git(&dir, &["commit", "-qm", "init"]);
    // A brand-new authored doc, created but never staged.
    fs::write(dir.join("new.md"), trope).unwrap();

    let m = host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::WorkingTree).unwrap();
    let flagged: std::collections::HashSet<&str> = m.matches.iter().map(|x| x.file.as_str()).collect();
    assert!(flagged.contains("tracked.md"), "a tracked authored doc is scanned");
    assert!(
        flagged.contains("new.md"),
        "host-lint#17: a new untracked authored doc is scanned, not silently skipped"
    );
    assert!(
        !flagged.contains("gen/page.md"),
        "a gitignored generated doc stays excluded (--exclude-standard)"
    );
    let _ = fs::remove_dir_all(&dir);
}

// The gate judges the record; the hook scans the working tree (agentic-host call/0048).
// The same tree yields a different corpus under each, so an uncommitted draft can never
// redden a gate that a release depends on.
#[test]
fn the_record_corpus_leaves_the_uncommitted_draft_alone() {
    let dir = std::env::temp_dir().join(format!("hl-docs-corpus-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    let trope = "# T\n\nWe shipped it \u{2014} and it works.\n";
    fs::write(dir.join("tracked.md"), trope).unwrap();
    git(&dir, &["add", "tracked.md"]);
    git(&dir, &["commit", "-qm", "init"]);
    fs::write(dir.join("scratch.md"), trope).unwrap();

    let hook = host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::WorkingTree).unwrap();
    let hook_files: std::collections::HashSet<&str> =
        hook.matches.iter().map(|x| x.file.as_str()).collect();
    assert!(hook_files.contains("scratch.md"), "the hook still sees the uncommitted draft");

    let gate = host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::Record).unwrap();
    let gate_files: std::collections::HashSet<&str> =
        gate.matches.iter().map(|x| x.file.as_str()).collect();
    assert!(gate_files.contains("tracked.md"), "the gate still judges the recorded document");
    assert!(
        !gate_files.contains("scratch.md"),
        "a gate that reddens over an uncommitted note judges what no clone contains"
    );
    let _ = fs::remove_dir_all(&dir);
}

// A document the walk listed and could not read is reported, never dropped. Reporting the
// rest as clean would be a verdict over a corpus with a hole in it, which is the class
// agentic-host plan/0077 closed for its reference sweep and left open here.
#[cfg(unix)]
#[test]
fn a_document_that_cannot_be_read_is_named_rather_than_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("hl-docs-unread-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    fs::write(dir.join("clean.md"), "# T\n\nPlain prose.\n").unwrap();
    fs::write(dir.join("locked.md"), "# T\n\nWe shipped it \u{2014} and it works.\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    fs::set_permissions(dir.join("locked.md"), fs::Permissions::from_mode(0o000)).unwrap();

    let scan = host_lint::run_docs(&dir, &host_lint::LexiconScopes::new(&dir), &[], host_lint::Corpus::Record).unwrap();
    assert!(
        scan.unread.iter().any(|p| p == "locked.md"),
        "the walk names the document it could not open, so a caller can refuse"
    );
    assert!(
        !scan.matches.iter().any(|m| m.file == "locked.md"),
        "nothing is claimed about a document that was never read"
    );

    let _ = fs::set_permissions(dir.join("locked.md"), fs::Permissions::from_mode(0o644));
    let _ = fs::remove_dir_all(&dir);
}

// The vocabulary is a proxy for the tell, not the tell. Renaming `## Leg 1` to
// `## Check 1` passed the linter while keeping the tell entirely: a heading naming a
// position rather than content (host-lint#24). This catches the shape.
#[test]
fn a_noun_swap_does_not_evade_the_ordinal_heading_rule() {
    use host_lint::check_ordinal_scaffold_header as f;
    // The evasion from the issue, and its siblings.
    assert!(f("## Check 1").is_some(), "the exact evasion reported");
    assert!(f("## Part 2").is_some());
    assert!(f("### Item 3").is_some());
    assert!(f("## Check 1:").is_some(), "trailing punctuation does not save it");

    // The genuine fix in that issue: named by content, so it must stay clean.
    assert!(f("## MCP tool surface passes").is_none());
    assert!(f("## Step 3 of the plan").is_none(), "more than the bare shape is prose");

    // Not this rule's business: a version, a year, a status code. The bare-numeral
    // rule beside it already judges those, and double-reporting would be noise.
    assert!(f("## Rust 1.95").is_none());
    assert!(f("## 2024").is_none());
    assert!(f("## Release 404").is_none(), "three digits reads as a code");

    // The vocabulary still owns its own nouns, so this never double-reports.
    assert!(f("## Phase 1").is_none(), "FLAG_TERMS handles it; this rule stands aside");
    assert!(f("## Stage 2").is_none());

    // Not a heading at all.
    assert!(f("Check 1 of the sequence").is_none());
    assert!(f("####### Check 1").is_none(), "seven hashes is not a heading");
}

// The carve-out half of host-lint#24: the shape admits genuine designators the
// vocabulary rules never collide with, so it reports advisory; a LEXICON
// declaration masks a real designator, and strict escalates an undeclared one
// to a flag whose remedy names the declaration command.
#[test]
fn an_ordinal_scaffold_heading_warns_and_the_lexicon_carves_out_designators() {
    use host_lint::{classify_line, scan_text_with_allow_strict, Severity};
    assert_eq!(classify_line("## Check 1", true).map(|(s, _)| s), Some(Severity::Warn));
    assert_eq!(
        classify_line("## Step 3", true).map(|(s, _)| s),
        Some(Severity::Warn),
        "a paragraph-tier advisory noun is not harder-judged for being a whole heading"
    );

    // A declared designator is masked before classification, strict or not.
    let allow = vec!["windows 11".to_string()];
    let mut m = Vec::new();
    scan_text_with_allow_strict("## Windows 11\n", "cal.md", &allow, &[], true, &mut m);
    assert!(m.is_empty(), "a declared designator heading is clean under strict");

    // Undeclared under strict: escalated, and the remedy names the declaration.
    let mut m = Vec::new();
    scan_text_with_allow_strict("## Windows 11\n", "cal.md", &[], &[], true, &mut m);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].severity, Severity::Flag);
    assert!(m[0].cite.contains("lexicon add"), "cite was: {}", m[0].cite);

    // Undeclared without strict: advisory, the author confirms and declares.
    let mut m = Vec::new();
    scan_text_with_allow_strict("## Windows 11\n", "cal.md", &[], &[], false, &mut m);
    assert_eq!(m[0].severity, Severity::Warn);
}

// === LEXICON scopes: one repository can contain another ===============

/// A scratch tree under `target/` (gitignored, so a stray directory never
/// reaches a commit). Named per test, and cleared first so a re-run starts from
/// nothing rather than from what the last one left.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new("target").join("lexicon-scopes").join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch tree");
    dir
}

fn lexicon_at(dir: &Path, body: &str) {
    fs::create_dir_all(dir).expect("scope dir");
    fs::write(dir.join("LEXICON"), body).expect("LEXICON");
}

#[test]
fn a_nested_scope_adds_to_the_one_enclosing_it() {
    let root = scratch("adds");
    lexicon_at(&root, "outer phrase here\n");
    let inner = root.join("vendored");
    lexicon_at(&inner, "inner phrase here\n");

    let lex = host_lint::resolve_lexicon(&inner, &root);
    assert!(lex.phrases_lc.iter().any(|p| p == "inner phrase here"));
    assert!(
        lex.phrases_lc.iter().any(|p| p == "outer phrase here"),
        "an allowlist is additive, so the enclosing scope still applies"
    );
}

#[test]
fn a_scope_declaring_root_inherits_nothing() {
    let root = scratch("root-stops");
    lexicon_at(&root, "outer phrase here\n");
    let inner = root.join("template");
    lexicon_at(&inner, "# host-lint: root\ninner phrase here\n");

    let lex = host_lint::resolve_lexicon(&inner, &root);
    assert!(lex.is_root);
    assert!(lex.phrases_lc.iter().any(|p| p == "inner phrase here"));
    assert!(
        !lex.phrases_lc.iter().any(|p| p == "outer phrase here"),
        "a vendored repository answers to its own vocabulary, not its host's"
    );
}

#[test]
fn an_inner_scope_cannot_relax_an_outer_escalation() {
    let root = scratch("strict-holds");
    lexicon_at(&root, "# host-lint: strict\nouter phrase here\n");
    let inner = root.join("sub");
    lexicon_at(&inner, "inner phrase here\n");

    assert!(
        host_lint::resolve_lexicon(&inner, &root).strict,
        "strict holds if any scope in the chain declares it"
    );
}

#[test]
fn the_walk_does_not_read_a_lexicon_above_the_tree_under_audit() {
    let outside = scratch("bounded");
    lexicon_at(&outside, "phrase from outside\n");
    let root = outside.join("repo");
    lexicon_at(&root, "phrase from the repo\n");
    let inner = root.join("docs");
    fs::create_dir_all(&inner).expect("docs dir");

    let lex = host_lint::resolve_lexicon(&inner, &root);
    assert!(lex.phrases_lc.iter().any(|p| p == "phrase from the repo"));
    assert!(
        !lex.phrases_lc.iter().any(|p| p == "phrase from outside"),
        "a file above the repository does not decide what this one may say"
    );
}

#[test]
fn the_root_directive_is_a_directive_and_not_an_entry() {
    assert!(host_lint::is_root_directive("# host-lint: root"));
    assert!(host_lint::is_root_directive("  #   host-lint: root  "));
    assert!(!host_lint::is_root_directive("# host-lint: rooted"));
    assert!(!host_lint::is_root_directive("host-lint: root"));
    assert!(
        parse_lexicon_line("# host-lint: root").is_none(),
        "a directive never masks as a phrase"
    );
}

#[test]
fn a_bare_filename_resolves_where_a_dotted_one_does() {
    // `doc.md` and `./doc.md` name the same file. The first has an empty parent
    // rather than none, and an empty root reads no lexicon, so the two spellings
    // once disagreed about which phrases were declared.
    let root = scratch("bare-name");
    lexicon_at(&root, "declared phrase here\n");

    let scopes = host_lint::LexiconScopes::new(&root);
    let bare = scopes.for_file(Path::new("doc.md"));
    let dotted = scopes.for_file(&root.join("doc.md"));
    assert_eq!(bare.phrases_lc, dotted.phrases_lc);
    assert!(dotted.phrases_lc.iter().any(|p| p == "declared phrase here"));
}

#[test]
fn no_scope_declaring_strict_leaves_it_off() {
    // The other side of the merge rule: strict is a disjunction over the chain,
    // so a chain that declares it nowhere resolves to off rather than inheriting
    // an escalation nobody committed to.
    let root = scratch("strict-off");
    lexicon_at(&root, "outer phrase here\n");
    let inner = root.join("sub");
    lexicon_at(&inner, "inner phrase here\n");

    assert!(!host_lint::resolve_lexicon(&inner, &root).strict);
}

// === host-lint#28: the commit verb's duplication check, pure over two texts ===

#[test]
fn added_comment_shapes_gate_and_strip() {
    assert_eq!(added_comment_text("+// a note").as_deref(), Some("a note"));
    assert_eq!(added_comment_text("+/// doc line").as_deref(), Some("doc line"));
    assert_eq!(added_comment_text("+//! crate doc").as_deref(), Some("crate doc"));
    assert_eq!(added_comment_text("+# heading or shell").as_deref(), Some("heading or shell"));
    assert_eq!(added_comment_text("+## deeper").as_deref(), Some("deeper"));
    assert_eq!(added_comment_text("+/* block open").as_deref(), Some("block open"));
    assert_eq!(added_comment_text("+ * continuation").as_deref(), Some("continuation"));
    assert_eq!(added_comment_text("+-- lua or sql").as_deref(), Some("lua or sql"));
    assert_eq!(added_comment_text("+<!-- html note").as_deref(), Some("html note"));
    assert_eq!(added_comment_text("+let x = 1; // trailing"), None, "a code line is code, trailing comment or not");
    assert_eq!(added_comment_text("+++ b/src/main.rs"), None, "the file header is not content");
    assert_eq!(added_comment_text(" // context line"), None, "an unchanged line is not added");
    assert_eq!(added_comment_text("+- list item"), None, "a single dash is a list, not a comment");
    assert_eq!(added_comment_text("+--x"), None, "a double dash without its space is an expression");
}

#[test]
fn restate_normalization_is_the_calibrated_form() {
    assert_eq!(
        normalize_for_restate("An upstream, `set` by-hand:  KEPT (as) found."),
        "an upstream set by hand kept as found"
    );
    assert_eq!(normalize_for_restate("  .,;  "), "");
}

#[test]
fn sentences_split_at_boundaries_not_inside_versions() {
    assert_eq!(
        split_sentences("One two. Three four! v3.5 stays whole. Wait... done, tail"),
        vec!["One two", "Three four", "v3.5 stays whole", "Wait", "done, tail"]
    );
}

#[test]
fn a_restated_sentence_is_found_and_every_boundary_holds() {
    let diff = "+++ b/f.rs\n+/// The upstream that was set by hand is kept exactly as it was found.\n+/// Short line here.\n+fn f() {}\n";
    let body = "Explains itself.\n\nThe upstream that was set by hand is kept exactly as it was found.";
    let hits = restated_comment_sentences(diff, body);
    assert_eq!(hits.len(), 1, "the fourteen-word sentence matches; the three-word one is under the bar");
    assert!(hits[0].starts_with("The upstream"));

    let none = restated_comment_sentences(diff, "A different body with nothing repeated from the change at all.");
    assert!(none.is_empty(), "no restatement, no finding");

    let wrapped = "body where the upstream that was set by hand is kept\nexactly as it was found, wrapped";
    assert_eq!(restated_comment_sentences(diff, wrapped).len(), 1, "wrapping does not hide a restatement");

    let twice = format!("{diff}+// The upstream that was set by hand is kept exactly as it was found.\n");
    assert_eq!(restated_comment_sentences(&twice, body).len(), 1, "the same sentence twice reports once");
}
