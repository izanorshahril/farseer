//! Identity.
//!
//! `02 record scope` needs two ids on every event and is explicit about why one cannot do
//! both jobs: UUIDv7 is only k-sortable, so two events in the same millisecond
//! have a random tail and no deterministic order. `seq` is the cursor.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The cursor. A monotonic per-log integer that never leaves the machine.
///
/// `09 store decision` benched this as `INTEGER PRIMARY KEY`, therefore the rowid, which is
/// what makes a range scan a b-tree seek plus a sequential walk. It is **not
/// contiguous after a purge**, so nothing may infer a count from a delta.
pub type Seq = i64;

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A fresh time-ordered id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }

            pub fn as_bytes(&self) -> &[u8; 16] {
                self.0.as_bytes()
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(Uuid::from_bytes(bytes))
            }

            /// The all-zero id, for a record entry that belongs to a **cell**
            /// rather than to any run.
            ///
            /// `17 cell lifecycle`'s verbs act on the cell, and the events they
            /// append have no run to name. Minting a fresh id instead would put
            /// a run in the record that never existed, which `02 record scope`
            /// forbids more strongly than it requires the column to be
            /// interesting.
            pub fn none() -> Self {
                Self(Uuid::nil())
            }

            /// Whether this is [`Self::none`].
            pub fn is_none(&self) -> bool {
                self.0.is_nil()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

uuid_id!(
    EventId,
    "The portable identity of an event. UUIDv7, per `02 record scope`."
);
uuid_id!(
    RunId,
    "One worker contract's execution, per `07 attach semantics`."
);
uuid_id!(TaskId, "Groups runs, per `11 analytics questions`.");
uuid_id!(
    CallId,
    "One cell call, per `06 cell transport`. Returned to the caller immediately, because a cell call is fire-and-forget and its result arrives on the event stream."
);
uuid_id!(
    MemoryId,
    "One memory claim. `25 memory lifecycle` retracts by appending a superseding tombstone, so an id here is never reused and never removed."
);

/// A cell's identity.
///
/// `17 cell lifecycle` requires this to be **stable across reload and never derived from
/// content**: content changes on every edit, and `06 cell transport` needs the id to survive a
/// reload or the record loses its join key. So it is an author-chosen string,
/// not a hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CellId(String);

impl CellId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CellId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_ids_are_time_ordered_across_milliseconds() {
        let a = EventId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = EventId::new();
        assert!(a.as_uuid() < b.as_uuid(), "uuidv7 must sort by time");
    }

    #[test]
    fn event_id_survives_a_byte_round_trip() {
        let a = EventId::new();
        assert_eq!(a, EventId::from_bytes(*a.as_bytes()));
    }

    #[test]
    fn cell_id_is_the_authors_string_not_a_hash() {
        assert_eq!(CellId::new("social").as_str(), "social");
    }
}
