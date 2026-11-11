//! The user's time zone, carried across the bridge as a table rather than a name.
//!
//! `mt-core` refuses to know what `Africa/Accra` means, and deliberately: shipping tzdata
//! into a WASM bundle to answer "when is midnight in Accra" is weight the platform already
//! carries. So the platform answers. Dart reads its own zone database, hands over the
//! offsets in force across the window being worked on, and this module implements
//! [`mt_core::zone::Zone`] over them.
//!
//! A fixed-offset zone — Ghana's among them — is the degenerate case: a base offset and no
//! transitions at all.

use mt_core::zone::Zone;
use time::{Date, OffsetDateTime, Time, UtcOffset};

use crate::error::BridgeError;
use crate::scalar;

/// One moment a zone's offset changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneTransitionDto {
    /// The instant the new offset takes effect, RFC 3339.
    pub at: String,
    /// The offset in force from then on, in seconds east of UTC.
    pub offset_seconds: i32,
}

/// A time zone, as a base offset plus the changes to it.
///
/// Supply transitions covering the window being asked about, in any order. Outside the
/// range supplied the base offset stands, so a zone that never shifts needs no transitions
/// and a zone that does needs only the ones near the dates in play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneDto {
    /// The offset in force before the first transition, in seconds east of UTC.
    pub base_offset_seconds: i32,
    /// Every change to it.
    pub transitions: Vec<ZoneTransitionDto>,
}

impl ZoneDto {
    /// A fixed-offset zone.
    #[must_use]
    pub fn fixed(offset_seconds: i32) -> Self {
        Self {
            base_offset_seconds: offset_seconds,
            transitions: Vec::new(),
        }
    }

    /// Reads the table into something the core's rules can ask questions of.
    ///
    /// # Errors
    /// [`BridgeError::OutOfRange`] for an offset beyond ±25:59:59, or
    /// [`BridgeError::InvalidTimestamp`] for a transition instant that does not parse.
    pub fn to_table(&self) -> Result<ZoneTable, BridgeError> {
        let mut transitions = Vec::with_capacity(self.transitions.len());
        for transition in &self.transitions {
            transitions.push((
                scalar::instant("zone.transition.at", &transition.at)?,
                offset("zone.transition.offset_seconds", transition.offset_seconds)?,
            ));
        }
        // Sorted here rather than trusted: the caller is a platform zone database read
        // through Dart, and a table out of order would silently answer with the wrong
        // offset instead of failing.
        transitions.sort_by_key(|(at, _)| *at);
        Ok(ZoneTable {
            base: offset("zone.base_offset_seconds", self.base_offset_seconds)?,
            transitions,
        })
    }
}

/// Reads an offset in seconds east of UTC.
fn offset(field: &str, seconds: i32) -> Result<UtcOffset, BridgeError> {
    UtcOffset::from_whole_seconds(seconds).map_err(|_| BridgeError::OutOfRange {
        field: field.to_owned(),
        value: seconds.to_string(),
    })
}

/// A zone the core's rules can ask both of its questions of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneTable {
    base: UtcOffset,
    /// Ascending by instant.
    transitions: Vec<(OffsetDateTime, UtcOffset)>,
}

impl Zone for ZoneTable {
    fn offset_for_instant(&self, at: OffsetDateTime) -> UtcOffset {
        let after = self.transitions.partition_point(|(start, _)| *start <= at);
        match after
            .checked_sub(1)
            .and_then(|last| self.transitions.get(last))
        {
            Some((_, offset)) => *offset,
            None => self.base,
        }
    }

    fn offset_for_date(&self, date: Date) -> UtcOffset {
        // A local date is not an instant, so this is asked in two steps: guess with the
        // offset in force at the same wall clock read as UTC, then re-ask at the instant
        // that guess produces. The two agree everywhere except the hour a zone shifts,
        // where either answer is permitted and `Zone::start_of_day` corrects whichever
        // this returns.
        let naive = date.with_time(Time::MIDNIGHT);
        let guess = self.offset_for_instant(naive.assume_utc());
        self.offset_for_instant(naive.assume_offset(guess))
    }
}
