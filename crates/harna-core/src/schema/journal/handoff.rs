//! Component `handoff`: the burst journal.
//!
//! "Where was I?" is answered by data, not archaeology: the agent records
//! the current-burst state (summary, declared next steps) before compaction
//! or context runs out, and every session-start surface (onboarding,
//! status, doctor) carries it. One file, overwritten by the next handoff —
//! the history is git blame, as everywhere in harna.

use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::schema::store::Store;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handoff {
    #[serde(default = "default_version")]
    pub schema_version: u32,
    /// When the burst state was recorded (RFC3339).
    pub created: String,
    /// What this burst was doing.
    pub summary: String,
    /// What the next session should pick up, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_next: Vec<String>,
    /// Open parity targets at record time — the session-start surfaces show
    /// the live count; this is what the recording agent believed.
    #[serde(default)]
    pub open_parity: usize,
}

fn default_version() -> u32 {
    crate::SCHEMA_VERSION
}

/// `.harna/handoff.toml` (created alongside the rest of the data dir).
pub fn path(store: &Store) -> PathBuf {
    store
        .manifest_path()
        .parent()
        .map(|p| p.join("handoff.toml"))
        .unwrap_or_else(|| PathBuf::from("handoff.toml"))
}

/// The current handoff, if one is recorded and parseable. An unparseable
/// file reads as no handoff; doctor reports the corruption.
pub fn load(store: &Store) -> Option<Handoff> {
    std::fs::read_to_string(path(store))
        .ok()
        .and_then(|raw| toml::from_str(&raw).ok())
}

/// Record this burst's state, overwriting any previous handoff.
///
/// `open_parity` is supplied by the caller (interface/composition) — the
/// schema layer must not reach into enforcement, so the live debt count is
/// handed in at the layer that is allowed to read the parity ledger.
pub fn save(
    store: &Store,
    summary: &str,
    declared_next: &[String],
    open_parity: usize,
) -> anyhow::Result<PathBuf> {
    if summary.trim().is_empty() {
        anyhow::bail!("handoff needs a summary — what was this burst doing?");
    }
    let handoff = Handoff {
        schema_version: crate::SCHEMA_VERSION,
        created: Utc::now().to_rfc3339(),
        summary: summary.trim().to_string(),
        declared_next: declared_next.iter().filter(|s| !s.trim().is_empty()).cloned().collect(),
        open_parity,
    };
    let file = path(store);
    let mut raw = String::new();
    raw.push_str("# handoff: the burst journal — recorded before compaction, read at session start.\n\
                  # Overwritten by your next `harna handoff`; history is git blame.\n");
    raw.push_str(&format!("schema_version = {}\n", handoff.schema_version));
    raw.push_str(&format!("created = \"{}\"\n", escape(&handoff.created)));
    raw.push_str(&format!("summary = \"{}\"\n", escape(&handoff.summary)));
    raw.push_str(&format!("declared_next = {}\n", toml_list(&handoff.declared_next)));
    raw.push_str(&format!("open_parity = {}\n", handoff.open_parity));
    crate::schema::store::atomic::write(&file, raw)?;
    Ok(file)
}

/// The burst is resumed: remove the handoff.
pub fn clear(store: &Store) -> anyhow::Result<bool> {
    let file = path(store);
    if file.exists() {
        std::fs::remove_file(&file)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Token-efficient view for the agents' session start (onboarding) and the
/// one-shot health line (status). Empty string when nothing is recorded.
pub fn render(handoff: &Handoff) -> String {
    let mut out = format!(
        "handoff recorded {} — {}\n",
        handoff.created.get(..10).unwrap_or(&handoff.created),
        handoff.summary,
    );
    if !handoff.declared_next.is_empty() {
        out.push_str("declared next:\n");
        for (i, step) in handoff.declared_next.iter().enumerate() {
            out.push_str(&format!("  {}. {step}\n", i + 1));
        }
    }
    out.push_str(&format!(
        "  parity believed open at record time: {} (live truth: harna parity show --open)\n",
        handoff.open_parity
    ));
    out
}

/// The paragraph for onboarding: empty is a legitimate answer (no debt).
pub fn onboarding_text(store: &Store) -> String {
    match load(store) {
        Some(h) => render(&h),
        None => String::new(),
    }
}

/// TOML basic-string escaping: quotes, backslashes and every control
/// character (a raw newline in a `"..."` string is invalid TOML, and
/// `load` would then read the whole burst journal as absent).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn toml_list(items: &[String]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", escape(s)))
        .collect();
    format!("[{}]", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir()
            .join(format!("harna-handoff-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(dir.join(".harna"));
        let store = Store::open(&dir).unwrap();
        store.init().unwrap();
        (dir, store)
    }

    #[test]
    fn save_overwrites_and_clear_removes() -> anyhow::Result<()> {
        let (dir, store) = fixture("overwrite");
        assert!(load(&store).is_none());
        assert!(onboarding_text(&store).is_empty(), "no burst yet, onboarding stays quiet");

        let p1 = save(&store, "first burst: shipped render", &["scan".to_string(), "commit".to_string()], 3)?;
        assert!(p1.ends_with("handoff.toml"));
        let h1 = load(&store).expect("recorded");
        assert_eq!(h1.summary, "first burst: shipped render");
        assert_eq!(h1.declared_next, vec!["scan".to_string(), "commit".to_string()]);
        assert_eq!(h1.open_parity, 3, "the caller hands in the live debt count");
        assert!(h1.created.len() >= 10);

        // Second handoff replaces the first — one burst state at a time.
        let _ = save(&store, "second burst", &[], 0)?;
        assert!(matches!(load(&store), Some(h) if h.summary == "second burst"));

        assert!(clear(&store)?, "clear removes a recorded burst");
        assert!(!clear(&store)?, "second clear reports already-clean");
        assert!(load(&store).is_none());
        std::fs::remove_dir_all(dir.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn empty_summary_is_rejected_and_empty_next_steps_are_skipped() -> anyhow::Result<()> {
        let (dir, store) = fixture("filter");
        assert!(save(&store, "   ", &[], 0).is_err());
        let _ = save(&store, "ok", &["".to_string(), "  ".to_string(), "real step".to_string()], 0)?;
        let h = load(&store).unwrap();
        assert_eq!(h.declared_next, vec!["real step".to_string()]);
        std::fs::remove_dir_all(dir.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn corpus_is_parseable_toml_and_quoting_survives() -> anyhow::Result<()> {
        let (dir, store) = fixture("toml");
        let _ = save(&store, "quote \" and backslash \\ old.md path \"x\"", &["fix \"thing\"".to_string()], 0)?;
        let raw = std::fs::read_to_string(path(&store))?;
        // The raw file is honest TOML, not a serialization side effect:
        // header + version + the escaped values.
        assert!(raw.starts_with("# handoff:"));
        assert!(raw.contains("declared_next = [\"fix \\\"thing\\\"\"]"));
        let h = load(&store).unwrap();
        assert_eq!(h.summary, "quote \" and backslash \\ old.md path \"x\"");
        assert_eq!(h.declared_next, vec!["fix \"thing\"".to_string()]);
        std::fs::remove_dir_all(dir.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn multiline_and_control_chars_round_trip() -> anyhow::Result<()> {
        let (dir, store) = fixture("multiline");
        let summary = "fixed X\nnext: Y\r\n\ttabbed \u{7} bell";
        let _ = save(&store, summary, &["line one\nline two".to_string()], 0)?;
        let h = load(&store).expect("a multi-line summary must stay parseable");
        assert_eq!(h.summary, summary.trim());
        assert_eq!(h.declared_next, vec!["line one\nline two".to_string()]);
        std::fs::remove_dir_all(dir.join(".harna"))?;
        Ok(())
    }

    #[test]
    fn render_is_telegraphic_and_onboarding_paragraph_flows() -> anyhow::Result<()> {
        let (dir, store) = fixture("render");
        let _ = save(
            &store,
            "mid-burst",
            &["next A".to_string(), "next B".to_string()],
            0,
        )?;
        let h = load(&store).unwrap();
        let text = render(&h);
        assert!(text.contains("handoff recorded"));
        assert!(text.contains("1. next A"));
        assert!(text.contains("2. next B"));
        assert!(text.contains("harna parity show --open"));
        let onb = onboarding_text(&store);
        assert_eq!(onb, text);
        std::fs::remove_dir_all(dir.join(".harna"))?;
        Ok(())
    }
}
