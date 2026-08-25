//! UI state: the one thing farseer stores and never reads.
//!
//! `24` made this a fourth category alongside events, memory and attachments,
//! and it differs from all three on every rule `02` set: **mutable,
//! last-write-wins, no `seq`, no scrub, and no event emitted on write** - a
//! cursor drag is not history, and logging one would flood the log.
//!
//! It is the second unscrubbed thing after attachments, and for the same reason:
//! farseer cannot scrub what it will not read.

use rusqlite::OptionalExtension;

use crate::{Result, Store, StoreError};

/// `24`: above this, the API answers `413`.
pub const UI_STATE_CAP_BYTES: usize = 1024 * 1024;

/// `24 ui state persistence`'s second invariant: a cap on the key itself, so an
/// opaque key cannot grow without limit either. The key stays an opaque string -
/// capping its length is not parsing it.
pub const UI_STATE_KEY_CAP_BYTES: usize = 256;

impl Store {
    /// Read a blob back, or `None` if this key was never written.
    ///
    /// This is what makes a canvas layout survive a restart instead of coming
    /// back vanilla.
    pub fn ui_state(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn()
            .query_row("SELECT blob FROM ui_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Store a blob under an opaque key.
    ///
    /// No validation, no schema, no reference-following, and **no concurrency
    /// control** - that is `01`'s out-of-scope ruling on concurrent clients
    /// rather than a shortcut. The key is opaque and farseer never splits it on
    /// a separator.
    pub fn put_ui_state(&self, key: &str, blob: &[u8], ts: i64) -> Result<()> {
        if key.len() > UI_STATE_KEY_CAP_BYTES {
            return Err(StoreError::UiStateKeyTooLong {
                size: key.len(),
                cap: UI_STATE_KEY_CAP_BYTES,
            });
        }
        if blob.len() > UI_STATE_CAP_BYTES {
            return Err(StoreError::UiStateTooLarge {
                key: key.to_string(),
                size: blob.len(),
                cap: UI_STATE_CAP_BYTES,
            });
        }
        self.conn().execute(
            "INSERT INTO ui_state (key, blob, ts) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET blob = excluded.blob, ts = excluded.ts",
            rusqlite::params![key, blob, ts],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanFilter;

    #[test]
    fn a_layout_written_before_a_restart_reads_back_byte_for_byte() {
        let s = Store::open_in_memory().unwrap();
        let blob = br#"{"widgets":[{"id":"trades","x":0,"y":0}]}"#;
        s.put_ui_state("command-center", blob, 1).unwrap();
        assert_eq!(
            s.ui_state("command-center").unwrap().as_deref(),
            Some(&blob[..])
        );
    }

    #[test]
    fn a_write_is_last_write_wins_and_emits_no_event() {
        let s = Store::open_in_memory().unwrap();
        s.put_ui_state("k", b"first", 1).unwrap();
        s.put_ui_state("k", b"second", 2).unwrap();
        assert_eq!(s.ui_state("k").unwrap().as_deref(), Some(&b"second"[..]));
        assert!(
            s.scan(0, 10, &ScanFilter::default()).unwrap().is_empty(),
            "a cursor drag is not history"
        );
    }

    #[test]
    fn an_unwritten_key_is_absent_rather_than_empty() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.ui_state("never-written").unwrap(), None);
    }

    #[test]
    fn the_key_is_opaque_and_farseer_never_splits_it() {
        let s = Store::open_in_memory().unwrap();
        s.put_ui_state("a/b:c.d", b"x", 1).unwrap();
        assert_eq!(s.ui_state("a/b:c.d").unwrap().as_deref(), Some(&b"x"[..]));
        assert_eq!(s.ui_state("a").unwrap(), None);
    }

    #[test]
    fn a_blob_over_a_mebibyte_is_refused() {
        let s = Store::open_in_memory().unwrap();
        let oversized = vec![0u8; UI_STATE_CAP_BYTES + 1];
        assert!(matches!(
            s.put_ui_state("big", &oversized, 1),
            Err(StoreError::UiStateTooLarge { .. })
        ));
        assert!(s.put_ui_state("big", &oversized[1..], 1).is_ok());
    }

    #[test]
    fn a_key_over_the_key_cap_is_refused() {
        let s = Store::open_in_memory().unwrap();
        let long = "k".repeat(UI_STATE_KEY_CAP_BYTES + 1);
        assert!(matches!(
            s.put_ui_state(&long, b"x", 1),
            Err(StoreError::UiStateKeyTooLong { .. })
        ));
        assert!(s.put_ui_state(&long[1..], b"x", 1).is_ok());
    }

    #[test]
    fn a_blob_is_stored_verbatim_and_never_scrubbed() {
        // `24`: farseer cannot scrub what it will not read, and stating that is
        // the point. A UI storing something secret-shaped keeps it verbatim.
        let s = Store::open_in_memory().unwrap();
        let blob = b"ghp_ZzAa0011223344556677889900";
        s.put_ui_state("k", blob, 1).unwrap();
        assert_eq!(s.ui_state("k").unwrap().as_deref(), Some(&blob[..]));
    }
}
