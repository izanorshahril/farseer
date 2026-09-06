//! Optional, best-effort resource observations for supervised jobs.
//!
//! `21 optional supervised resource monitor` deliberately keeps this observer
//! outside the runner contract: a failed observation cannot change execution.

use serde::{Deserialize, Serialize};

/// The collector version is part of the observation so future Windows API
/// changes cannot make old values look interchangeable with new ones.
pub const COLLECTOR_VERSION: &str = "windows-job-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceObservation {
    pub run_id: String,
    pub source: String,
    pub scope: String,
    pub cpu_time_100ns: Option<u64>,
    pub memory_high_water_bytes: Option<u64>,
    pub cpu_unit: &'static str,
    pub memory_unit: &'static str,
    pub timestamp_ms: i64,
    pub collector_version: &'static str,
    pub status: &'static str,
    pub final_sample: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobMetrics {
    pub cpu_time_100ns: u64,
    pub memory_high_water_bytes: u64,
}

pub fn observation(
    run_id: impl Into<String>,
    metrics: Option<JobMetrics>,
    timestamp_ms: i64,
    final_sample: bool,
) -> ResourceObservation {
    let (cpu_time_100ns, memory_high_water_bytes, status) = match metrics {
        Some(metrics) => (
            Some(metrics.cpu_time_100ns),
            Some(metrics.memory_high_water_bytes),
            "measured",
        ),
        None => (None, None, "unavailable"),
    };
    ResourceObservation {
        run_id: run_id.into(),
        source: "windows-job-object".into(),
        scope: "supervised-job".into(),
        cpu_time_100ns,
        memory_high_water_bytes,
        cpu_unit: "100ns",
        memory_unit: "bytes",
        timestamp_ms,
        collector_version: COLLECTOR_VERSION,
        status,
        final_sample,
    }
}

#[cfg(windows)]
pub(crate) fn query_job(
    job: windows::Win32::Foundation::HANDLE,
) -> windows::core::Result<JobMetrics> {
    use windows::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    };
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let mut returned = 0u32;
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut _,
            std::mem::size_of_val(&accounting) as u32,
            Some(&mut returned),
        )?;
    }
    let mut extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectExtendedLimitInformation,
            &mut extended as *mut _ as *mut _,
            std::mem::size_of_val(&extended) as u32,
            Some(&mut returned),
        )?;
    }
    let cpu = accounting.TotalUserTime as u64 + accounting.TotalKernelTime as u64;
    Ok(JobMetrics {
        cpu_time_100ns: cpu,
        memory_high_water_bytes: extended.PeakJobMemoryUsed as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_values_are_nonnegative_and_provenanced() {
        let value = observation(
            "run-fixture",
            Some(JobMetrics {
                cpu_time_100ns: 12,
                memory_high_water_bytes: 4096,
            }),
            123,
            true,
        );
        assert_eq!(value.status, "measured");
        assert_eq!(value.cpu_time_100ns, Some(12));
        assert_eq!(value.memory_high_water_bytes, Some(4096));
        assert_eq!(value.source, "windows-job-object");
        assert_eq!(value.scope, "supervised-job");
        assert_eq!(value.timestamp_ms, 123);
    }

    #[test]
    fn collector_failure_is_explicitly_unavailable() {
        let value = observation("run-fixture", None, 123, false);
        assert_eq!(value.status, "unavailable");
        assert_eq!(value.cpu_time_100ns, None);
        assert_eq!(value.memory_high_water_bytes, None);
    }
}
