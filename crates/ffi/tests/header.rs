//! `include/plumcraft.h` is kept by hand: its functions must be exactly the library's C exports
//! (`#[unsafe(no_mangle)]` functions named `plum_*` in `src/lib.rs`), and its constants the
//! library's.

use std::collections::BTreeSet;

fn exports(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut lines = src.lines().peekable();
    while let Some(l) = lines.next() {
        if l.trim() == "#[unsafe(no_mangle)]"
            && let Some(sig) = lines.peek()
            && let Some(rest) = sig.split("fn ").nth(1)
        {
            out.insert(rest.split('(').next().unwrap_or("").trim().to_owned());
        }
    }
    out
}

fn declared(header: &str) -> BTreeSet<String> {
    header
        .lines()
        .filter(|l| !l.trim_start().starts_with("typedef") && !l.trim_start().starts_with('*') && !l.trim_start().starts_with("/*"))
        .filter_map(|l| {
            let before = l.split('(').next()?;
            let name = before.split_whitespace().last()?.trim_start_matches('*');
            (name.starts_with("plum_") && l.contains('(')).then(|| name.to_owned())
        })
        .collect()
}

#[test]
fn the_header_lists_every_export_and_nothing_else() {
    let src = include_str!("../src/lib.rs");
    let header = include_str!("../include/plumcraft.h");
    let (rust, c) = (exports(src), declared(header));
    assert!(rust.len() >= 9, "{rust:?}");
    assert_eq!(
        rust,
        c,
        "plumcraft.h and src/lib.rs disagree: only in Rust {:?}, only in the header {:?}",
        rust.difference(&c).collect::<Vec<_>>(),
        c.difference(&rust).collect::<Vec<_>>()
    );
}

#[test]
fn the_header_constants_are_the_library_s() {
    let header = include_str!("../include/plumcraft.h");
    let value = |name: &str| -> i64 {
        let line = header.lines().find(|l| l.starts_with(&format!("#define {name} "))).unwrap_or_else(|| panic!("no {name} in the header"));
        line.split_whitespace().nth(2).unwrap().trim_matches(|c| c == '(' || c == ')').parse().unwrap()
    };
    assert_eq!(value("PLUM_ABI_VERSION"), i64::from(plumcraft_ffi::PLUM_ABI_VERSION));
    assert_eq!(value("PLUM_OK"), i64::from(plumcraft_ffi::PLUM_OK));
    assert_eq!(value("PLUM_ERR_CORE"), i64::from(plumcraft_ffi::PLUM_ERR_CORE));
    assert_eq!(value("PLUM_ERR_ARG"), i64::from(plumcraft_ffi::PLUM_ERR_ARG));
    assert_eq!(value("PLUM_ERR_MISSING"), i64::from(plumcraft_ffi::PLUM_ERR_MISSING));
    assert_eq!(value("PLUM_ERR_PANIC"), i64::from(plumcraft_ffi::PLUM_ERR_PANIC));
}

#[test]
fn the_parsers_see_what_they_should() {
    assert_eq!(exports("#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn plum_x(a: u8) -> u8 {"), BTreeSet::from(["plum_x".to_owned()]));
    assert_eq!(
        declared("/* plum_y(…) */\nplum_buf plum_poll(plum_core core);\ntypedef void (*plum_wake_fn)(void *ctx);"),
        BTreeSet::from(["plum_poll".to_owned()])
    );
}
