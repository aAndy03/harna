//! The practices pack (1.1.1): every rule fires on the habit it names and
//! stays quiet on the healthy version of the same code — judged through
//! `try_rule`, exactly as validate judges a file.

use harna_core::enforcement::validate::content::pattern::try_rule;
use harna_core::generation::structure::plugin;

/// (rule id, file it is judged at, a hit, a clean twin)
const CASES: &[(&str, &str, &str, &str)] = &[
    (
        "errors-are-not-swallowed",
        "web/src/api.ts",
        "try { save(); } catch (e) {}",
        "try { save(); } catch (e) { report(e); throw e; }",
    ),
    (
        "errors-are-not-swallowed",
        "src/store.rs",
        "match load() { Ok(v) => use_it(v), Err(_) => {} }",
        "match load() { Ok(v) => use_it(v), Err(e) => return Err(e) }",
    ),
    (
        "no-silent-fallbacks",
        "web/src/api.ts",
        "try { return await get(); } catch { return []; }",
        "try { return await get(); } catch (e) { throw new LoadError(e); }",
    ),
    (
        "no-silent-fallbacks",
        "src/store.rs",
        "        Err(_) => Vec::new(),",
        "        Err(e) => return Err(e.into()),",
    ),
    (
        "no-placeholders",
        "src/billing.rs",
        "// TODO: handle refunds\nfn refund() { todo!() }",
        "// Refunds are out of scope until 1.2 (deferred: refunds).\nfn refund() -> Result<(), E> { Err(E::NotYet) }",
    ),
    (
        "no-debug-leftovers",
        "web/src/view.tsx",
        "console.log(order);",
        "logger.info('order saved', { id: order.id });",
    ),
    (
        "no-debug-leftovers",
        "src/view.rs",
        "dbg!(&order);",
        "tracing::info!(id = %order.id, \"saved\");",
    ),
    (
        "no-secret-shapes",
        "src/config.rs",
        "const KEY: &str = \"AKIAABCDEFGHIJKLMNOP\";",
        "let key = std::env::var(\"AWS_ACCESS_KEY_ID\")?;",
    ),
    (
        "no-secret-shapes",
        "web/src/client.ts",
        "const apiKey = \"sk_live_51Habcdefghij\";",
        "const apiKey = process.env.API_KEY;",
    ),
    // review of 1.1.2 (M6): the probes the first regexes missed
    (
        "errors-are-not-swallowed",
        "web/src/api.ts",
        "save().catch(() => {});",
        "save().catch((e) => report(e));",
    ),
    (
        "no-silent-fallbacks",
        "web/src/api.ts",
        "const rows = await get().catch(() => []);",
        "const rows = await get().catch(fail);",
    ),
    (
        "no-silent-fallbacks",
        "src/store.rs",
        "        Err(_) => return Vec::new();",
        "        Err(e) => return Err(e);",
    ),
    (
        "no-secret-shapes",
        "src/config.rs",
        "let db_password: &str = \"hunter2hunter2\";",
        "let db_password = std::env::var(\"DB_PASSWORD\")?;",
    ),
    (
        "no-secret-shapes",
        "web/src/client.ts",
        "const AUTH_TOKEN = \"ghp_abcdefghijklmnop1234\";",
        "const AUTH_TOKEN = process.env.AUTH_TOKEN;",
    ),
    (
        "design-tokens-not-raw-colors",
        "web/src/Button.tsx",
        "const style = { color: \"#ff0055\" };",
        "const style = { color: tokens.accent };",
    ),
    (
        "no-raw-pixel-sizes",
        "web/src/Card.tsx",
        "<div style={{ padding: \"13px\" }} />",
        "<div style={{ padding: space[3] }} />",
    ),
];

#[test]
fn every_practice_fires_on_its_habit_and_not_on_the_healthy_twin() {
    let pack = plugin::resolve("practices", None).expect("the practices pack ships");
    assert!(
        pack.meta.interface == 1 && !pack.rules().rules.is_empty(),
        "pack loads at interface 1"
    );
    let rules = pack.rules();
    for (id, file, hit, clean) in CASES {
        let found = try_rule(&rules, id, file, hit).unwrap();
        assert!(!found.is_empty(), "{id} misses: {hit}");
        let quiet = try_rule(&rules, id, file, clean).unwrap();
        assert!(
            quiet.is_empty(),
            "{id} fires on the healthy twin: {clean} -> {quiet:?}"
        );
    }
}

/// Prose about placeholders and fixtures quoting them are not placeholders
/// (1.1.1: dogfooded on harna, all 12 first findings were of these kinds).
#[test]
fn writing_about_placeholders_is_not_a_placeholder() {
    let rules = plugin::resolve("practices", None).unwrap().rules();
    for quiet in [
        "/// An answer that answers nothing: a TODO/TBD note, only punctuation.",
        "//! - `lines:all` reads every line (a TODO, a pasted key).",
        "    \"todo!(\",",
        "    \"pub fn average(a: &[Money]) -> Money { todo!() }\\n\",",
        "            \"TODO: wire later\",",
    ] {
        let found = try_rule(&rules, "no-placeholders", "src/x.rs", quiet).unwrap();
        assert!(found.is_empty(), "not a placeholder: {quiet} -> {found:?}");
    }
    for hit in [
        "    // TODO: refunds",
        "# FIXME later",
        "fn f() { unimplemented!() }",
        " * HACK: retry twice",
    ] {
        let found = try_rule(&rules, "no-placeholders", "src/x.rs", hit).unwrap();
        assert!(!found.is_empty(), "a placeholder: {hit}");
    }
}

#[test]
fn copied_files_are_named_by_their_path() {
    let rules = plugin::resolve("practices", None).unwrap().rules();
    for path in [
        "web/src/Calendar2.tsx",
        "src/auth_new.rs",
        "src/view_old.rs",
        "web/src/form-copy.ts",
    ] {
        let found = try_rule(&rules, "no-copied-files", path, "").unwrap();
        assert!(!found.is_empty(), "{path} is a copy");
    }
    for path in [
        "web/src/Calendar.tsx",
        "src/auth.rs",
        "src/sha2.rs.bak.md",
        "src/oauth2_client.rs",
        "src/oauth2.rs",
        "src/sha2.rs",
        "web/src/vec2.ts",
    ] {
        let found = try_rule(&rules, "no-copied-files", path, "").unwrap();
        assert!(found.is_empty(), "{path} is not a copy: {found:?}");
    }
}
