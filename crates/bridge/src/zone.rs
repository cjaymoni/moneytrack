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
use time::{Date, Duration, OffsetDateTime, Time, UtcOffset};

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

/// A time zone, as a base offset plus the changes to it, and the window it is right for.
///
/// Supply transitions covering the window being asked about, in any order. Outside the
/// range supplied the base offset stands, so a zone that never shifts needs no transitions
/// and a zone that does needs only the ones near the dates in play.
///
/// `base_offset_seconds` is the offset in force *before the first transition*, which is not
/// the same as the offset in force now. Handing over "the current offset" with a truncated
/// transition list would otherwise be the one way to get silently wrong answers out of this
/// type: a table that stops short looks exactly like a zone that stopped shifting, and no
/// amount of care in Rust can tell the two apart.
///
/// Which is what `covers_from` and `covers_through` are for. State the window the table was
/// built for, and every call that reckons a local date outside it is refused rather than
/// answered from the base offset. Leaving them unset makes no claim and skips the check —
/// honest only for a zone with no shifts at all, which is why [`ZoneDto::fixed`] is the one
/// constructor that does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneDto {
    /// The offset in force before the first transition, in seconds east of UTC.
    pub base_offset_seconds: i32,
    /// Every change to it.
    pub transitions: Vec<ZoneTransitionDto>,
    /// First instant the transition list is complete for, RFC 3339.
    ///
    /// A question about a local date is answered from instants up to 26 hours either side
    /// of its UTC midnight, so claim 26 hours beyond the earliest date you will ask about.
    /// Set both ends or neither.
    pub covers_from: Option<String>,
    /// Last instant the transition list is complete for, RFC 3339.
    ///
    /// As [`ZoneDto::covers_from`]: claim 26 hours beyond the latest date you will ask
    /// about, and set both ends or neither.
    pub covers_through: Option<String>,
}

impl ZoneDto {
    /// A zone that never shifts, which is therefore right for every instant there is.
    #[must_use]
    pub fn fixed(offset_seconds: i32) -> Self {
        Self {
            base_offset_seconds: offset_seconds,
            transitions: Vec::new(),
            covers_from: None,
            covers_through: None,
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
        let covers_from = scalar::optional_instant("zone.covers_from", self.covers_from.as_ref())?;
        let covers_through =
            scalar::optional_instant("zone.covers_through", self.covers_through.as_ref())?;
        match (covers_from, covers_through) {
            (Some(from), Some(through)) if from > through => {
                return Err(BridgeError::OutOfRange {
                    field: "zone.covers_through".to_owned(),
                    value: through.to_string(),
                });
            }
            // A claim open at one end is the hazard this type exists to close, wearing the
            // badge of a table that passed the check: a window starting in January and
            // never ending answers for ever from a transition list that stops in October.
            (Some(_), None) => {
                return Err(BridgeError::OutOfRange {
                    field: "zone.covers_through".to_owned(),
                    value: "a window claimed at one end only".to_owned(),
                });
            }
            (None, Some(_)) => {
                return Err(BridgeError::OutOfRange {
                    field: "zone.covers_from".to_owned(),
                    value: "a window claimed at one end only".to_owned(),
                });
            }
            _ => {}
        }

        Ok(ZoneTable {
            base: offset("zone.base_offset_seconds", self.base_offset_seconds)?,
            transitions,
            covers_from,
            covers_through,
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
    covers_from: Option<OffsetDateTime>,
    covers_through: Option<OffsetDateTime>,
}

/// How far either side of a local date an answer about it can reach.
///
/// A local date becomes an instant at the zone's own offset, which [`UtcOffset`] allows up
/// to just under 26 hours either way. Callers check coverage with this margin so a window
/// that only just reaches a date is not accepted for a question about it.
///
/// Only a caller that turns a local *date* into an instant needs it. Reading the local date
/// of an instant already in hand does not, which is why budget evaluation checks its rows
/// without a margin. If [`mt_core::budget::period_bounds`] is ever published from this
/// crate it does need one: that is the one core function that converts a period's dates
/// into instants.
pub const COVERAGE_MARGIN: Duration = Duration::hours(26);

impl ZoneTable {
    /// Refuses a question whose answer would be read from outside the stated window.
    ///
    /// A table claiming no window answers everything, which is the fixed-offset case. One
    /// that claims a window is held to it: past the edge the base offset silently stands
    /// in, and the symptom is a bill counted against the wrong month rather than anything
    /// that looks like an error.
    ///
    /// # Errors
    /// [`BridgeError::ZoneCoverage`] naming the end that falls outside.
    pub fn check_covers(
        &self,
        field: &str,
        from: OffsetDateTime,
        through: OffsetDateTime,
    ) -> Result<(), BridgeError> {
        let outside = if self.covers_from.is_some_and(|covered| from < covered) {
            Some(from)
        } else if self.covers_through.is_some_and(|covered| through > covered) {
            Some(through)
        } else {
            None
        };

        match outside {
            Some(bound) => Err(BridgeError::ZoneCoverage {
                field: field.to_owned(),
                needed: scalar::format_instant("zone.coverage", bound)?,
            }),
            None => Ok(()),
        }
    }
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
