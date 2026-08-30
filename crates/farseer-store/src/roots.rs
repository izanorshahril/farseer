//! Where farseer is allowed to work.
//!
//! `39 what an installed farseer points at` settled that farseer is an
//! application that **points at** projects rather than a tool run inside one,
//! and that the operator names the directories it may point at. A row here is
//! one such grant.
//!
//! Two rules, both from `39`:
//!
//! - **Only roots are stored.** A project is a directory inside a root and is
//!   never registered, so the list cannot drift from a disk the operator also
//!   edits in Explorer.
//! - **Paths are stored canonicalized.** The comparison a launch makes is
//!   `is this canonical path inside a canonical root`, and a list holding the
//!   string somebody typed cannot answer that.

use crate::{Result, Store};

impl Store {
    /// Every authorized root, oldest grant first.
    pub fn roots(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path FROM roots ORDER BY ts, path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Grant access to a directory. Granting one already granted moves nothing.
    ///
    /// The caller canonicalizes: this crate does not touch the filesystem, and
    /// a store that silently resolved paths would make the row disagree with
    /// the check the launch path runs.
    pub fn authorize_root(&self, canonical_path: &str, ts: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO roots (path, ts) VALUES (?1, ?2) ON CONFLICT(path) DO NOTHING",
            rusqlite::params![canonical_path, ts],
        )?;
        Ok(())
    }

    /// Withdraw a grant. Returns whether there was one.
    ///
    /// Nothing on disk is touched. Revoking access to a directory and deleting
    /// the operator's work are not the same act, and only one of them is what
    /// the button says.
    pub fn revoke_root(&self, canonical_path: &str) -> Result<bool> {
        let n = self
            .conn()
            .execute("DELETE FROM roots WHERE path = ?1", [canonical_path])?;
        Ok(n > 0)
    }
}
