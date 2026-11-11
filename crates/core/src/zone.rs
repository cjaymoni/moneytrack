//! Resolving a time zone, without carrying a time-zone database.
//!
//! This crate stores an IANA zone name on a recurring rule and otherwise refuses to know
//! what one means: shipping tzdata into a WASM bundle to answer "when is midnight in Accra"
//! is not a trade worth making, and the platform already has the answer. So the caller
//! implements [`Zone`] against whatever its own system offers, and the rules here ask it.
//!
//! ## Why two lookups and not one
//!
//! A zone is a function from an instant to an offset. Going the other way — from a local
//! date to the offset in force on it — is a different question with a different answer, and
//! on the day a zone shifts the two do not agree.
//!
//! Both directions are genuinely needed. Reading the local date a transaction falls on
//! starts from an instant; working out when a bill lands starts from a local date. Asking
//! one question with the signature of the other is how a purchase ends up counted against
//! the wrong month, so the two are separate methods and the compiler can tell them apart.
//!
//! A zone that never shifts — Ghana's among them — answers both with the same constant, and
//! the blanket implementation for [`UtcOffset`] does exactly that.

use time::{Date, Duration, OffsetDateTime, Time, UtcOffset};

/// What the rules in this crate need to know about the user's time zone.
///
/// Implement it over the platform's own zone database. For a fixed-offset zone, use a
/// [`UtcOffset`] directly — it implements this trait.
pub trait Zone {
    /// The offset in force at an instant.
    ///
    /// This is the question a zone database answers natively, and it is never ambiguous.
    fn offset_for_instant(&self, at: OffsetDateTime) -> UtcOffset;

    /// The offset in force on a local date, at the start of that date.
    ///
    /// On the day a zone shifts, a date covers more than one offset; this one is asked for
    /// the first of them. Two local dates a year have no single answer, and an implementor
    /// should not agonise over them — [`Zone::start_of_day`] re-resolves against the instant
    /// and corrects whatever this returns:
    ///
    /// - **Midnight skipped.** The clocks jump from 23:59 to 01:00, so 00:00 never happens.
    ///   Return either neighbouring offset.
    /// - **Midnight repeated.** The clocks fall back through 00:00, so it happens twice.
    ///   Return either; the later one is used.
    fn offset_for_date(&self, date: Date) -> UtcOffset;

    /// The local date an instant falls on.
    fn local_date(&self, at: OffsetDateTime) -> Date {
        at.to_offset(self.offset_for_instant(at)).date()
    }

    /// The instant a local date begins.
    ///
    /// Guarantees `zone.local_date(zone.start_of_day(d)) == d`. That round trip is the one
    /// property the rest of the crate leans on, and it is not free: [`recurrence`] builds an
    /// instant from a local date and [`budget`] reads a local date back out of it, so a zone
    /// where the two disagree puts a generated bill in the wrong month.
    ///
    /// Taking the offset from the date alone is what breaks it. On the day a zone shifts,
    /// the offset in force at local midnight may not be the one that produces local
    /// midnight, so this resolves from the date, re-resolves against the instant that
    /// produced, and keeps the answer that reads back as the date asked for.
    ///
    /// [`recurrence`]: crate::recurrence
    /// [`budget`]: crate::budget
    fn start_of_day(&self, date: Date) -> OffsetDateTime {
        let naive = date.with_time(Time::MIDNIGHT);
        let from_date = naive.assume_offset(self.offset_for_date(date));
        let from_instant = naive.assume_offset(self.offset_for_instant(from_date));
        if self.local_date(from_instant) == date {
            return from_instant;
        }
        if self.local_date(from_date) == date {
            return from_date;
        }
        // Neither reads back: the clocks jumped over midnight, so this date has no 00:00 at
        // all. Its first instant is the jump itself, which is the later of the two.
        from_date.max(from_instant)
    }

    /// The last instant of a local date.
    ///
    /// Used for the far end of an inclusive window, so a purchase at 23:59 falls inside the
    /// day it looks like it falls inside.
    ///
    /// Defined as a nanosecond before the next day begins rather than as 23:59:59.999 on
    /// this one. The two differ whenever the zone shifts during the day: reckoning the end
    /// of the day at the offset in force at its *start* loses the last hour of an autumn
    /// day, and a store query built from it would then skip purchases that
    /// [`Zone::local_date`] agrees are inside the window.
    fn end_of_day(&self, date: Date) -> OffsetDateTime {
        match date.next_day() {
            Some(next) => self.start_of_day(next) - Duration::nanoseconds(1),
            // The end of the representable calendar; there is no next day to work back from.
            None => date
                .with_time(Time::MAX)
                .assume_offset(self.offset_for_date(date)),
        }
    }
}

/// A zone that never shifts answers both questions with itself.
impl Zone for UtcOffset {
    fn offset_for_instant(&self, _: OffsetDateTime) -> UtcOffset {
        *self
    }

    fn offset_for_date(&self, _: Date) -> UtcOffset {
        *self
    }
}

/// A borrowed zone is a zone, so `&impl Zone` can be passed on without reborrowing dances.
impl<Z: Zone + ?Sized> Zone for &Z {
    fn offset_for_instant(&self, at: OffsetDateTime) -> UtcOffset {
        (**self).offset_for_instant(at)
    }

    fn offset_for_date(&self, date: Date) -> UtcOffset {
        (**self).offset_for_date(date)
    }
}
