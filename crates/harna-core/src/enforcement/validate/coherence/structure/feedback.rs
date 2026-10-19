//! `feedback` (0.3.48, codex/zed/ruff study H12): cycles at scale, as the
//! few edges that break them. After adoption codex showed 450 rings, zed
//! 281, ruff 248 — many sharing one import (`analytics/events.rs` ↔
//! `facts.rs`). A list of rings is unreadable; the edges to cut are not.
//!
//! Rings are found by depth-first search (each back edge closes one), then
//! reduced greedily: take the edge that sits in the most unbroken rings,
//! drop it, repeat; search again until the graph is acyclic (cutting an
//! edge can expose a ring the first search reached another way). The
//! result is a small feedback edge set — not provably minimal (that is
//! NP-hard), but each edge is named with the rings it breaks.

use std::collections::{BTreeMap, BTreeSet};

/// One import whose removal breaks `rings` rings; `example` is one of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Feedback {
    pub from: String,
    pub to: String,
    pub rings: usize,
    pub example: Vec<String>,
}

pub fn edges(graph: &BTreeMap<&str, Vec<&str>>) -> Vec<Feedback> {
    let mut g: BTreeMap<String, BTreeSet<String>> = graph
        .iter()
        .map(|(k, v)| (k.to_string(), v.iter().map(|t| t.to_string()).collect()))
        .collect();
    let mut out: Vec<Feedback> = Vec::new();
    loop {
        let mut open = rings(&g);
        if open.is_empty() {
            break;
        }
        while !open.is_empty() {
            let mut count: BTreeMap<(&str, &str), usize> = BTreeMap::new();
            for ring in &open {
                for w in ring.windows(2) {
                    *count.entry((w[0].as_str(), w[1].as_str())).or_default() += 1;
                }
            }
            // most rings first; ties go to the smallest edge (deterministic)
            let (&(from, to), &n) = count
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                .expect("an open ring has edges");
            let (from, to) = (from.to_string(), to.to_string());
            let in_edge = |r: &Vec<String>| r.windows(2).any(|w| w[0] == from && w[1] == to);
            let example = open
                .iter()
                .find(|r| in_edge(r))
                .cloned()
                .unwrap_or_default();
            open.retain(|r| !in_edge(r));
            if let Some(targets) = g.get_mut(&from) {
                targets.remove(&to);
            }
            match out.iter_mut().find(|f| f.from == from && f.to == to) {
                Some(f) => f.rings += n,
                None => out.push(Feedback {
                    from,
                    to,
                    rings: n,
                    example,
                }),
            }
        }
    }
    out.sort_by(|a, b| b.rings.cmp(&a.rings).then_with(|| a.from.cmp(&b.from)));
    out
}

/// The rings a depth-first search closes, each rotated so its smallest
/// member opens and listed once (`a -> b -> a`).
pub fn rings(g: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        Grey,
        Black,
    }
    let mut color: BTreeMap<&str, Color> = BTreeMap::new();
    let mut found: BTreeSet<Vec<String>> = BTreeSet::new();
    for start in g.keys() {
        if color.contains_key(start.as_str()) {
            continue;
        }
        // iterative DFS: (node, next child index)
        let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
        color.insert(start.as_str(), Color::Grey);
        while let Some((node, i)) = stack.last().copied() {
            let next = g.get(node).and_then(|t| t.iter().nth(i));
            let Some(next) = next else {
                color.insert(node, Color::Black);
                stack.pop();
                continue;
            };
            stack.last_mut().expect("non-empty").1 += 1;
            match color.get(next.as_str()) {
                Some(Color::Black) => {}
                Some(Color::Grey) => {
                    if let Some(pos) = stack.iter().position(|(n, _)| *n == next) {
                        let mut ring: Vec<String> =
                            stack[pos..].iter().map(|(n, _)| n.to_string()).collect();
                        ring.push(next.clone());
                        found.insert(normalize(&ring));
                    }
                }
                None => {
                    color.insert(next.as_str(), Color::Grey);
                    stack.push((next.as_str(), 0));
                }
            }
        }
    }
    found.into_iter().collect()
}

/// Rotate the ring so its smallest member opens — the same ring reports
/// once, deterministically, whichever member's search closed it.
fn normalize(ring: &[String]) -> Vec<String> {
    let members = &ring[..ring.len() - 1];
    let best = (0..members.len()).min_by_key(|&i| &members[i]).unwrap_or(0);
    let n = members.len();
    let mut rotated: Vec<String> = (0..n).map(|i| members[(best + i) % n].clone()).collect();
    rotated.push(members[best].clone());
    rotated
}

#[cfg(test)]
mod tests;
