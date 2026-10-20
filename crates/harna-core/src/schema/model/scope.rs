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
    let scope = normal(scope);
    if scope.is_empty() {
        return true;
    }
    let path = slashed(path);
    let path = path.strip_prefix("./").unwrap_or(&path);
    path == scope.as_ref()
        || path
            .strip_prefix(scope.as_ref())
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Container homing: like `path_within`, but an undeclared (empty)
/// container path homes nothing — only an empty *rule* scope is global.
pub fn path_in_container(path: &str, container_path: &str) -> bool {
    !normal(container_path).is_empty() && path_within(path, container_path)
}

/// The value under the deepest folder of `path` that `by_path` holds (keys
/// normalized, no empty key): what `path_in_container` over every
/// container, longest first, answers, in O(path depth) instead of
/// O(containers) (0.9.1: 98k modules x 1 700 containers per check).
pub fn deepest<'a, T>(
    by_path: &'a std::collections::HashMap<String, T>,
    path: &str,
) -> Option<&'a T> {
    let path = slashed(path);
    let mut p: &str = path.strip_prefix("./").unwrap_or(&path);
    loop {
        if let Some(v) = by_path.get(p) {
            return Some(v);
        }
        p = &p[..p.rfind('/')?];
    }
}

/// Every value whose key is a folder holding `path` (itself included),
/// deepest first: `path_in_container` against each key, without scanning
/// them all.
pub fn enclosing<'a, T>(
    by_path: &'a std::collections::HashMap<String, Vec<T>>,
    path: &str,
) -> Vec<&'a T> {
    let path = slashed(path);
    let mut p: &str = path.strip_prefix("./").unwrap_or(&path);
    let mut out = Vec::new();
    loop {
        if let Some(v) = by_path.get(p) {
            out.extend(v.iter());
        }
        match p.rfind('/') {
            Some(i) => p = &p[..i],
            None => return out,
        }
    }
}

/// Index labels by their normalized container path (empty paths home
/// nothing and are left out).
pub fn index_by_path<T>(
    items: impl IntoIterator<Item = (String, T)>,
) -> std::collections::HashMap<String, Vec<T>> {
    let mut map: std::collections::HashMap<String, Vec<T>> = std::collections::HashMap::new();
    for (path, item) in items {
        let key = normalize_scope(&path);
        if !key.is_empty() {
            map.entry(key).or_default().push(item);
        }
    }
    map
}

/// Forward slashes, borrowed when there is nothing to replace (0.9.1: the
/// homing helpers allocated three strings per call, and checks call them
/// files x containers times).
fn slashed(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\\') {
        std::borrow::Cow::Owned(s.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// `normalize_scope` without allocating in the common case.
fn normal(scope: &str) -> std::borrow::Cow<'_, str> {
    match slashed(scope.trim()) {
        std::borrow::Cow::Borrowed(s) => {
            let s = s.strip_prefix("./").unwrap_or(s).trim_end_matches('/');
            std::borrow::Cow::Borrowed(if s == "." { "" } else { s })
        }
        std::borrow::Cow::Owned(_) => std::borrow::Cow::Owned(normalize_scope(scope)),
    }
}

/// A path glob (0.4.7, test kinds): `*` matches within one segment,
/// `**` any number of segments. `crates/*/tests/**` covers
/// `crates/app/tests/api.rs`; `**/*_tests.rs` covers any `*_tests.rs`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = normalize_scope(pattern);
    let path = path.replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
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
