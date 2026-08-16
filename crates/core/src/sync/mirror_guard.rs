//! The two guards standing between Mirror and a wiped destination.
//!
//! Mirror deletes whatever the source no longer has, which is exactly right when the source is
//! the truth and catastrophic when it briefly is not. Both guards run before any deletion is
//! applied.

/// Below this many destination files the ratio guard says nothing: "more than half" of three
/// files is not evidence of anything.
pub const MIN_DESTINATION_FILES_FOR_RATIO_GUARD: usize = 10;

/// What the guard thinks of a planned set of deletions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorGuardVerdict {
    /// Go ahead.
    Allowed,
    /// The source is empty or unavailable, so every destination file looks orphaned.
    ///
    /// **Not overridable.** There is no version of "the drive is unplugged, delete the backup"
    /// that is not a mistake.
    EmptySource,
    /// A large fraction of the destination would go. Hold the run and ask, once, for this run
    /// only.
    NeedsConfirmation,
}

/// Weighs a planned set of deletions.
///
/// `mass_delete_threshold` is a fraction of the destination; 0 disables the ratio guard, and
/// the empty-source guard still applies. `override_mass_delete` is the user's one-shot answer
/// to a previous `NeedsConfirmation` — never persisted, never remembered past this run.
pub fn evaluate(
    delete_count: usize,
    destination_file_count: usize,
    source_is_empty: bool,
    mass_delete_threshold: f64,
    override_mass_delete: bool,
) -> MirrorGuardVerdict {
    if delete_count == 0 {
        return MirrorGuardVerdict::Allowed;
    }

    if source_is_empty {
        return MirrorGuardVerdict::EmptySource;
    }

    if !override_mass_delete
        && mass_delete_threshold > 0.0
        && destination_file_count >= MIN_DESTINATION_FILES_FOR_RATIO_GUARD
        && delete_count as f64 >= destination_file_count as f64 * mass_delete_threshold
    {
        return MirrorGuardVerdict::NeedsConfirmation;
    }

    MirrorGuardVerdict::Allowed
}

/// What the confirmation window shows: how many files would go, and a sample of which.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirrorDeletePreview {
    pub count: usize,
    pub sample: Vec<String>,
}

impl MirrorDeletePreview {
    /// How many paths the preview lists before it says "and N more".
    pub const SAMPLE_SIZE: usize = 25;

    pub fn new(count: usize, sample: Vec<String>) -> Self {
        Self { count, sample }
    }

    /// No preview available. The preview is advisory — anything that goes wrong while building
    /// it degrades to this rather than faulting the command the user just clicked.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_delete_is_always_allowed() {
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(0, 100, true, 0.5, false)
        );
    }

    #[test]
    fn an_empty_source_blocks_every_deletion_and_cannot_be_overridden() {
        assert_eq!(
            MirrorGuardVerdict::EmptySource,
            evaluate(1, 100, true, 0.5, false)
        );
        assert_eq!(
            MirrorGuardVerdict::EmptySource,
            evaluate(1, 100, true, 0.5, true),
            "there is no version of this that is not a mistake"
        );
        assert_eq!(
            MirrorGuardVerdict::EmptySource,
            evaluate(1, 100, true, 0.0, false),
            "turning off the ratio guard does not turn off this one"
        );
    }

    #[test]
    fn a_mass_deletion_asks_first() {
        assert_eq!(
            MirrorGuardVerdict::NeedsConfirmation,
            evaluate(50, 100, false, 0.5, false)
        );
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(49, 100, false, 0.5, false)
        );
    }

    #[test]
    fn a_confirmed_mass_deletion_goes_ahead() {
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(100, 100, false, 0.5, true)
        );
    }

    #[test]
    fn a_zero_threshold_turns_the_ratio_guard_off() {
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(100, 100, false, 0.0, false)
        );
    }

    #[test]
    fn a_nearly_empty_destination_is_not_evidence_of_anything() {
        // Deleting "more than half" of three files is a normal edit, not a catastrophe.
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(3, 3, false, 0.5, false)
        );
        assert_eq!(
            MirrorGuardVerdict::Allowed,
            evaluate(9, 9, false, 0.5, false),
            "one below the floor"
        );
        assert_eq!(
            MirrorGuardVerdict::NeedsConfirmation,
            evaluate(10, 10, false, 0.5, false)
        );
    }
}
