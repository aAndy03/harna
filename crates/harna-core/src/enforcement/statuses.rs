//! `parity-statuses-loader` — the single loader for the `parity-statuses`
//! data source (statuses.toml). All consumers of parity statuses import this
//! loader and nothing else; the toml file is the point of truth and stays
//! code-free.

const RAW: &str = include_str!("statuses.toml");

/// The status ids in declaration order (the ledger's logical lifecycle).
pub fn all() -> Vec<String> {
    entries().into_iter().map(|e| e.key).collect()
}

/// Status ids with their human explanations — the ledger's vocabulary.
pub fn explain() -> Vec<(String, String)> {
    entries().into_iter().map(|e| (e.key, e.value)).collect()
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RawEntries {
    #[serde(default, rename = "entries")]
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct Entry {
    key: String,
    #[serde(default)]
    value: String,
}

fn entries() -> Vec<Entry> {
    let parsed: RawEntries =
        toml::from_str(RAW).expect("statuses.toml must be valid data (no code)");
    parsed.entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_come_from_the_data_file() {
        assert_eq!(all(), vec!["open", "ack", "noop", "auto", "origin"]);
        let explained = explain();
        assert!(explained.iter().all(|(_, v)| !v.is_empty()));
    }
}
