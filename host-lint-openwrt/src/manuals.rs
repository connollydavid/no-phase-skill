//! The vendored style manuals, pinned by whole-file digest.
//!
//! These four files are the pack's rule sources: project-authored measurements of
//! what `openwrt/packages` actually does. They are vendored and pinned, because a
//! rule that changes without the registry changing is exactly what a digest pin
//! exists to catch.
//!
//! An artefact the pack checks a submission *against* gets the opposite treatment.
//! `.github/pull_request_template.md` is a file upstream ships and moves, so it is
//! referenced and resolved at check time, never frozen into this directory. A
//! vendored copy would be wrong the moment upstream edited it, and wrong silently.
//! The decision is recorded as `call/0055-upstream-artefacts-are-referenced-not-embedded`
//! in the agentic-host repository (named rather than linked: that record lives in a
//! different repository, and a relative path would dangle for anyone who cloned
//! host-lint on its own).

/// One vendored manual: the bytes, the file name, and the digest they must hash to.
pub struct Manual {
    pub name: &'static str,
    pub sha256: &'static str,
    pub text: &'static str,
}

pub const DESCRIPTION: Manual = Manual {
    name: "openwrt-description-style.md",
    sha256: "8bc8f3c53001f369aca1cd710ded4072c24907c9aa09ae3c7c6078e511c52103",
    text: include_str!("../fixtures/manuals/openwrt-description-style.md"),
};

pub const COMMENT: Manual = Manual {
    name: "openwrt-comment-style.md",
    sha256: "661a0ae31c98d167d8b729247f786debe8ea90155ab05095e73f302b83971611",
    text: include_str!("../fixtures/manuals/openwrt-comment-style.md"),
};

pub const COMMIT: Manual = Manual {
    name: "openwrt-package-commit-style.md",
    sha256: "1106c598e5c24f2367b4ddc62d454bd7088bc50ab84508e33761aa154e725229",
    text: include_str!("../fixtures/manuals/openwrt-package-commit-style.md"),
};

pub const PR: Manual = Manual {
    name: "openwrt-pr-style.md",
    sha256: "0fff7d6b21b84949a67e6c7ab722bc31d45f72281c4a54d3ba56fb77db40db84",
    text: include_str!("../fixtures/manuals/openwrt-pr-style.md"),
};

/// Every vendored manual, in the order the lanes are declared.
pub const ALL: &[&Manual] = &[&DESCRIPTION, &COMMENT, &COMMIT, &PR];

#[cfg(test)]
mod tests {
    use super::*;

    // The pin. A manual edited without its digest re-recorded reddens here rather
    // than silently changing what the pack enforces, which is the whole reason the
    // sources are vendored instead of fetched.
    #[test]
    fn every_manual_matches_its_recorded_digest() {
        for m in ALL {
            let got = crate::sha256::hex(m.text.as_bytes());
            assert_eq!(
                got, m.sha256,
                "{} drifted from its recorded digest; re-record it in the same commit that edits it",
                m.name
            );
        }
    }

    // The four are distinct files, so a copy-paste that pointed two entries at one
    // manual would otherwise pass the digest test above.
    #[test]
    fn the_four_manuals_are_distinct() {
        for (i, a) in ALL.iter().enumerate() {
            for b in ALL.iter().skip(i + 1) {
                assert_ne!(a.name, b.name, "duplicate manual name");
                assert_ne!(a.sha256, b.sha256, "duplicate manual digest");
            }
        }
    }

    // The commit manual's exemplars quote real commit subjects, so they are fenced
    // `host-lint:ignore`. Losing those fences on a re-import would make the prose
    // audit fire on the quotation; they are load-bearing, not decoration.
    #[test]
    fn the_commit_exemplar_fences_survive_import() {
        let n = COMMIT.text.lines().filter(|l| *l == "```host-lint:ignore").count();
        assert_eq!(n, 4, "the four quoted exemplars must keep their ignore fences");
    }
}
