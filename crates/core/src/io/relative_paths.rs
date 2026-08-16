//! Operations on paths stored relative to a configured root.

use std::path::{Component, Path, PathBuf};

use crate::text::{eq_ignore_case, starts_with_ignore_case};

/// Joins a root with a forward-slash relative path.
pub fn join(root: &Path, relative_path: &str) -> PathBuf {
    let mut joined = PathBuf::from(root);
    for segment in relative_path.split(['/', '\\']).filter(|s| !s.is_empty()) {
        joined.push(segment);
    }
    joined
}

/// Whether two folder paths identify overlapping trees on a Windows filesystem: the same
/// location, or one nested beneath the other, in either direction.
///
/// This is the path relation behind every task shape convention — a destination inside its
/// source turns the app's own output into input, and a source inside a destination makes
/// Mirror treat the live source as orphaned content and delete it. Blank or unresolvable
/// input (a partially typed UNC prefix, say) overlaps nothing: callers evaluate user input as
/// it is typed, and a path with no filesystem location has nothing to relate to.
pub fn overlaps(first: Option<&Path>, second: Option<&Path>) -> bool {
    let (Some(first), Some(second)) = (
        first.and_then(try_normalize_full_path),
        second.and_then(try_normalize_full_path),
    ) else {
        return false;
    };

    eq_ignore_case(&first, &second)
        || starts_with_ignore_case(&first, &with_trailing_separator(&second))
        || starts_with_ignore_case(&second, &with_trailing_separator(&first))
}

/// [`overlaps`] for paths that are already known to be present.
pub fn paths_overlap(first: &Path, second: &Path) -> bool {
    overlaps(Some(first), Some(second))
}

fn with_trailing_separator(path: &str) -> String {
    if path.ends_with(['/', '\\']) {
        path.to_owned()
    } else {
        format!("{path}{}", std::path::MAIN_SEPARATOR)
    }
}

/// Resolves `path` to an absolute, separator-normalized form without touching the filesystem,
/// or `None` when it has no filesystem location at all.
fn try_normalize_full_path(path: &Path) -> Option<String> {
    if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
        return None;
    }

    // A half-typed UNC prefix (`\\`, `\\server`) names no location until it has both a server
    // and a share — .NET's GetFullPath rejects it outright, and so do we.
    if is_incomplete_unc(path) {
        return None;
    }

    let absolute = std::path::absolute(path).ok()?;
    let normalized = absolute.to_string_lossy().replace('/', "\\");
    Some(trim_end_separator(&normalized).to_owned())
}

fn is_incomplete_unc(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('/', "\\");
    let Some(rest) = text.strip_prefix(r"\\") else {
        return false;
    };
    // Needs a server *and* a share: `\\server\share`.
    rest.split('\\')
        .filter(|segment| !segment.is_empty())
        .count()
        < 2
}

fn trim_end_separator(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    // A root like `C:\` or `\\server\share\` is all separator past the prefix; keep it whole
    // rather than turning it into something that is no longer a path.
    if trimmed.is_empty() || is_bare_prefix(trimmed) {
        path
    } else {
        trimmed
    }
}

fn is_bare_prefix(path: &str) -> bool {
    matches!(
        Path::new(path).components().next(),
        Some(Component::Prefix(_))
    ) && Path::new(path).components().count() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> &Path {
        Path::new(path)
    }

    #[test]
    fn join_appends_a_forward_slash_relative_path() {
        assert_eq!(
            PathBuf::from(r"C:\src\a\b.txt"),
            join(p(r"C:\src"), "a/b.txt")
        );
        assert_eq!(
            PathBuf::from(r"C:\src\a\b.txt"),
            join(p(r"C:\src\"), "/a/b.txt")
        );
        assert_eq!(PathBuf::from(r"C:\src"), join(p(r"C:\src"), ""));
    }

    #[test]
    fn a_path_overlaps_itself_regardless_of_case_or_trailing_separator() {
        assert!(paths_overlap(p(r"C:\Photos"), p(r"c:\photos")));
        assert!(paths_overlap(p(r"C:\Photos"), p(r"C:\Photos\")));
        assert!(paths_overlap(p(r"C:\Photos"), p(r"C:/Photos")));
    }

    #[test]
    fn nesting_overlaps_in_either_direction() {
        assert!(paths_overlap(p(r"C:\Photos"), p(r"C:\Photos\Backup")));
        assert!(paths_overlap(p(r"C:\Photos\Backup"), p(r"C:\Photos")));
    }

    #[test]
    fn siblings_under_a_common_parent_do_not_overlap() {
        assert!(!paths_overlap(p(r"C:\Photos"), p(r"C:\Videos")));
        assert!(
            !paths_overlap(p(r"C:\Photos"), p(r"C:\Photos2")),
            "a longer sibling name is not nesting — the separator is what makes it nesting"
        );
    }

    #[test]
    fn unresolvable_input_overlaps_nothing() {
        // Callers probe these while the user is still typing.
        for partial in ["", "   ", r"\\", r"\\server"] {
            assert!(
                !paths_overlap(p(partial), p(r"C:\Photos")),
                "partial {partial:?}"
            );
            assert!(
                !paths_overlap(p(r"C:\Photos"), p(partial)),
                "partial {partial:?}"
            );
        }
        assert!(!overlaps(None, Some(p(r"C:\Photos"))));
        assert!(!overlaps(Some(p(r"C:\Photos")), None));
    }

    #[test]
    fn a_complete_unc_path_resolves_and_relates() {
        assert!(paths_overlap(
            p(r"\\server\share"),
            p(r"\\server\share\sub")
        ));
        assert!(!paths_overlap(p(r"\\server\share"), p(r"\\server\other")));
    }
}
