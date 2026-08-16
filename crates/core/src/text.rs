//! Case-insensitive comparison shared by filters and path relations.
//!
//! Both source and destination are Windows filesystems, so every path comparison in the
//! engine ignores case. ASCII is the overwhelmingly common case and gets a fast path;
//! anything else falls back to a full-Unicode uppercase fold, which is what .NET's
//! `OrdinalIgnoreCase` amounts to for the inputs we see.

/// Case-insensitive char equality.
pub(crate) fn chars_equal_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_uppercase().eq(b.to_uppercase())
}

/// Case-insensitive string equality.
pub(crate) fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut b = b.chars();
    for ac in a.chars() {
        match b.next() {
            Some(bc) if chars_equal_ignore_case(ac, bc) => {}
            _ => return false,
        }
    }
    b.next().is_none()
}

/// Case-insensitive prefix test.
pub(crate) fn starts_with_ignore_case(haystack: &str, prefix: &str) -> bool {
    let mut h = haystack.chars();
    for pc in prefix.chars() {
        match h.next() {
            Some(c) if chars_equal_ignore_case(c, pc) => {}
            _ => return false,
        }
    }
    true
}

/// Case-insensitive suffix test.
pub(crate) fn ends_with_ignore_case(haystack: &str, suffix: &str) -> bool {
    let mut h = haystack.chars().rev();
    for sc in suffix.chars().rev() {
        match h.next() {
            Some(c) if chars_equal_ignore_case(c, sc) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparisons_ignore_case_including_beyond_ascii() {
        assert!(eq_ignore_case("Photos", "photos"));
        assert!(eq_ignore_case("Ärger", "ärger"));
        assert!(!eq_ignore_case("photos", "photo"));
        assert!(starts_with_ignore_case("Photos/2024", "photos/"));
        assert!(!starts_with_ignore_case("Photos", "photos/2024"));
        assert!(ends_with_ignore_case("a/B.JPG", ".jpg"));
        assert!(!ends_with_ignore_case("a.jpg", "x.jpg"));
    }

    #[test]
    fn an_empty_needle_matches_anything() {
        assert!(starts_with_ignore_case("a", ""));
        assert!(ends_with_ignore_case("a", ""));
        assert!(eq_ignore_case("", ""));
    }
}
