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

/// `./src/api/` -> `src/api`; backslashes become `/`.
pub fn normalize_scope(scope: &str) -> String {
    let s = scope.trim().replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    s.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_match_on_whole_segments_only() {
        assert!(path_within("src/api/x.rs", "src/api"));
        assert!(path_within("src/api", "src/api"));
        assert!(!path_within("src/api_v2/x.rs", "src/api"), "no partial-segment match");
        assert!(!path_within("src/apix.rs", "src/api"));
        // Normalized spellings are one scope.
        for s in ["src/api/", "./src/api", "src\\api", " src/api "] {
            assert!(path_within("src/api/x.rs", s), "{s:?}");
        }
        assert!(path_within("src\\api\\x.rs", "src/api"));
        assert!(path_within("anything.rs", ""), "an empty rule scope is global");
        assert!(!path_in_container("anything.rs", ""), "an empty container homes nothing");
        assert!(!path_in_container("anything.rs", "/"));
        assert!(path_in_container("crates/x/src/a.rs", "crates/x/"));
    }
}
