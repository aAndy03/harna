//! Component `purpose` (0.4.4): placement by what containers are FOR, not
//! only by words the code already contains.
//!
//! The household field test asked `place` for persistence and never got
//! the store layer: matching ran over existing signatures only, and a
//! fresh area has none. Containers, layers and components carry declared
//! purpose (description, summary); slots carry the future's. This ranks
//! them against the idea with light stemming ("persist" meets
//! "persistence") and the project's own glossary (an alias reaches its
//! word). No vocabulary is built in: the words are the project's.

use crate::schema::model::manifest::Manifest;

/// A crude, language-agnostic stem: lowercase, common English suffixes
/// stripped (0.4.9: `storage` kept `storag` and missed `store`; `users`
/// missed `user`), then at most 5 leading chars — "persist/persistence",
/// "store/storage", "report/reporting", "user/users", "cache/caches".
fn stem(word: &str) -> String {
    let w = word.to_lowercase();
    let mut base = w.as_str();
    for suffix in [
        "ences", "ence", "ances", "ance", "ings", "ing", "ages", "age", "es", "s", "e",
    ] {
        if let Some(b) = base.strip_suffix(suffix) {
            if b.chars().count() >= 3 {
                base = b;
                break;
            }
        }
    }
    base.chars().take(5).collect()
}

fn stems(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(stem)
        .collect()
}

/// The idea's stems, widened by the glossary: a token that is an alias of
/// a term also brings the term's word.
pub fn idea_stems(
    tokens: &[String],
    terms: &crate::schema::registers::terms::Terms,
) -> Vec<String> {
    let mut out: Vec<String> = tokens.iter().map(|t| stem(t)).collect();
    for t in &terms.terms {
        let hit = t
            .aliases
            .iter()
            .chain(std::iter::once(&t.word))
            .any(|a| tokens.iter().any(|tok| stem(a) == stem(tok)));
        if hit {
            out.extend(stems(&t.word));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Containers ranked by declared purpose: (score, layer, container, why).
pub fn homes(manifest: &Manifest, idea: &[String]) -> Vec<(usize, String, String, String)> {
    let count = |text: &str| stems(text).iter().filter(|s| idea.contains(s)).count();
    let mut ranked = Vec::new();
    for l in &manifest.layers {
        for c in &l.containers {
            let summaries: String = c
                .components
                .iter()
                .map(|k| format!("{} {}", k.name, k.summary))
                .collect::<Vec<_>>()
                .join(" ");
            let score = 3 * count(&c.name)
                + 2 * count(&c.description)
                + count(&summaries)
                + count(&format!("{} {}", l.name, l.description));
            if score > 0 {
                let why = if !c.description.is_empty() {
                    c.description.clone()
                } else {
                    l.description.clone()
                };
                ranked.push((score, l.name.clone(), c.name.clone(), why));
            }
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    ranked
}

/// The briefing block: the top homes by purpose, when any.
pub fn homes_block(manifest: &Manifest, idea: &[String]) -> String {
    let top: Vec<_> = homes(manifest, idea).into_iter().take(3).collect();
    if top.is_empty() {
        return String::new();
    }
    let mut out = String::from("homes by purpose (declared descriptions that fit the idea):\n");
    for (_, layer, container, why) in top {
        let why = if why.is_empty() {
            String::new()
        } else {
            format!(" — {why}")
        };
        out.push_str(&format!(
            "  - container `{container}` [layer {layer}]{why}\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests;
