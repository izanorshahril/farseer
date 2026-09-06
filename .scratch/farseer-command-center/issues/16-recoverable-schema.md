# 16: Recoverable schema and backup

**Parent:** [Command-center decision map](../map.md).
**What to build:** Upgrade or restore a local record through an explicit versioned storage contract, and make failure recoverable before any automatic installation change.
**Blocked by:** None; package approved.
**Status:** implemented.
**Execution:** package approved; verify named blockers before implementation.
**Review refs:** R14; R13 applies as targeted cleanup.
**Decision:** [Self-maintenance and domain integration](../decisions/05-maintenance.md).

## Contract choices

Preserve the existing specific attachment migration and add ordered schema versions with reader/writer compatibility rules.
The ordinary store-open path handles supported upgrades and refuses unsupported versions before admitting work.
Provide a local backup/restore operation using a consistent SQLite snapshot and a manifest covering referenced attachment bytes.
Store format versions describe schema; projection versions continue describing derived meaning.
An incompatible old binary refuses newer data instead of trying to repair it.
A backup and matching binary form the rollback unit.
Use a local directory fixture for recovery tests; do not touch the operator's live record.

## Acceptance criteria

- [x] A supported prior schema upgrades once, preserving task/run lineage and repeated attachment associations, and reopens without a second migration.
- [x] Unsupported future schema returns a clear nonzero startup error before new work is admitted.
- [x] Injected migration/disk failure leaves the original or documented recoverable state, with no partially admitted run.
- [x] Backup/restore through the public CLI reproduces the record and attachment references using the matching version, including committed WAL state.
- [x] A restore is validated by opening and querying the recovered record, not merely by finding backup files.

**Exclusions:** Cloud backup, generic tiering, multi-writer storage, and executing real installation promotion.
**Test seam/demo:** Upgrade an old fixture, inject failure, restore its verified backup, and query the same task and attachment through the normal interface.

## Evidence

Implemented in `crates/farseer-store/src/lib.rs`, `crates/farseer-store/Cargo.toml`, and `crates/farseer/src/main.rs`.
`Store::migrate_schema` now applies ordered version dispatch and wraps additive schema creation plus the attachment migration in one recoverable transaction.
`a_migrated_store_reopens_without_losing_work_lineage_or_associations` covers the legacy upgrade, durable task/run lineage, repeated digest associations, and idempotent reopen.
`a_failed_migration_rolls_back_to_the_original_fixture` injects an invalid legacy fixture and proves the original rows remain while new schema tables are not published.
`crates/farseer/tests/recovery.rs` invokes the public binary for future-schema refusal and backup/restore while a writer holds committed WAL data, then opens the restored record and verifies the run, task, attachment reference, and bytes.
The full store suite and the two CLI recovery tests pass.
