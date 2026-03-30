use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccelerationPolicy {
    Conservative,
    Balanced,
    Aggressive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferFeatureConfig {
    pub enabled: bool,
    pub policy: AccelerationPolicy,
    pub min_workers: usize,
    pub max_workers: usize,
    pub verify_download_hashes: bool,
    pub allow_cdn_redirects: bool,
    pub checkpoint_every_parts: usize,
}

impl Default for TransferFeatureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            policy: AccelerationPolicy::Balanced,
            min_workers: 1,
            max_workers: 8,
            verify_download_hashes: true,
            allow_cdn_redirects: true,
            checkpoint_every_parts: 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferJob {
    pub id: Uuid,
    pub file_name: String,
    pub total_bytes: u64,
    pub direction: TransferDirection,
    pub tier: crate::telegram::AccountTier,
    pub ui_visible: bool,
}

impl TransferJob {
    pub fn new(
        file_name: impl Into<String>,
        total_bytes: u64,
        direction: TransferDirection,
        tier: crate::telegram::AccountTier,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            file_name: file_name.into(),
            total_bytes,
            direction,
            tier,
            ui_visible: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferPlan {
    pub job_id: Uuid,
    pub direction: TransferDirection,
    pub part_size: usize,
    pub total_parts: usize,
    pub worker_count: usize,
    pub parallel_file_budget: usize,
    pub big_file_api: bool,
    pub needs_md5_for_finalize: bool,
    pub verify_hashes: bool,
    pub allow_cdn: bool,
    pub notes: Vec<String>,
}

impl TransferPlan {
    pub fn estimated_in_flight_bytes(&self) -> u64 {
        self.worker_count as u64 * self.part_size as u64
    }
}
