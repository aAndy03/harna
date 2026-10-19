//! `supers` (0.3.49, ledger field test): the moved file's own `super::`
//! paths. They name the file's parent module, so a move to another parent
//! silently re-points them; the move rewrites each to the old parent's
//! absolute `crate::` path, which still names the same items. Only paths
//! at the file's top level: inside an inline module (`mod tests { use
//! super::*; }`) `super` is the moved module itself and stays.

/// Rewrite the top-level `super::` / `super::super::` paths of `src`, a
/// file whose module was `old` (`schema::terms_data`). None when nothing
/// changed.
pub fn rewrite(src: &str, old: &str) -> Option<String> {
    let parent = old.rsplit_once("::").map(|(p, _)| p).unwrap_or("");
    let grand = parent.rsplit_once("::").map(|(g, _)| g).unwrap_or("");
    let abs = |m: &str| {
        if m.is_empty() {
            "crate::".to_string()
        } else {
            format!("crate::{m}::")
        }
    };
    let (to_parent, to_grand) = (abs(parent), abs(grand));
    let mut out = String::with_capacity(src.len());
    let mut depth = 0i32;
    // depths at which an inline module opened: inside one, keep `super`
    let mut inline: Vec<i32> = Vec::new();
    let mut changed = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let opens_mod = {
            let mut s = t;
            while s.starts_with("#[") {
                s = s.split_once(']').map(|(_, r)| r.trim_start()).unwrap_or("");
            }
            if s.starts_with("pub(") {
                s = s.split_once(')').map(|(_, r)| r.trim_start()).unwrap_or(s);
            }
            let s = s.strip_prefix("pub ").unwrap_or(s);
            s.starts_with("mod ") && s.contains('{')
        };
        let rewritten = if inline.is_empty() && !t.starts_with("//") {
            let l = line.replace("super::super::", "\u{0}");
            let l = l.replace("super::", &to_parent);
            l.replace('\u{0}', &to_grand)
        } else {
            line.to_string()
        };
        if rewritten != line {
            changed = true;
        }
        out.push_str(&rewritten);
        if opens_mod {
            inline.push(depth + 1);
        }
        depth += crate::search::digest::live::brace_delta(line);
        while inline.last().is_some_and(|d| *d > depth) {
            inline.pop();
        }
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests;
