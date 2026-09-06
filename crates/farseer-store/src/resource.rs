//! Durable optional process-resource observations.

use serde::Serialize;

use farseer_core::RunId;

use crate::{Result, Store};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceSample {
    pub run_id: RunId,
    pub source: String,
    pub scope: String,
    pub cpu_time_100ns: Option<u64>,
    pub memory_high_water_bytes: Option<u64>,
    pub cpu_unit: String,
    pub memory_unit: String,
    pub timestamp_ms: i64,
    pub collector_version: String,
    pub status: String,
    pub final_sample: bool,
}

impl Store {
    pub fn record_resource(&self, sample: &ResourceSample) -> Result<()> {
        self.conn().execute(
            "INSERT INTO resource_samples
             (run_id, source, scope, cpu_time_100ns, memory_high_water_bytes,
              cpu_unit, memory_unit, timestamp_ms, collector_version, status, final_sample)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &sample.run_id.as_bytes()[..],
                sample.source,
                sample.scope,
                sample.cpu_time_100ns.map(|value| value as i64),
                sample.memory_high_water_bytes.map(|value| value as i64),
                sample.cpu_unit,
                sample.memory_unit,
                sample.timestamp_ms,
                sample.collector_version,
                sample.status,
                sample.final_sample,
            ],
        )?;
        Ok(())
    }

    pub fn resource_samples(&self, run_id: RunId, limit: usize) -> Result<Vec<ResourceSample>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT source, scope, cpu_time_100ns, memory_high_water_bytes,
                    cpu_unit, memory_unit, timestamp_ms, collector_version, status, final_sample
             FROM resource_samples WHERE run_id = ?1
             ORDER BY timestamp_ms, rowid LIMIT ?2",
        )?;
        let rows = statement.query_map(
            rusqlite::params![&run_id.as_bytes()[..], limit.min(1000) as i64],
            |row| {
                Ok(ResourceSample {
                    run_id,
                    source: row.get(0)?,
                    scope: row.get(1)?,
                    cpu_time_100ns: row.get::<_, Option<i64>>(2)?.map(|value| value as u64),
                    memory_high_water_bytes: row
                        .get::<_, Option<i64>>(3)?
                        .map(|value| value as u64),
                    cpu_unit: row.get(4)?,
                    memory_unit: row.get(5)?,
                    timestamp_ms: row.get(6)?,
                    collector_version: row.get(7)?,
                    status: row.get(8)?,
                    final_sample: row.get(9)?,
                })
            },
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn prune_resource_samples(&self, before_ts: i64) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM resource_samples WHERE timestamp_ms < ?1 AND final_sample = 0",
            [before_ts],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_keeps_final_cumulative_observation() {
        let store = Store::open_in_memory().unwrap();
        let run_id = RunId::new();
        let old = ResourceSample {
            run_id,
            source: "fixture".into(),
            scope: "supervised-job".into(),
            cpu_time_100ns: Some(1),
            memory_high_water_bytes: Some(2),
            cpu_unit: "100ns".into(),
            memory_unit: "bytes".into(),
            timestamp_ms: 1,
            collector_version: "fixture-v1".into(),
            status: "measured".into(),
            final_sample: false,
        };
        let final_sample = ResourceSample {
            timestamp_ms: 2,
            final_sample: true,
            ..old.clone()
        };
        store.record_resource(&old).unwrap();
        store.record_resource(&final_sample).unwrap();
        assert_eq!(store.prune_resource_samples(2).unwrap(), 1);
        let rows = store.resource_samples(run_id, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].final_sample);
    }
}
