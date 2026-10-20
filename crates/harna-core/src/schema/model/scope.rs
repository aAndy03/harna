//! Component `scope`: the one path-scope test.
//!
//! Rule scopes, container paths and template scopes all ask "does this
//! path lie under that one?" — on whole segments, with spellings
//! normalized. Pure and dependency-free so every schema component (and
//! every engine above) can share it without an import cycle.

/// The one scope test every rule scope, container path and template scope
/// uses: `path` is `scope` itself or lies under it on a whole segment
/// (`src/api` covers `src/api/x.rs`, never `src/api_v2/x.rs`). Both sides
/// are normalized — `./src/api/`, `src\api` and `src/api` are one scope.
/// An empty scope covers everything.
pub fn path_within(path: &str, scope: &str) -> bool {
    let scope = normalize_scope(scope);
    if scope.is_empty() {
        return true;
    }
    let path = path.replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    path == scope
        || path
            .strip_prefix(scope.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Container homing: like `path_within`, but an undeclared (empty)
/// container path homes nothing — only an empty *rule* scope is global.
pub fn path_in_container(path: &str, container_path: &str) -> bool {
    !normalize_scope(container_path).is_empty() && path_within(path, container_path)
}

/// A path glob (0.4.7, test kinds): `*` matches within one segment,
/// `**` any number of segments. `crates/*/tests/**` covers
/// `crates/app/tests/api.rs`; `**/*_tests.rs` covers any `*_tests.rs`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = normalize_scope(pattern);
    let path = path.replace('\\', "/");
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let s: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    fn seg(pat: &str, text: &str) -> bool {
        // `*` within a segment: split on it and match the pieces in order
        let parts: Vec<&str> = pat.split('*').collect();
        if parts.len() == 1 {
            return pat == text;
        }
        let mut rest = text;
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                let Some(r) = rest.strip_prefix(part) else {
                    return false;
                };
                rest = r;
            } else if i == parts.len() - 1 {
                return rest.ends_with(part);
            } else {
                let Some(at) = rest.find(part) else {
                    return false;
                };
                rest = &rest[at + part.len()..];
            }
        }
        true
    }
    fn walk(p: &[&str], s: &[&str]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(&"**") => (0..=s.len()).any(|k| walk(&p[1..], &s[k..])),
            Some(first) => !s.is_empty() && seg(first, s[0]) && walk(&p[1..], &s[1..]),
        }
    }
    walk(&p, &s)
}

/// `./src/api/` -> `src/api`; backslashes become `/`.
pub fn normalize_scope(scope: &str) -> String {
    let s = scope.trim().replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    let s = s.trim_end_matches('/');
    // `.` is the repo itself: a scope of `.` covers everything (0.3.44 —
    // it used to match no path at all, silently).
    if s == "." {
        String::new()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_segments_and_suffixes() {
        assert!(glob_match("crates/*/tests/**", "crates/app/tests/api.rs"));
        assert!(glob_match(
            "crates/*/tests/**",
            "crates/app/tests/common/mod.rs"
        ));
        assert!(!glob_match("crates/*/tests/**", "crates/app/src/api.rs"));
        assert!(glob_match("**/*_tests.rs", "crates/app/src/money_tests.rs"));
        assert!(!glob_match("**/*_tests.rs", "crates/app/src/money.rs"));
        assert!(glob_match("src/**", "src/a/b.rs"));
    }

    #[test]
    fn scopes_match_on_whole_segments_only() {
        assert!(path_within("src/api/x.rs", "src/api"));
        assert!(path_within("src/api", "src/api"));
        assert!(
            !path_within("src/api_v2/x.rs", "src/api"),
            "no partial-segment match"
        );
        assert!(!path_within("src/apix.rs", "src/api"));
        // Normalized spellings are one scope.
        for s in ["src/api/", "./src/api", "src\\api", " src/api "] {
            assert!(path_within("src/api/x.rs", s), "{s:?}");
        }
        assert!(path_within("src\\api\\x.rs", "src/api"));
        assert!(
            path_within("anything.rs", ""),
            "an empty rule scope is global"
        );
        // 0.3.44: `.` (and `./`) is the repo itself, not a path nobody has.
        for s in [".", "./", ".\\"] {
            assert!(path_within("ext/fs/ops.rs", s), "{s:?}");
        }
        assert!(
            !path_in_container("anything.rs", ""),
            "an empty container homes nothing"
        );
        assert!(!path_in_container("anything.rs", "/"));
        assert!(path_in_container("crates/x/src/a.rs", "crates/x/"));
    }
}
