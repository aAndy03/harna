use super::*;

#[test]
fn flatten_keeps_aliases_globs_and_self() {
    assert_eq!(
        flatten("a::{b::{C as D, *}, self, e}"),
        vec!["a::b::C as D", "a::b::*", "a", "a::e"]
    );
    assert_eq!(flatten("x::y"), vec!["x::y"]);
    assert_eq!(flatten("m::{self as n}"), vec!["m as n"]);
}

/// 0.3.46 dogfood: moving `commands/ack.rs` under `commands/registers/`
/// missed `use crate::commands::{ack, other};`.
#[test]
fn only_statements_touching_the_moved_path_are_flattened() {
    let src = "use std::{fs, io};\n    pub(crate) use crate::commands::{\n        ack::run,\n        other,\n    };\nfn f() {}\n";
    let out = expand(
        src,
        |l| l.starts_with("crate::commands::ack"),
        |l| l.replace("crate::commands::ack", "crate::commands::registers::ack"),
    )
    .unwrap();
    assert_eq!(
        out,
        "use std::{fs, io};\n    pub(crate) use crate::commands::registers::ack::run;\n    pub(crate) use crate::commands::other;\nfn f() {}\n"
    );
    assert!(expand(src, |_| false, |l| l.to_string()).is_none());
}
