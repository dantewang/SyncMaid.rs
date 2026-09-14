//! Which language the window is speaking, and the three ways a string is used.
//!
//! The tables themselves are generated from `lang/*.json` — see `build/strings.rs`. What is
//! here is only the small runtime around them: which one is active, how a `{0}` slot is filled,
//! and which plural form a count wants.
//!
//! **Only the UI language switches.** Dates and numbers keep following the system's regional
//! settings, because those are a separate preference and the user has already expressed it.
//! And engine messages stay in English: the core carries no display strings, so what a failure
//! says is the OS's own words, wrapped in a sentence that *is* translated.

use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::strings::{Language, COUNT, LANGUAGES};

/// Index into [`LANGUAGES`]. English is index 0, and also the fallback.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Every language that ships, in the order the settings page offers them.
pub fn languages() -> &'static [Language] {
    &LANGUAGES
}

pub fn active() -> &'static Language {
    // Only ever set from `choose`, which clamps to a real index.
    &LANGUAGES[ACTIVE.load(Ordering::Relaxed)]
}

/// One string, in the active language.
pub fn text(key: usize) -> &'static str {
    debug_assert!(key < COUNT);
    active().strings[key]
}

/// Fills the `{0}`, `{1}` … slots of one string.
///
/// A slot with no argument is left as it was written rather than blanked: a visible `{1}` is a
/// bug report, and an empty gap is a mystery.
pub fn format(key: usize, arguments: &[&dyn Display]) -> String {
    substitute(text(key), arguments)
}

/// Picks the singular or plural wording and fills in the count.
///
/// Languages without a plural distinction simply carry the same text twice, which keeps every
/// call site the same shape whatever the language is.
pub fn plural(one: usize, other: usize, count: i64) -> String {
    let key = if count == 1 { one } else { other };
    substitute(text(key), &[&count])
}

fn substitute(template: &str, arguments: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let characters: Vec<char> = template.chars().collect();
    let mut index = 0;

    while index < characters.len() {
        if characters[index] == '{' {
            let mut end = index + 1;
            while end < characters.len() && characters[end].is_ascii_digit() {
                end += 1;
            }
            if end > index + 1 && characters.get(end) == Some(&'}') {
                let slot: usize = characters[index + 1..end]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .unwrap_or(usize::MAX);
                match arguments.get(slot) {
                    Some(argument) => {
                        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{argument}"));
                    }
                    None => out.extend(&characters[index..=end]),
                }
                index = end + 1;
                continue;
            }
        }
        out.push(characters[index]);
        index += 1;
    }
    out
}

/// Switches language. `None` means "whatever the system is set to".
///
/// Returns true when the active language actually changed, which is the caller's cue to redraw
/// every window — nothing is cached, so a redraw is all a switch takes.
pub fn set_language(tag: Option<&str>) -> bool {
    let chosen = choose(tag);
    ACTIVE.swap(chosen, Ordering::Relaxed) != chosen
}

/// The index of the best table for `tag`, or for the system when it is `None`.
fn choose(tag: Option<&str>) -> usize {
    let wanted = match tag {
        Some(tag) if !tag.is_empty() => tag.to_owned(),
        _ => match system_language() {
            Some(tag) => tag,
            None => return 0,
        },
    };
    best_match(&wanted).unwrap_or(0)
}

/// Matches a BCP-47 tag against what ships, from most specific to least.
fn best_match(wanted: &str) -> Option<usize> {
    let wanted = wanted.to_lowercase();

    if let Some(index) = LANGUAGES
        .iter()
        .position(|language| language.tag.to_lowercase() == wanted)
    {
        return Some(index);
    }

    // `zh-Hant-TW`, `zh-Hans-CN` — a region under a script we do ship.
    if let Some(index) = LANGUAGES
        .iter()
        .position(|language| wanted.starts_with(&format!("{}-", language.tag.to_lowercase())))
    {
        return Some(index);
    }

    // Chinese without a script: the region decides. Traditional is used in Taiwan, Hong Kong
    // and Macau; everywhere else defaults to Simplified.
    if wanted.starts_with("zh") {
        let traditional = ["hant", "tw", "hk", "mo"]
            .iter()
            .any(|marker| wanted.contains(marker));
        return tag_index(if traditional { "zh-Hant" } else { "zh-Hans" });
    }

    // `ja-JP` and friends: the primary subtag is enough.
    let primary = wanted.split('-').next().unwrap_or(&wanted);
    LANGUAGES
        .iter()
        .position(|language| language.tag.to_lowercase() == primary)
}

fn tag_index(tag: &str) -> Option<usize> {
    LANGUAGES.iter().position(|language| language.tag == tag)
}

/// What Windows says the user's language is.
pub fn system_language() -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

        // LOCALE_NAME_MAX_LENGTH. Not re-exported by windows-sys, and fixed by the Windows
        // SDK contract at 85 UTF-16 units including the terminator.
        let mut buffer = [0u16; 85];
        let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
        if length <= 1 {
            return None;
        }
        // The count includes the terminating null.
        Some(String::from_utf16_lossy(&buffer[..length as usize - 1]))
    }
    #[cfg(not(windows))]
    {
        std::env::var("LANG").ok().map(|value| {
            value
                .split(['.', '@'])
                .next()
                .unwrap_or_default()
                .replace('_', "-")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_language_is_reachable_by_its_own_tag() {
        for (index, language) in LANGUAGES.iter().enumerate() {
            assert_eq!(Some(index), best_match(language.tag), "{}", language.tag);
        }
    }

    #[test]
    fn a_region_falls_back_to_the_language_it_belongs_to() {
        assert_eq!(tag_index("ja"), best_match("ja-JP"));
        assert_eq!(tag_index("en"), best_match("en-GB"));
        assert_eq!(tag_index("zh-Hans"), best_match("zh-Hans-CN"));
        assert_eq!(tag_index("zh-Hant"), best_match("zh-Hant-TW"));
    }

    #[test]
    fn chinese_without_a_script_is_decided_by_the_region() {
        // The distinction the user actually feels; guessing wrong shows the wrong characters.
        assert_eq!(tag_index("zh-Hant"), best_match("zh-TW"));
        assert_eq!(tag_index("zh-Hant"), best_match("zh-HK"));
        assert_eq!(tag_index("zh-Hans"), best_match("zh-CN"));
        assert_eq!(tag_index("zh-Hans"), best_match("zh"));
    }

    #[test]
    fn a_language_that_does_not_ship_falls_back_to_english() {
        assert_eq!(None, best_match("de-DE"));
        assert_eq!(0, choose(Some("de-DE")));
        assert_eq!("en", LANGUAGES[0].tag, "English is the fallback");
    }

    #[test]
    fn slots_are_filled_in_order_and_an_unfilled_one_stays_visible() {
        assert_eq!("a-b", substitute("{0}-{1}", &[&"a", &"b"]));
        assert_eq!("b-a", substitute("{1}-{0}", &[&"a", &"b"]));
        assert_eq!(
            "a-{1}",
            substitute("{0}-{1}", &[&"a"]),
            "a visible slot is a bug report; an empty gap is a mystery"
        );
    }

    #[test]
    fn a_brace_that_is_not_a_slot_is_left_alone() {
        assert_eq!("{}", substitute("{}", &[]));
        assert_eq!("{x}", substitute("{x}", &[&"a"]));
        assert_eq!("100%", substitute("100%", &[]));
    }

    #[test]
    fn the_count_decides_which_wording_and_is_substituted_into_it() {
        use crate::strings::key;

        assert_eq!(
            "1 file",
            plural(
                key::COMMON_FILES_COUNT_ONE,
                key::COMMON_FILES_COUNT_OTHER,
                1
            )
        );
        assert_eq!(
            "0 files",
            plural(
                key::COMMON_FILES_COUNT_ONE,
                key::COMMON_FILES_COUNT_OTHER,
                0
            ),
            "zero takes the plural form in English, the way it does in speech"
        );
        assert_eq!(
            "12 files",
            plural(
                key::COMMON_FILES_COUNT_ONE,
                key::COMMON_FILES_COUNT_OTHER,
                12
            )
        );
    }
}
