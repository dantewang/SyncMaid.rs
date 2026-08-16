//! Cron schedules, in local wall-clock time.
//!
//! Five fields — minute, hour, day-of-month, month, day-of-week — interpreted the way a system
//! cron does, so `0 2 * * 1` means "02:00 every Monday" as the user reads it off their own
//! clock, DST included.

use std::str::FromStr;

use chrono::{DateTime, Local};
use croner::Cron;

/// A parsed five-field cron expression.
#[derive(Debug, Clone)]
pub struct CronSchedule {
    cron: Cron,
    expression: String,
}

impl CronSchedule {
    /// Parses `expression`, or fails if it is not a usable cron pattern.
    pub fn parse(expression: &str) -> Result<Self, CronParseError> {
        let trimmed = expression.trim();
        if trimmed.is_empty() {
            return Err(CronParseError("a cron expression cannot be blank".into()));
        }
        let cron = Cron::from_str(trimmed).map_err(|error| CronParseError(error.to_string()))?;
        Ok(Self {
            cron,
            expression: trimmed.to_owned(),
        })
    }

    /// Whether `expression` parses. The editor refuses to save one that does not.
    pub fn is_valid(expression: &str) -> bool {
        Self::parse(expression).is_ok()
    }

    /// The expression as the user typed it.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The next time this fires strictly after `after`, in local time.
    ///
    /// `None` when the pattern is valid but has no future occurrence at all — `0 0 30 2 *`
    /// names the 30th of February. The editor warns rather than pretending it will run.
    pub fn next_occurrence_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        self.cron.find_next_occurrence(&after, false).ok()
    }

    /// The next occurrence from now.
    pub fn next_occurrence(&self) -> Option<DateTime<Local>> {
        self.next_occurrence_after(Local::now())
    }
}

/// The expression is not a usable cron pattern.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CronParseError(pub String);

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeZone, Timelike};

    use super::*;

    fn local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .expect("a real instant")
    }

    #[test]
    fn the_documented_examples_all_parse() {
        for expression in [
            "*/5 * * * *",
            "0 * * * *",
            "0 2 * * *",
            "0 2 * * 1",
            "30 22 1 * *",
        ] {
            assert!(CronSchedule::is_valid(expression), "{expression}");
        }
    }

    #[test]
    fn a_malformed_expression_is_refused() {
        for expression in ["", "   ", "not cron", "0 2 * *", "99 * * * *"] {
            assert!(
                !CronSchedule::is_valid(expression),
                "{expression:?} should not parse"
            );
        }
    }

    #[test]
    fn the_next_occurrence_is_strictly_after_the_given_time() {
        let schedule = CronSchedule::parse("0 2 * * *").unwrap();
        let at_two = local(2026, 8, 9, 2, 0);

        let next = schedule.next_occurrence_after(at_two).unwrap();

        assert!(
            next > at_two,
            "a schedule standing on its own occurrence moves to the next one"
        );
        assert_eq!(10, next.day());
        assert_eq!(2, next.hour());
    }

    #[test]
    fn a_weekday_pattern_uses_posix_numbering() {
        // 1 is Monday, the numbering system cron uses — and the one Cronos used.
        let schedule = CronSchedule::parse("0 2 * * 1").unwrap();

        let next = schedule
            .next_occurrence_after(local(2026, 8, 9, 3, 0))
            .unwrap();

        assert_eq!(chrono::Weekday::Mon, next.weekday());
    }

    #[test]
    fn a_pattern_with_no_future_occurrence_reports_none_rather_than_pretending() {
        // The 30th of February never comes.
        let schedule = CronSchedule::parse("0 0 30 2 *").unwrap();

        assert_eq!(
            None,
            schedule.next_occurrence_after(local(2026, 8, 9, 2, 0))
        );
    }

    #[test]
    fn every_five_minutes_lands_on_the_next_five_minute_boundary() {
        let schedule = CronSchedule::parse("*/5 * * * *").unwrap();

        let next = schedule
            .next_occurrence_after(local(2026, 8, 9, 2, 3))
            .unwrap();

        assert_eq!(5, next.minute());
        assert_eq!(2, next.hour());
    }

    #[test]
    fn the_schedule_walks_forward_one_occurrence_at_a_time() {
        let schedule = CronSchedule::parse("0 * * * *").unwrap();
        let mut when = local(2026, 8, 9, 2, 30);

        let mut hours = Vec::new();
        for _ in 0..3 {
            when = schedule.next_occurrence_after(when).unwrap();
            hours.push(when.hour());
        }

        assert_eq!(vec![3, 4, 5], hours);
    }

    #[test]
    fn the_expression_is_kept_as_typed_for_the_card_badge() {
        let schedule = CronSchedule::parse("  0 2 * * *  ").unwrap();
        assert_eq!("0 2 * * *", schedule.expression());
    }
}
