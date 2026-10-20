use super::*;

fn err(check: &str, file: &str, line: usize) -> Violation {
    Violation {
        check: check.into(),
        file: file.into(),
        line,
        message: "found".into(),
        hint: String::new(),
        severity: Severity::Error,
    }
}

/// deno (0.3.44): `EXPECTED_VIOLATIONS` maps 26 polyfill files to counts
/// that may only go down. The same ratchet, as harna data: adopt an error
/// check on legacy code without either failing it or waving it through.
#[test]
fn a_recorded_baseline_absorbs_todays_debt_and_blocks_growth() {
    let today = "2026-10-19";
    let found = vec![
        err("no-panic", "ext/fs.rs", 3),
        err("no-panic", "ext/fs.rs", 9),
        err("no-panic", "ext/net.rs", 4),
        err("layer-boundary", "cli/a.rs", 1),
    ];
    let mut ledger = Baselines::default();
    assert_eq!(record(&mut ledger, &found, Some("no-panic"), today), 2);
    assert_eq!(ledger.allowed("no-panic", "ext/fs.rs"), Some(2));
    assert_eq!(
        ledger.allowed("layer-boundary", "cli/a.rs"),
        None,
        "filtered"
    );

    // Same debt: absorbed into receipts; the unbaselined check stays live.
    let (live, receipts) = partition(&ledger, found.clone());
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].check, "layer-boundary");
    assert_eq!(receipts.len(), 2);

    // Growth in one file: every error there is live, the overage named.
    let mut grown = found.clone();
    grown.push(err("no-panic", "ext/fs.rs", 20));
    let (live, _) = partition(&ledger, grown);
    let fs_live: Vec<_> = live.iter().filter(|v| v.file == "ext/fs.rs").collect();
    assert_eq!(fs_live.len(), 3);
    assert!(fs_live[0].message.contains("over baseline: 3 > 2 allowed"));

    // Paid down: the receipt shows the slack; tighten lowers, never raises.
    let fewer = vec![err("no-panic", "ext/fs.rs", 3)];
    let (_, receipts) = partition(&ledger, fewer.clone());
    let fs = receipts.iter().find(|r| r.file == "ext/fs.rs").unwrap();
    assert_eq!((fs.found, fs.allowed), (1, 2));
    assert_eq!(tighten(&mut ledger, &fewer, today), (1, 1));
    assert_eq!(ledger.allowed("no-panic", "ext/fs.rs"), Some(1));
    assert_eq!(ledger.allowed("no-panic", "ext/net.rs"), None, "paid off");
    assert_eq!(tighten(&mut ledger, &found, today), (0, 0), "never raises");
    assert_eq!(ledger.total(), 1);
}

#[test]
fn infos_and_empty_ledgers_pass_through_untouched() {
    let mut info = err("hotspot", "a.rs", 0);
    info.severity = Severity::Info;
    let (live, receipts) = partition(&Baselines::default(), vec![info.clone()]);
    assert_eq!((live.len(), receipts.len()), (1, 0));
    let mut ledger = Baselines::default();
    ledger.set("hotspot", "a.rs", 1, "today");
    let (live, _) = partition(&ledger, vec![info]);
    assert_eq!(live.len(), 1, "baselines hold errors only");
}

/// 0.9.2 (codex field test): a contract break, an owed parity debt and
/// unreadable data are never frozen — not by `record`, not by an old entry.
#[test]
fn breaks_and_debts_are_never_baselined() {
    let today = "2026-10-20";
    let found = vec![
        err("contract", ".harna/contracts/core.toml", 0),
        err("parity-open", ".harna/parity/v1.toml", 0),
        err("no-panic", "src/a.rs", 1),
    ];
    let mut ledger = Baselines::default();
    assert_eq!(record(&mut ledger, &found, None, today), 1, "only no-panic");
    ledger.set("contract", ".harna/contracts/core.toml", 1, today);
    let (live, _) = partition(&ledger, found);
    assert!(
        live.iter().any(|v| v.check == "contract"),
        "an old entry absorbs nothing"
    );
    assert!(live.iter().any(|v| v.check == "parity-open"));
}
