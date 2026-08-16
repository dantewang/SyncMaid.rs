//! File filters: a closed, JSON-discriminated rule hierarchy plus its matching algorithms.
//!
//! Every rule matches against a **relative path** — forward-slash separated, root-exclusive,
//! as produced by [`crate::io::TreeListing`] — and every comparison is case-insensitive,
//! because the only supported source and destination filesystems are Windows ones.
//!
//! Three rules deliberately match *nothing* when they are empty: a blank path prefix, a blank
//! extension, and a blank wildcard pattern. Selecting everything is [`FilterRule::AllFiles`]'s
//! job, and an empty `AllOf`/`AnyOf` is a half-built rule, not a wildcard.

use serde::{Deserialize, Serialize};

use crate::text::{chars_equal_ignore_case, ends_with_ignore_case, eq_ignore_case};

/// One filter rule. Persisted with a `"kind"` discriminator; the hierarchy is closed so the
/// JSON shape is a fixed, reviewable contract rather than whatever reflection finds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum FilterRule {
    /// Always matches. The only filter a Mirror destination is allowed to carry.
    #[serde(rename = "all")]
    AllFiles,
    /// Matches a directory prefix relative to the source root, e.g. `photos/2024`.
    #[serde(rename = "path")]
    Path(PathFilter),
    /// Matches a file extension anywhere in the tree, e.g. `jpg`.
    #[serde(rename = "extension")]
    Extension(ExtensionFilter),
    /// Matches a glob against the whole relative path, e.g. `**/ChatGPT*.png`.
    #[serde(rename = "wildcard")]
    Wildcard(WildcardFilter),
    /// Matches when every nested rule matches. Empty matches nothing.
    #[serde(rename = "allOf")]
    AllOf(CompositeFilter),
    /// Matches when any nested rule matches. Empty matches nothing.
    #[serde(rename = "anyOf")]
    AnyOf(CompositeFilter),
    /// Inverts the nested rule.
    #[serde(rename = "not")]
    Not(NotFilter),
}

impl FilterRule {
    /// A path-prefix rule. The prefix is normalized (back-slashes folded, ends trimmed).
    pub fn path(prefix: impl AsRef<str>) -> Self {
        Self::Path(PathFilter::new(prefix))
    }

    /// An extension rule. Leading `*` and `.` are stripped, so `*.jpg`, `.jpg` and `jpg` agree.
    pub fn extension(extension: impl AsRef<str>) -> Self {
        Self::Extension(ExtensionFilter::new(extension))
    }

    /// A wildcard rule. `**` spans folder levels, `*` and `?` never cross one.
    pub fn wildcard(pattern: impl AsRef<str>) -> Self {
        Self::Wildcard(WildcardFilter::new(pattern))
    }

    /// Matches when every nested rule matches; an empty list matches nothing.
    pub fn all_of(rules: impl IntoIterator<Item = FilterRule>) -> Self {
        Self::AllOf(CompositeFilter::new(rules))
    }

    /// Matches when any nested rule matches; an empty list matches nothing.
    pub fn any_of(rules: impl IntoIterator<Item = FilterRule>) -> Self {
        Self::AnyOf(CompositeFilter::new(rules))
    }

    /// Inverts `rule`. Named for the `"not"` discriminator it persists as, not for `ops::Not`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(rule: FilterRule) -> Self {
        Self::Not(NotFilter::new(rule))
    }

    /// Does this rule select `relative_path`?
    pub fn matches(&self, relative_path: &str) -> bool {
        match self {
            Self::AllFiles => true,
            Self::Path(f) => f.matches(relative_path),
            Self::Extension(f) => f.matches(relative_path),
            Self::Wildcard(f) => f.matches(relative_path),
            Self::AllOf(f) => {
                !f.rules.is_empty() && f.rules.iter().all(|r| r.matches(relative_path))
            }
            Self::AnyOf(f) => f.rules.iter().any(|r| r.matches(relative_path)),
            Self::Not(f) => !f.rule.matches(relative_path),
        }
    }

    /// True for the lone all-files rule a Mirror destination must carry.
    pub fn is_all_files(&self) -> bool {
        matches!(self, Self::AllFiles)
    }
}

// ---------------------------------------------------------------------------------------------
// Leaf rules. Their payloads are private so a rule can never exist in an un-normalized state —
// normalization runs both in the constructor and on the way in from JSON.
// ---------------------------------------------------------------------------------------------

/// A directory prefix relative to the source root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "PathRepr", into = "PathRepr")]
pub struct PathFilter {
    prefix: String,
}

#[derive(Serialize, Deserialize)]
struct PathRepr {
    #[serde(rename = "Prefix")]
    prefix: String,
}

impl PathFilter {
    pub fn new(prefix: impl AsRef<str>) -> Self {
        Self {
            prefix: prefix
                .as_ref()
                .replace('\\', "/")
                .trim_matches('/')
                .to_owned(),
        }
    }

    /// The normalized prefix, e.g. `photos/2024`.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    fn matches(&self, relative_path: &str) -> bool {
        if self.prefix.is_empty() {
            // A prefix of "" would select the whole tree. Selecting everything is
            // `AllFiles`'s job, so a blank prefix is a half-built rule and matches nothing.
            return false;
        }

        let path = relative_path.trim_start_matches(is_separator);
        let mut chars = path.chars();
        for expected in self.prefix.chars() {
            match chars.next() {
                Some(actual) if path_chars_equal(actual, expected) => {}
                _ => return false,
            }
        }

        // Either the path *is* the prefix, or the prefix is a whole leading directory of it —
        // `photos/2024` must not match `photos/2024b/x.jpg`.
        match chars.next() {
            None => true,
            Some(next) => is_separator(next),
        }
    }
}

impl From<PathRepr> for PathFilter {
    fn from(repr: PathRepr) -> Self {
        Self::new(repr.prefix)
    }
}

impl From<PathFilter> for PathRepr {
    fn from(filter: PathFilter) -> Self {
        Self {
            prefix: filter.prefix,
        }
    }
}

/// A file extension, matched anywhere in the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ExtensionRepr", into = "ExtensionRepr")]
pub struct ExtensionFilter {
    extension: String,
    /// `.jpg` — precomputed so matching never allocates.
    suffix: String,
}

#[derive(Serialize, Deserialize)]
struct ExtensionRepr {
    #[serde(rename = "Extension")]
    extension: String,
}

impl ExtensionFilter {
    pub fn new(extension: impl AsRef<str>) -> Self {
        let extension = extension
            .as_ref()
            .trim_start_matches('*')
            .trim_start_matches('.')
            .to_owned();
        let suffix = format!(".{extension}");
        Self { extension, suffix }
    }

    /// The normalized extension, without a leading dot, e.g. `jpg`.
    pub fn extension(&self) -> &str {
        &self.extension
    }

    fn matches(&self, relative_path: &str) -> bool {
        if self.extension.is_empty() {
            return false;
        }
        ends_with_ignore_case(relative_path, &self.suffix)
    }
}

impl From<ExtensionRepr> for ExtensionFilter {
    fn from(repr: ExtensionRepr) -> Self {
        Self::new(repr.extension)
    }
}

impl From<ExtensionFilter> for ExtensionRepr {
    fn from(filter: ExtensionFilter) -> Self {
        Self {
            extension: filter.extension,
        }
    }
}

/// A glob matched against the whole relative path.
///
/// Equality compares the pattern only: the parsed segments are a cache, and letting them into
/// the comparison would make two rules with the same pattern compare unequal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "WildcardRepr", into = "WildcardRepr")]
pub struct WildcardFilter {
    pattern: String,
    segments: Vec<Segment>,
}

#[derive(Serialize, Deserialize)]
struct WildcardRepr {
    #[serde(rename = "Pattern")]
    pattern: String,
}

#[derive(Debug, Clone)]
enum Segment {
    /// `**` — spans any number of path segments, including none.
    AnyDepth,
    /// A segment with no wildcards; compared case-insensitively.
    Literal(String),
    /// A segment containing `*` or `?`.
    Glob(Vec<char>),
}

impl PartialEq for WildcardFilter {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for WildcardFilter {}

impl WildcardFilter {
    pub fn new(pattern: impl AsRef<str>) -> Self {
        let pattern = pattern
            .as_ref()
            .replace('\\', "/")
            .trim_matches('/')
            .to_owned();
        let segments = pattern
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s == "**" {
                    Segment::AnyDepth
                } else if s.contains(['*', '?']) {
                    Segment::Glob(s.chars().collect())
                } else {
                    Segment::Literal(s.to_owned())
                }
            })
            .collect();
        Self { pattern, segments }
    }

    /// The normalized pattern as the user typed it, e.g. `**/ChatGPT*.png`.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    fn matches(&self, relative_path: &str) -> bool {
        if self.segments.is_empty() {
            return false;
        }

        let path: Vec<&str> = relative_path
            .split(is_separator)
            .filter(|s| !s.is_empty())
            .collect();

        match_segments(&self.segments, &path)
    }
}

impl From<WildcardRepr> for WildcardFilter {
    fn from(repr: WildcardRepr) -> Self {
        Self::new(repr.pattern)
    }
}

impl From<WildcardFilter> for WildcardRepr {
    fn from(filter: WildcardFilter) -> Self {
        Self {
            pattern: filter.pattern,
        }
    }
}

/// The payload shared by `AllOf` and `AnyOf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositeFilter {
    #[serde(rename = "Rules")]
    pub rules: Vec<FilterRule>,
}

impl CompositeFilter {
    pub fn new(rules: impl IntoIterator<Item = FilterRule>) -> Self {
        Self {
            rules: rules.into_iter().collect(),
        }
    }
}

/// The payload of `Not`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotFilter {
    #[serde(rename = "Rule")]
    pub rule: Box<FilterRule>,
}

impl NotFilter {
    pub fn new(rule: FilterRule) -> Self {
        Self {
            rule: Box::new(rule),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Matching primitives
// ---------------------------------------------------------------------------------------------

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Like [`chars_equal_ignore_case`], but `/` and `\` are the same character.
fn path_chars_equal(a: char, b: char) -> bool {
    if is_separator(a) && is_separator(b) {
        return true;
    }
    chars_equal_ignore_case(a, b)
}

/// Segment-level glob with a single backtrack point — the same shape as [`match_glob`] one
/// level up. `**` is the only construct that can span segments.
fn match_segments(pattern: &[Segment], path: &[&str]) -> bool {
    let mut p = 0usize;
    let mut s = 0usize;
    let mut star: Option<(usize, usize)> = None;

    while s < path.len() {
        match pattern.get(p) {
            Some(Segment::AnyDepth) => {
                // Provisionally let `**` span nothing; the backtrack below widens it.
                star = Some((p, s));
                p += 1;
            }
            Some(seg) if match_segment(seg, path[s]) => {
                p += 1;
                s += 1;
            }
            _ => match star {
                Some((star_p, star_s)) => {
                    star = Some((star_p, star_s + 1));
                    p = star_p + 1;
                    s = star_s + 1;
                }
                None => return false,
            },
        }
    }

    // Trailing `**` consume nothing, which is what makes `a/**` match `a` itself.
    while matches!(pattern.get(p), Some(Segment::AnyDepth)) {
        p += 1;
    }
    p == pattern.len()
}

fn match_segment(pattern: &Segment, segment: &str) -> bool {
    match pattern {
        Segment::AnyDepth => true,
        Segment::Literal(literal) => eq_ignore_case(literal, segment),
        Segment::Glob(glob) => match_glob(glob, segment),
    }
}

/// Char-level glob within one path segment: `*` matches any run, `?` exactly one char, and
/// neither ever crosses a separator (they can't — the segment has none). One backtrack point
/// is enough because only `*` is variable-width.
fn match_glob(pattern: &[char], segment: &str) -> bool {
    let mut p = 0usize;
    let mut cursor = segment;
    let mut star: Option<(usize, &str)> = None;

    while let Some(c) = cursor.chars().next() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, cursor));
                p += 1;
            }
            Some('?') => {
                p += 1;
                cursor = &cursor[c.len_utf8()..];
            }
            Some(&expected) if chars_equal_ignore_case(expected, c) => {
                p += 1;
                cursor = &cursor[c.len_utf8()..];
            }
            _ => match star {
                Some((star_p, star_cursor)) => {
                    // Widen the `*` by one char and retry everything after it.
                    let widened =
                        &star_cursor[star_cursor.chars().next().map_or(0, char::len_utf8)..];
                    star = Some((star_p, widened));
                    p = star_p + 1;
                    cursor = widened;
                }
                None => return false,
            },
        }
    }

    while matches!(pattern.get(p), Some('*')) {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Path ---------------------------------------------------------------------------------

    #[test]
    fn path_filter_matches_the_prefix_itself_and_everything_under_it() {
        let rule = FilterRule::path("photos/2024");
        assert!(rule.matches("photos/2024"));
        assert!(rule.matches("photos/2024/img.jpg"));
        assert!(rule.matches("photos/2024/raw/img.dng"));
    }

    #[test]
    fn path_filter_does_not_match_a_sibling_with_a_longer_name() {
        let rule = FilterRule::path("photos/2024");
        assert!(!rule.matches("photos/2024b/img.jpg"));
        assert!(!rule.matches("photos/20"));
    }

    #[test]
    fn path_filter_is_separator_and_case_insensitive() {
        let rule = FilterRule::path("\\Photos\\2024\\");
        assert_eq!("Photos/2024", rule_prefix(&rule));
        assert!(rule.matches("photos/2024/img.jpg"));
        assert!(rule.matches("PHOTOS\\2024\\img.jpg"));
        assert!(rule.matches("/photos/2024/img.jpg"));
    }

    #[test]
    fn empty_or_slash_only_path_filter_matches_nothing() {
        for prefix in ["", "/", "///", "\\"] {
            let rule = FilterRule::path(prefix);
            assert!(!rule.matches("anything.txt"), "prefix {prefix:?}");
            assert!(!rule.matches(""), "prefix {prefix:?}");
        }
    }

    // -- Extension ----------------------------------------------------------------------------

    #[test]
    fn extension_filter_normalizes_star_and_dot() {
        for written in ["jpg", ".jpg", "*.jpg"] {
            let rule = FilterRule::extension(written);
            assert!(rule.matches("photos/img.jpg"), "written as {written:?}");
        }
    }

    #[test]
    fn extension_filter_is_case_insensitive_and_matches_at_any_depth() {
        let rule = FilterRule::extension("JPG");
        assert!(rule.matches("img.jpg"));
        assert!(rule.matches("a/b/c/IMG.JpG"));
        assert!(!rule.matches("img.jpeg"));
        assert!(!rule.matches("jpg"));
    }

    #[test]
    fn empty_extension_filter_matches_nothing() {
        let rule = FilterRule::extension("*.");
        assert!(!rule.matches("img.jpg"));
        assert!(!rule.matches("img."));
    }

    // -- Wildcard -----------------------------------------------------------------------------

    #[test]
    fn wildcard_star_does_not_cross_a_separator() {
        let rule = FilterRule::wildcard("photos/*.raw");
        assert!(rule.matches("photos/a.raw"));
        assert!(!rule.matches("photos/2024/a.raw"));
    }

    #[test]
    fn wildcard_double_star_spans_any_number_of_segments() {
        let rule = FilterRule::wildcard("photos/**/*.raw");
        assert!(rule.matches("photos/a.raw"));
        assert!(rule.matches("photos/2024/a.raw"));
        assert!(rule.matches("photos/2024/05/a.raw"));
        assert!(!rule.matches("videos/a.raw"));
    }

    #[test]
    fn a_trailing_double_star_matches_the_directory_itself() {
        let rule = FilterRule::wildcard("photos/**");
        assert!(rule.matches("photos"));
        assert!(rule.matches("photos/a.raw"));
        assert!(rule.matches("photos/2024/a.raw"));
    }

    #[test]
    fn a_leading_double_star_is_what_makes_a_pattern_apply_at_any_depth() {
        let rule = FilterRule::wildcard("**/ChatGPT*.png");
        assert!(rule.matches("ChatGPT Image 1.png"));
        assert!(rule.matches("downloads/2026/ChatGPT-x.png"));
        assert!(!rule.matches("downloads/Claude.png"));
    }

    #[test]
    fn wildcard_question_mark_matches_exactly_one_char() {
        let rule = FilterRule::wildcard("IMG_????.jpg");
        assert!(rule.matches("IMG_0042.jpg"));
        assert!(!rule.matches("IMG_042.jpg"));
        assert!(!rule.matches("IMG_00042.jpg"));
    }

    #[test]
    fn wildcard_is_case_and_separator_insensitive() {
        let rule = FilterRule::wildcard("\\Photos\\**\\*.RAW");
        assert!(rule.matches("photos/2024/a.raw"));
        assert!(rule.matches("PHOTOS\\2024\\A.Raw"));
    }

    #[test]
    fn empty_wildcard_matches_nothing() {
        for pattern in ["", "/", "\\\\"] {
            assert!(
                !FilterRule::wildcard(pattern).matches("a.txt"),
                "pattern {pattern:?}"
            );
        }
    }

    #[test]
    fn wildcard_backtracks_when_the_first_star_expansion_fails() {
        let rule = FilterRule::wildcard("*a*b.txt");
        assert!(rule.matches("xaybzb.txt"));
        assert!(!rule.matches("xayz.txt"));
    }

    #[test]
    fn wildcard_compares_by_pattern() {
        assert_eq!(
            FilterRule::wildcard("a/**/b"),
            FilterRule::wildcard("a/**/b")
        );
        assert_ne!(
            FilterRule::wildcard("a/**/b"),
            FilterRule::wildcard("a/**/c")
        );
    }

    // -- Composites ---------------------------------------------------------------------------

    #[test]
    fn all_of_requires_every_rule() {
        let rule = FilterRule::all_of([FilterRule::path("docs"), FilterRule::extension("pdf")]);
        assert!(rule.matches("docs/a.pdf"));
        assert!(!rule.matches("docs/a.txt"));
        assert!(!rule.matches("photos/a.pdf"));
    }

    #[test]
    fn any_of_requires_one_rule() {
        let rule = FilterRule::any_of([FilterRule::extension("jpg"), FilterRule::extension("png")]);
        assert!(rule.matches("a.jpg"));
        assert!(rule.matches("a.png"));
        assert!(!rule.matches("a.gif"));
    }

    #[test]
    fn empty_composites_match_nothing() {
        assert!(!FilterRule::all_of([]).matches("a.txt"));
        assert!(!FilterRule::any_of([]).matches("a.txt"));
    }

    #[test]
    fn not_inverts_and_double_negation_cancels_out() {
        let inner = FilterRule::extension("tmp");
        assert!(FilterRule::not(inner.clone()).matches("a.txt"));
        assert!(!FilterRule::not(inner.clone()).matches("a.tmp"));
        assert!(FilterRule::not(FilterRule::not(inner)).matches("a.tmp"));
    }

    #[test]
    fn composites_compare_by_value() {
        let a = FilterRule::all_of([FilterRule::path("docs"), FilterRule::extension("pdf")]);
        let b = FilterRule::all_of([FilterRule::path("docs"), FilterRule::extension("pdf")]);
        let c = FilterRule::all_of([FilterRule::extension("pdf"), FilterRule::path("docs")]);
        assert_eq!(a, b);
        assert_ne!(
            a, c,
            "order is part of the value, as it is in the C# SequenceEqual"
        );
    }

    // -- Persisted shape ----------------------------------------------------------------------

    #[test]
    fn rules_round_trip_through_the_legacy_json_shape() {
        let cases = [
            (FilterRule::AllFiles, r#"{"kind":"all"}"#),
            (
                FilterRule::path("photos/2024"),
                r#"{"kind":"path","Prefix":"photos/2024"}"#,
            ),
            (
                FilterRule::extension("jpg"),
                r#"{"kind":"extension","Extension":"jpg"}"#,
            ),
            (
                FilterRule::wildcard("**/a*.png"),
                r#"{"kind":"wildcard","Pattern":"**/a*.png"}"#,
            ),
            (
                FilterRule::not(FilterRule::extension("tmp")),
                r#"{"kind":"not","Rule":{"kind":"extension","Extension":"tmp"}}"#,
            ),
            (
                FilterRule::any_of([FilterRule::extension("jpg")]),
                r#"{"kind":"anyOf","Rules":[{"kind":"extension","Extension":"jpg"}]}"#,
            ),
            (
                FilterRule::all_of([FilterRule::path("docs")]),
                r#"{"kind":"allOf","Rules":[{"kind":"path","Prefix":"docs"}]}"#,
            ),
        ];

        for (rule, json) in cases {
            assert_eq!(json, serde_json::to_string(&rule).unwrap());
            assert_eq!(rule, serde_json::from_str::<FilterRule>(json).unwrap());
        }
    }

    #[test]
    fn hand_edited_rules_are_normalized_on_load() {
        let rule: FilterRule =
            serde_json::from_str(r#"{"kind":"path","Prefix":"\\Photos\\2024\\"}"#).unwrap();
        // Separators are folded and the ends trimmed, but the user's casing is preserved —
        // only *matching* ignores case, exactly as the C# record does.
        assert_eq!("Photos/2024", rule_prefix(&rule));
        assert!(rule.matches("photos/2024/a.jpg"));

        let rule: FilterRule =
            serde_json::from_str(r#"{"kind":"extension","Extension":"*.JPG"}"#).unwrap();
        assert!(rule.matches("a.jpg"));
    }

    fn rule_prefix(rule: &FilterRule) -> &str {
        match rule {
            FilterRule::Path(f) => f.prefix(),
            other => panic!("not a path filter: {other:?}"),
        }
    }
}
