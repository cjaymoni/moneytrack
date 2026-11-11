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

use time::{Date, OffsetDateTime, Time, UtcOffset};

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
    /// On the day a zone shifts, a date covers more than one offset. Returning the one in
    /// force at the start of the day is the convention here, which keeps a bill generated
    /// for "the first of the month" landing on the first of the month.
    fn offset_for_date(&self, date: Date) -> UtcOffset;

    /// The local date an instant falls on.
    fn local_date(&self, at: OffsetDateTime) -> Date {
        at.to_offset(self.offset_for_instant(at)).date()
    }

    /// The instant a local date begins.
    fn start_of_day(&self, date: Date) -> OffsetDateTime {
        date.with_time(Time::MIDNIGHT)
            .assume_offset(self.offset_for_date(date))
    }

    /// The last representable instant of a local date.
    ///
    /// Used for the far end of an inclusive window, so a purchase at 23:59 falls inside the
    /// day it looks like it falls inside.
    fn end_of_day(&self, date: Date) -> OffsetDateTime {
        date.with_time(Time::MAX)
            .assume_offset(self.offset_for_date(date))
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
