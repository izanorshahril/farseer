//! Bounded maintenance proposals and explicit fixture promotion.
//!
//! This module owns reviewable metadata and file-layout primitives only.  It
//! never starts a process, edits the active runtime, or decides to promote a
//! candidate.  Callers must invoke each promotion phase explicitly.
//!
//! `17 bounded maintenance source proposals` and `18 safe staged runtime
//! promotion` define the lineage, identity, drain, backup and rollback rules.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROPOSAL_FORMAT_VERSION: u32 = 1;
pub const PROMOTION_FORMAT_VERSION: u32 = 1;
pub const MAX_PROPOSAL_ATTEMPTS: usize = 1;
pub const MAX_PROPOSAL_SCOPE: usize = 32;
pub const MAX_PROPOSAL_FIELD_BYTES: usize = 4 * 1024;
pub const MAX_VALIDATION_EVIDENCE: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceError {
    #[error("maintenance field `{0}` is empty")]
    EmptyField(&'static str),
    #[error("maintenance proposal `{0}` is already active")]
    ActiveProposal(String),
    #[error("trigger `{0}` already belongs to proposal `{1}`")]
    DuplicateTrigger(String, String),
    #[error("proposal `{0}` was not found")]
    MissingProposal(String),
    #[error("proposal attempt limit is one")]
    AttemptLimit,
    #[error("maintenance proposal `{0}` is not open")]
    ProposalNotOpen(String),
    #[error("maintenance proposal `{0}` already has a task")]
    TaskAlreadyLinked(String),
    #[error("maintenance field `{field}` exceeds {limit} bytes")]
    FieldTooLarge { field: &'static str, limit: usize },
    #[error("maintenance validation evidence exceeds {0} rows")]
    EvidenceTooLarge(usize),
    #[error("successful proposal requires a candidate and validation evidence")]
    MissingEvidence,
    #[error("invalid promotion transition from `{from}` to `{to}`")]
    InvalidPromotionTransition { from: String, to: String },
    #[error("runtime identity mismatch for `{0}`")]
    IdentityMismatch(&'static str),
    #[error("health check failed: {0}")]
    HealthFailed(String),
    #[error("runtime drain still has {0} active run(s)")]
    DrainIncomplete(usize),
    #[error("unsafe fixture path component `{0}`")]
    UnsafePath(String),
    #[error("fixture path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error("fixture destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("backup is invalid: {0}")]
    InvalidBackup(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, MaintenanceError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Open,
    Succeeded,
    Failed,
    Cancelled,
}

impl ProposalStatus {
    fn active(&self) -> bool {
        matches!(self, Self::Open)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationEvidence {
    pub command: String,
    pub outcome: String,
    pub exit_code: Option<i32>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaintenanceAttempt {
    pub number: usize,
    pub started_ts: i64,
    pub finished_ts: Option<i64>,
    pub evidence: Vec<ValidationEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateSource {
    pub artifact: String,
    pub branch: Option<String>,
    pub reproducer: Option<String>,
    pub validation: Vec<ValidationEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProposalMetadata {
    pub format_version: u32,
    pub proposal_id: String,
    pub lineage_id: String,
    pub trigger_id: String,
    pub actor: String,
    pub source_revision: String,
    pub previous_revision: String,
    pub scope: Vec<String>,
    /// Existing work identity, when this proposal has been admitted as an
    /// ordinary task in the record.
    #[serde(default)]
    pub task_id: Option<String>,
    pub candidate: Option<CandidateSource>,
    pub attempts: Vec<MaintenanceAttempt>,
    pub status: ProposalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProposalRequest {
    pub proposal_id: String,
    pub lineage_id: String,
    pub trigger_id: String,
    pub actor: String,
    pub source_revision: String,
    pub previous_revision: String,
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginProposal {
    Created(String),
    Existing(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProposalLedger {
    pub proposals: Vec<ProposalMetadata>,
}

impl ProposalLedger {
    /// Start one bounded proposal.  A trigger is durable deduplication state,
    /// including after failure, so a restart cannot create a retry loop.
    pub fn begin(&mut self, request: ProposalRequest) -> Result<BeginProposal> {
        validate_request(&request)?;
        if let Some(existing) = self
            .proposals
            .iter()
            .find(|proposal| proposal.trigger_id == request.trigger_id)
        {
            if existing.lineage_id != request.lineage_id {
                return Err(MaintenanceError::DuplicateTrigger(
                    request.trigger_id,
                    existing.proposal_id.clone(),
                ));
            }
            return Ok(BeginProposal::Existing(existing.proposal_id.clone()));
        }
        if let Some(existing) = self
            .proposals
            .iter()
            .find(|proposal| proposal.status.active())
        {
            return Err(MaintenanceError::ActiveProposal(
                existing.proposal_id.clone(),
            ));
        }
        if self
            .proposals
            .iter()
            .any(|proposal| proposal.proposal_id == request.proposal_id)
        {
            return Err(MaintenanceError::DuplicateTrigger(
                request.proposal_id.clone(),
                request.proposal_id.clone(),
            ));
        }
        let proposal_id = request.proposal_id.clone();
        self.proposals.push(ProposalMetadata {
            format_version: PROPOSAL_FORMAT_VERSION,
            proposal_id,
            lineage_id: request.lineage_id,
            trigger_id: request.trigger_id,
            actor: request.actor,
            source_revision: request.source_revision,
            previous_revision: request.previous_revision,
            scope: request.scope,
            task_id: None,
            candidate: None,
            attempts: Vec::new(),
            status: ProposalStatus::Open,
        });
        Ok(BeginProposal::Created(request.proposal_id))
    }

    /// Link the proposal to the ordinary task that carries its work history.
    pub fn link_task(&mut self, proposal_id: &str, task_id: String) -> Result<()> {
        if task_id.trim().is_empty() {
            return Err(MaintenanceError::EmptyField("task_id"));
        }
        let proposal = self
            .proposals
            .iter_mut()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .ok_or_else(|| MaintenanceError::MissingProposal(proposal_id.into()))?;
        if proposal.task_id.is_some() {
            return Err(MaintenanceError::TaskAlreadyLinked(proposal_id.into()));
        }
        if !proposal.status.active() {
            return Err(MaintenanceError::ProposalNotOpen(proposal_id.into()));
        }
        proposal.task_id = Some(task_id);
        Ok(())
    }

    pub fn record_attempt(&mut self, proposal_id: &str, attempt: MaintenanceAttempt) -> Result<()> {
        let proposal = self
            .proposals
            .iter_mut()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .ok_or_else(|| MaintenanceError::MissingProposal(proposal_id.into()))?;
        if !proposal.status.active() {
            return Err(MaintenanceError::ProposalNotOpen(proposal_id.into()));
        }
        if proposal.attempts.len() >= MAX_PROPOSAL_ATTEMPTS {
            return Err(MaintenanceError::AttemptLimit);
        }
        if attempt.number != proposal.attempts.len() + 1 {
            return Err(MaintenanceError::AttemptLimit);
        }
        validate_evidence(&attempt.evidence)?;
        proposal.attempts.push(attempt);
        Ok(())
    }

    pub fn attach_candidate(
        &mut self,
        proposal_id: &str,
        candidate: CandidateSource,
    ) -> Result<()> {
        if candidate.artifact.trim().is_empty() {
            return Err(MaintenanceError::EmptyField("candidate.artifact"));
        }
        check_field("candidate.artifact", &candidate.artifact)?;
        if let Some(branch) = &candidate.branch {
            check_field("candidate.branch", branch)?;
        }
        if let Some(reproducer) = &candidate.reproducer {
            check_field("candidate.reproducer", reproducer)?;
        }
        validate_evidence(&candidate.validation)?;
        let proposal = self
            .proposals
            .iter_mut()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .ok_or_else(|| MaintenanceError::MissingProposal(proposal_id.into()))?;
        if !proposal.status.active() {
            return Err(MaintenanceError::ProposalNotOpen(proposal_id.into()));
        }
        proposal.candidate = Some(candidate);
        Ok(())
    }

    pub fn finish(&mut self, proposal_id: &str, status: ProposalStatus) -> Result<()> {
        if matches!(status, ProposalStatus::Open) {
            return Err(MaintenanceError::InvalidPromotionTransition {
                from: "proposal".into(),
                to: "open".into(),
            });
        }
        let proposal = self
            .proposals
            .iter_mut()
            .find(|proposal| proposal.proposal_id == proposal_id)
            .ok_or_else(|| MaintenanceError::MissingProposal(proposal_id.into()))?;
        if !proposal.status.active() {
            return Err(MaintenanceError::ProposalNotOpen(proposal_id.into()));
        }
        if matches!(status, ProposalStatus::Succeeded)
            && (proposal.candidate.is_none() || proposal.attempts.is_empty())
        {
            return Err(MaintenanceError::MissingEvidence);
        }
        proposal.status = status;
        Ok(())
    }

    pub fn suppresses(&self, lineage_id: &str, trigger_id: &str) -> bool {
        self.proposals
            .iter()
            .any(|proposal| proposal.lineage_id == lineage_id || proposal.trigger_id == trigger_id)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        atomic_write(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }
}

fn validate_request(request: &ProposalRequest) -> Result<()> {
    for (name, value) in [
        ("proposal_id", &request.proposal_id),
        ("lineage_id", &request.lineage_id),
        ("trigger_id", &request.trigger_id),
        ("actor", &request.actor),
        ("source_revision", &request.source_revision),
        ("previous_revision", &request.previous_revision),
    ] {
        if value.trim().is_empty() {
            return Err(MaintenanceError::EmptyField(name));
        }
        check_field(name, value)?;
    }
    if request.scope.len() > MAX_PROPOSAL_SCOPE {
        return Err(MaintenanceError::FieldTooLarge {
            field: "scope",
            limit: MAX_PROPOSAL_SCOPE,
        });
    }
    for scope in &request.scope {
        check_field("scope entry", scope)?;
    }
    Ok(())
}

fn check_field(field: &'static str, value: &str) -> Result<()> {
    if value.len() > MAX_PROPOSAL_FIELD_BYTES {
        return Err(MaintenanceError::FieldTooLarge {
            field,
            limit: MAX_PROPOSAL_FIELD_BYTES,
        });
    }
    Ok(())
}

fn validate_evidence(evidence: &[ValidationEvidence]) -> Result<()> {
    if evidence.len() > MAX_VALIDATION_EVIDENCE {
        return Err(MaintenanceError::EvidenceTooLarge(MAX_VALIDATION_EVIDENCE));
    }
    for item in evidence {
        check_field("validation.command", &item.command)?;
        check_field("validation.outcome", &item.outcome)?;
        if let Some(detail) = &item.detail {
            check_field("validation.detail", detail)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeIdentity {
    pub version: String,
    pub artifact_digest: String,
    pub schema_version: i64,
}

impl RuntimeIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.version.trim().is_empty() {
            return Err(MaintenanceError::EmptyField("runtime.version"));
        }
        if self.artifact_digest.trim().is_empty() {
            return Err(MaintenanceError::EmptyField("runtime.artifact_digest"));
        }
        if self.schema_version < 0 {
            return Err(MaintenanceError::IdentityMismatch("schema version"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupIdentity {
    pub runtime: RuntimeIdentity,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromotionPlan {
    pub candidate: RuntimeIdentity,
    pub previous: RuntimeIdentity,
    pub backup: BackupIdentity,
    pub candidate_path: PathBuf,
}

impl PromotionPlan {
    pub fn validate(&self) -> Result<()> {
        self.candidate.validate()?;
        self.previous.validate()?;
        self.backup.runtime.validate()?;
        if self.candidate == self.previous {
            return Err(MaintenanceError::IdentityMismatch(
                "candidate equals previous",
            ));
        }
        if self.candidate.schema_version != self.previous.schema_version {
            return Err(MaintenanceError::IdentityMismatch(
                "candidate schema requires an explicit migration",
            ));
        }
        if self.backup.runtime != self.previous {
            return Err(MaintenanceError::IdentityMismatch(
                "backup does not match previous",
            ));
        }
        if !self.candidate_path.is_dir() {
            return Err(MaintenanceError::MissingPath(self.candidate_path.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromotionPhase {
    Planned,
    Staged,
    Drained,
    BackedUp,
    Activated,
    Healthy,
    RolledBack,
    Stopped,
}

impl std::fmt::Display for PromotionPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        serde_json::to_string(self)
            .map_err(|_| std::fmt::Error)
            .and_then(|value| f.write_str(value.trim_matches('"')))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthObservation {
    pub runtime: RuntimeIdentity,
    pub authenticated: bool,
    pub smoke_ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromotionJournal {
    pub format_version: u32,
    pub plan: PromotionPlan,
    pub phase: PromotionPhase,
    pub staged_path: PathBuf,
    pub active_path: PathBuf,
    pub previous_path: PathBuf,
    pub health: Option<HealthObservation>,
    pub recovery: Option<String>,
}

pub struct FixturePromotion {
    root: PathBuf,
}

impl FixturePromotion {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Copy the candidate beside the active fixture.  The active directory is
    /// untouched until [`Self::activate`] is explicitly called.
    pub fn stage(&self, plan: PromotionPlan) -> Result<PromotionJournal> {
        plan.validate()?;
        let candidate_digest = digest_tree(&plan.candidate_path)?;
        if candidate_digest != plan.candidate.artifact_digest {
            return Err(MaintenanceError::IdentityMismatch("candidate artifact"));
        }
        if read_identity(&plan.candidate_path)? != plan.candidate {
            return Err(MaintenanceError::IdentityMismatch("candidate runtime"));
        }
        let active_path = self.root.join("active");
        let previous_path = self
            .root
            .join("previous")
            .join(safe_component(&plan.previous.version)?);
        let active_identity = read_identity(&active_path)?;
        if active_identity != plan.previous {
            return Err(MaintenanceError::IdentityMismatch("active runtime"));
        }
        let staged_path = self
            .root
            .join("staged")
            .join(safe_component(&plan.candidate.version)?);
        if staged_path.exists() {
            return Err(MaintenanceError::DestinationExists(staged_path));
        }
        fs::create_dir_all(staged_path.parent().unwrap_or(&self.root))?;
        copy_tree(&plan.candidate_path, &staged_path)?;
        Ok(PromotionJournal {
            format_version: PROMOTION_FORMAT_VERSION,
            plan,
            phase: PromotionPhase::Staged,
            staged_path,
            active_path,
            previous_path,
            health: None,
            recovery: None,
        })
    }

    pub fn record_backup(&self, journal: &mut PromotionJournal) -> Result<()> {
        if journal.phase != PromotionPhase::Drained {
            return Err(invalid_transition(&journal.phase, PromotionPhase::BackedUp));
        }
        validate_backup_bundle(
            &journal.plan.backup.path,
            journal.plan.backup.runtime.schema_version,
        )?;
        journal.phase = PromotionPhase::BackedUp;
        Ok(())
    }

    /// Record the existing runtime's drain result.  Activation cannot proceed
    /// while any accepted run remains active.
    pub fn record_drain(&self, journal: &mut PromotionJournal, active_runs: usize) -> Result<()> {
        if journal.phase != PromotionPhase::Staged {
            return Err(invalid_transition(&journal.phase, PromotionPhase::Drained));
        }
        if active_runs != 0 {
            return Err(MaintenanceError::DrainIncomplete(active_runs));
        }
        journal.phase = PromotionPhase::Drained;
        Ok(())
    }

    /// Switch directory names atomically enough for a local fixture, with a
    /// compensating rename if the second switch fails.
    pub fn activate(&self, journal: &mut PromotionJournal) -> Result<()> {
        if journal.phase != PromotionPhase::BackedUp {
            return Err(invalid_transition(
                &journal.phase,
                PromotionPhase::Activated,
            ));
        }
        if journal.previous_path.exists() {
            return Err(MaintenanceError::DestinationExists(
                journal.previous_path.clone(),
            ));
        }
        fs::create_dir_all(journal.previous_path.parent().unwrap_or(&self.root))?;
        fs::rename(&journal.active_path, &journal.previous_path)?;
        if let Err(error) = fs::rename(&journal.staged_path, &journal.active_path) {
            let _ = fs::rename(&journal.previous_path, &journal.active_path);
            return Err(error.into());
        }
        journal.phase = PromotionPhase::Activated;
        Ok(())
    }

    pub fn verify_health(
        &self,
        journal: &mut PromotionJournal,
        observation: HealthObservation,
    ) -> Result<()> {
        if journal.phase != PromotionPhase::Activated {
            return Err(invalid_transition(&journal.phase, PromotionPhase::Healthy));
        }
        if observation.runtime != journal.plan.candidate {
            return Err(MaintenanceError::IdentityMismatch("health runtime"));
        }
        if !observation.authenticated || !observation.smoke_ok {
            return Err(MaintenanceError::HealthFailed(
                "authenticated health and smoke checks are both required".into(),
            ));
        }
        journal.health = Some(observation);
        journal.phase = PromotionPhase::Healthy;
        Ok(())
    }

    /// Restore the previous directory without consulting or starting the
    /// candidate.  A restore failure is recorded as stopped and includes an
    /// operator-facing recovery note.
    pub fn rollback(&self, journal: &mut PromotionJournal) -> Result<()> {
        if !matches!(
            journal.phase,
            PromotionPhase::Activated | PromotionPhase::Healthy
        ) {
            return Err(invalid_transition(
                &journal.phase,
                PromotionPhase::RolledBack,
            ));
        }
        let failed_path = self
            .root
            .join("failed")
            .join(safe_component(&journal.plan.candidate.version)?);
        if failed_path.exists() {
            return Err(MaintenanceError::DestinationExists(failed_path));
        }
        fs::create_dir_all(failed_path.parent().unwrap_or(&self.root))?;
        if let Err(error) = fs::rename(&journal.active_path, &failed_path) {
            journal.phase = PromotionPhase::Stopped;
            journal.recovery = Some(format!(
                "candidate remains active; move {} aside, then restore {}",
                journal.active_path.display(),
                journal.previous_path.display()
            ));
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&journal.previous_path, &journal.active_path) {
            journal.phase = PromotionPhase::Stopped;
            journal.recovery = Some(format!(
                "candidate is at {}; restore {} to {} before admitting work",
                failed_path.display(),
                journal.previous_path.display(),
                journal.active_path.display()
            ));
            return Err(error.into());
        }
        journal.phase = PromotionPhase::RolledBack;
        Ok(())
    }

    pub fn save_journal(&self, journal: &PromotionJournal) -> Result<()> {
        atomic_write(
            &self.root.join("promotion.json"),
            &serde_json::to_vec_pretty(journal)?,
        )
    }

    /// Load the durable phase journal for an explicit rollback command.
    pub fn load_journal(&self) -> Result<PromotionJournal> {
        Ok(serde_json::from_slice(&fs::read(
            self.root.join("promotion.json"),
        )?)?)
    }
}

fn invalid_transition(from: &PromotionPhase, to: PromotionPhase) -> MaintenanceError {
    MaintenanceError::InvalidPromotionTransition {
        from: from.to_string(),
        to: to.to_string(),
    }
}

fn validate_backup_bundle(path: &Path, expected_schema: i64) -> Result<()> {
    if !path.is_dir() {
        return Err(MaintenanceError::MissingPath(path.to_path_buf()));
    }
    let database = path.join(super::BACKUP_DATABASE);
    if !database.is_file() {
        return Err(MaintenanceError::MissingPath(database));
    }
    if fs::metadata(&database)?.len() == 0 {
        return Err(MaintenanceError::InvalidBackup("database is empty"));
    }
    let manifest_path = path.join(super::BACKUP_MANIFEST);
    if !manifest_path.is_file() {
        return Err(MaintenanceError::MissingPath(manifest_path));
    }
    let manifest: super::BackupManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    if manifest.format_version > super::BACKUP_FORMAT_VERSION {
        return Err(MaintenanceError::InvalidBackup(
            "unsupported format version",
        ));
    }
    if manifest.schema_version != expected_schema {
        return Err(MaintenanceError::IdentityMismatch("backup schema"));
    }
    Ok(())
}

fn safe_component(value: &str) -> Result<&str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0'))
    {
        return Err(MaintenanceError::UnsafePath(value.into()));
    }
    Ok(value)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Err(error) = fs::rename(&tmp, path) {
        // Windows does not replace an existing destination with rename. Keep
        // the old journal recoverable while switching names, and restore it if
        // publishing the new bytes fails; never delete safety metadata first.
        if !path.is_file() {
            return Err(error.into());
        }
        let previous = parent.join(format!(
            ".{}.previous",
            path.file_name().unwrap().to_string_lossy()
        ));
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(path, &previous)?;
        if let Err(error) = fs::rename(&tmp, path) {
            let _ = fs::rename(&previous, path);
            return Err(error.into());
        }
        let _ = fs::remove_file(previous);
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(MaintenanceError::UnsafePath(
                source_path.display().to_string(),
            ));
        }
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn digest_tree(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(root.join(&relative))?);
        digest.update([0]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn collect_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy() == "identity.json" {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(MaintenanceError::UnsafePath(path.display().to_string()));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else {
            files.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

/// Return the deterministic digest used by [`RuntimeIdentity`].
pub fn artifact_digest(path: &Path) -> Result<String> {
    digest_tree(path)
}

/// Write the identity sidecar consumed by fixture promotion.
pub fn write_runtime_identity(path: &Path, identity: &RuntimeIdentity) -> Result<()> {
    atomic_write(
        &path.join("identity.json"),
        &serde_json::to_vec_pretty(identity)?,
    )
}

/// Read the identity sidecar consumed by fixture promotion and CLI control.
pub fn read_runtime_identity(path: &Path) -> Result<RuntimeIdentity> {
    read_identity(path)
}

fn read_identity(path: &Path) -> Result<RuntimeIdentity> {
    let identity = path.join("identity.json");
    if !identity.is_file() {
        return Err(MaintenanceError::MissingPath(identity));
    }
    Ok(serde_json::from_slice(&fs::read(identity)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn request(trigger_id: &str) -> ProposalRequest {
        ProposalRequest {
            proposal_id: format!("proposal-{trigger_id}"),
            lineage_id: "lineage-1".into(),
            trigger_id: trigger_id.into(),
            actor: "operator".into(),
            source_revision: "HEAD".into(),
            previous_revision: "parent".into(),
            scope: vec!["crates/farseer-store".into()],
        }
    }

    #[test]
    fn proposal_trigger_deduplicates_and_bounds_attempts() {
        let mut ledger = ProposalLedger::default();
        assert_eq!(
            ledger.begin(request("failure-1")).unwrap(),
            BeginProposal::Created("proposal-failure-1".into())
        );
        assert_eq!(
            ledger.begin(request("failure-1")).unwrap(),
            BeginProposal::Existing("proposal-failure-1".into())
        );
        ledger
            .record_attempt(
                "proposal-failure-1",
                MaintenanceAttempt {
                    number: 1,
                    started_ts: 1,
                    finished_ts: Some(2),
                    evidence: vec![],
                },
            )
            .unwrap();
        assert!(matches!(
            ledger.record_attempt(
                "proposal-failure-1",
                MaintenanceAttempt {
                    number: 2,
                    started_ts: 3,
                    finished_ts: None,
                    evidence: vec![]
                }
            ),
            Err(MaintenanceError::AttemptLimit)
        ));
        assert!(ledger.suppresses("lineage-1", "self-event"));
    }

    #[test]
    fn proposal_ledger_survives_reload_without_retrying_trigger() {
        let fixture = tempdir().unwrap();
        let path = fixture.path().join("proposal.json");
        let mut ledger = ProposalLedger::default();
        ledger.begin(request("failure-2")).unwrap();
        ledger.save(&path).unwrap();
        let mut reloaded = ProposalLedger::load(&path).unwrap();
        assert_eq!(
            reloaded.begin(request("failure-2")).unwrap(),
            BeginProposal::Existing("proposal-failure-2".into())
        );
    }

    #[test]
    fn incomplete_backup_bundle_is_rejected() {
        let fixture = tempdir().unwrap();
        let backup = fixture.path().join("backup");
        fs::create_dir(&backup).unwrap();

        assert!(matches!(
            validate_backup_bundle(&backup, crate::STORE_FORMAT_VERSION),
            Err(MaintenanceError::MissingPath(path)) if path == backup.join(crate::BACKUP_DATABASE)
        ));
    }

    #[test]
    fn fixture_promotion_stages_checks_health_and_rolls_back() {
        let fixture = tempdir().unwrap();
        let candidate = fixture.path().join("candidate-source");
        let active = fixture.path().join("active");
        fs::create_dir_all(&candidate).unwrap();
        fs::create_dir_all(&active).unwrap();
        fs::write(candidate.join("runtime.bin"), b"candidate").unwrap();
        fs::write(active.join("runtime.bin"), b"previous").unwrap();
        let previous_digest = digest_tree(&active).unwrap();
        let candidate_digest = digest_tree(&candidate).unwrap();
        let previous = RuntimeIdentity {
            version: "1".into(),
            artifact_digest: previous_digest,
            schema_version: 1,
        };
        let candidate_identity = RuntimeIdentity {
            version: "2".into(),
            artifact_digest: candidate_digest,
            schema_version: 1,
        };
        fs::write(
            active.join("identity.json"),
            serde_json::to_vec(&previous).unwrap(),
        )
        .unwrap();
        fs::write(
            candidate.join("identity.json"),
            serde_json::to_vec(&candidate_identity).unwrap(),
        )
        .unwrap();
        let previous = RuntimeIdentity {
            version: "1".into(),
            artifact_digest: digest_tree(&active).unwrap(),
            schema_version: 1,
        };
        let candidate_identity = RuntimeIdentity {
            version: "2".into(),
            artifact_digest: digest_tree(&candidate).unwrap(),
            schema_version: 1,
        };
        let backup = fixture.path().join("backup");
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join(crate::BACKUP_DATABASE), b"sqlite snapshot").unwrap();
        fs::write(
            backup.join(crate::BACKUP_MANIFEST),
            serde_json::to_vec(&crate::BackupManifest {
                format_version: crate::BACKUP_FORMAT_VERSION,
                schema_version: crate::STORE_FORMAT_VERSION,
                attachments: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        let plan = PromotionPlan {
            candidate: candidate_identity.clone(),
            previous: previous.clone(),
            backup: BackupIdentity {
                runtime: previous.clone(),
                path: backup,
            },
            candidate_path: candidate,
        };
        let controller = FixturePromotion::new(fixture.path());
        let mut journal = controller.stage(plan).unwrap();
        assert!(matches!(
            controller.record_drain(&mut journal, 1),
            Err(MaintenanceError::DrainIncomplete(1))
        ));
        controller.record_drain(&mut journal, 0).unwrap();
        controller.record_backup(&mut journal).unwrap();
        controller.activate(&mut journal).unwrap();
        controller
            .verify_health(
                &mut journal,
                HealthObservation {
                    runtime: candidate_identity,
                    authenticated: true,
                    smoke_ok: true,
                },
            )
            .unwrap();
        controller.rollback(&mut journal).unwrap();
        assert_eq!(
            fs::read(fixture.path().join("active/runtime.bin")).unwrap(),
            b"previous"
        );
        assert_eq!(journal.phase, PromotionPhase::RolledBack);
    }
}
