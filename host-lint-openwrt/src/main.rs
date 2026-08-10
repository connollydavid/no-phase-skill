//! `host-lint-openwrt`: the OpenWrt package style rules as a checkable corpus.
//!
//! Four lanes, one per vendored style manual: `meta` (TITLE and description),
//! `comment`, `msg` and `pr`. The first three read artefacts that live in a clone.
//! The fourth does not: a pull request body lives on the forge, so that lane takes
//! its input from an argument or standard input and refuses a directory rather
//! than sweeping a tree for something that is not in it.
//!
//! This binary is the pack's entry point. The lanes land task by task under
//! plan/0081 in the agentic-host repository; what is wired here is the corpus and
//! its pin, so the manuals cannot drift unnoticed while the checkers are written.

mod manuals;
mod sha256;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("manuals") => {
            for m in manuals::ALL {
                println!("{}  {}", &m.sha256[..16], m.name);
            }
        }
        _ => {
            eprintln!("usage: host-lint-openwrt manuals");
            eprintln!("  the meta, comment, msg and pr lanes land under plan/0081");
            std::process::exit(2);
        }
    }
}
