//! Component `validate-web` (1.1.1, moved 1.1.3): TypeScript and
//! JavaScript get the text checks too (a frontend got no code-pattern,
//! banned-api, URL or size check at all before 1.1.1). The Rust-specific
//! readers (boundaries by `use`, gateways, suppressions) keep reading Rust.

use std::path::Path;

use super::super::content::{self, pattern::ActivePattern};
use super::super::{check_accretion, hygiene, sizing::budgets, urls, Violation, ACCRETION_LINES};
use crate::schema::model::rules::Rules;

/// What one web pass needs, read-only and shared by every thread.
pub struct Inputs<'a> {
    pub root: &'a Path,
    pub rules: &'a Rules,
    pub env: Option<&'a str>,
    pub code_rules: &'a [content::pattern::code::BoundRule<'a>],
    pub pattern_rules: &'a [ActivePattern],
    pub generated: &'a [String],
}

/// The findings, and (path, line count) of each file read — all the
/// growth check needs (the review of 1.1.2: the sources were kept whole).
pub fn check(inp: &Inputs, files: &[String]) -> (Vec<Violation>, Vec<(String, usize)>) {
    let results = super::parallel::map(files, |file_rel| one(inp, file_rel));
    let mut found = Vec::new();
    let mut sizes = Vec::new();
    for (v, size) in results.into_iter().flatten() {
        found.extend(v);
        sizes.push(size);
    }
    (found, sizes)
}

fn one(inp: &Inputs, file_rel: &String) -> Option<(Vec<Violation>, (String, usize))> {
    if crate::search::digest::is_test_path(file_rel)
        || hygiene::generated::is_generated(inp.generated, file_rel)
    {
        return None;
    }
    let src = std::fs::read_to_string(inp.root.join(file_rel)).ok()?;
    let mut out = Vec::new();
    urls::check(&src, file_rel, &mut out);
    content::banned::check(inp.rules, inp.env, file_rel, &src, &mut out);
    content::pattern::code::check(inp.code_rules, file_rel, &src, &mut out);
    // path conventions (a copied `Calendar2.tsx`) hold for web files too
    content::pattern::apply(inp.pattern_rules, file_rel, &[], &mut out);
    let accretion = budgets::bound_limit(inp.rules, inp.env, "component-granularity", file_rel)
        .map(|l| l as usize)
        .unwrap_or(ACCRETION_LINES);
    check_accretion(file_rel, &src, accretion, &mut out);
    Some((out, (file_rel.clone(), src.lines().count())))
}
