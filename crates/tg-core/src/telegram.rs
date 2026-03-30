use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;

pub const SMALL_UPLOAD_CUTOFF: u64 = 10 * MIB;
pub const RECOMMENDED_UPLOAD_CHUNK_SIZE: usize = 512 * 1024;
pub const MAX_DOWNLOAD_CHUNK_SIZE: usize = 1024 * 1024;
pub const RECOMMENDED_DOWNLOAD_CHUNK_SIZE: usize = 1024 * 1024;
pub const PRECISE_DOWNLOAD_ALIGNMENT: usize = 1024;
pub const LEGACY_DEFAULT_MAX_UPLOAD_PARTS: usize = 4_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccountTier {
    #[default]
    Free,
    Premium,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AppConfigHints {
    pub upload_max_fileparts_default: Option<usize>,
    pub upload_max_fileparts_premium: Option<usize>,
    pub small_queue_max_active_operations_count: Option<usize>,
    pub large_queue_max_active_operations_count: Option<usize>,
}

impl AppConfigHints {
    pub fn upload_part_limit(&self, tier: AccountTier) -> usize {
        match tier {
            AccountTier::Free => self
                .upload_max_fileparts_default
                .unwrap_or(LEGACY_DEFAULT_MAX_UPLOAD_PARTS),
            AccountTier::Premium => self
                .upload_max_fileparts_premium
                .or(self.upload_max_fileparts_default)
                .unwrap_or(LEGACY_DEFAULT_MAX_UPLOAD_PARTS),
        }
    }

    pub fn queue_limit_for(&self, total_bytes: u64) -> Option<usize> {
        if total_bytes <= 64 * MIB {
            self.small_queue_max_active_operations_count
        } else {
            self.large_queue_max_active_operations_count
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TelegramRuleError {
    #[error("part size must be greater than 0")]
    ZeroPartSize,
    #[error("part size {part_size} must be divisible by 1024")]
    BadUploadAlignment { part_size: usize },
    #[error("part size {part_size} must evenly divide 524288")]
    BadUploadGranularity { part_size: usize },
    #[error(
        "upload would require {total_parts} parts, which exceeds the allowed maximum of {part_limit}"
    )]
    TooManyUploadParts {
        total_parts: usize,
        part_limit: usize,
    },
    #[error("download limit must be greater than 0")]
    ZeroDownloadLimit,
    #[error("download limit {limit} exceeds the maximum precise chunk size of {max_limit}")]
    DownloadLimitTooLarge { limit: usize, max_limit: usize },
    #[error("download offset {offset} must be divisible by {alignment}")]
    BadDownloadOffsetAlignment { offset: u64, alignment: usize },
    #[error("download limit {limit} must be divisible by {alignment}")]
    BadDownloadLimitAlignment { limit: usize, alignment: usize },
    #[error(
        "precise download request starting at {offset} with length {limit} crosses a 1 MB window"
    )]
    PreciseWindowCrossed { offset: u64, limit: usize },
}

pub fn validate_upload_part_size(part_size: usize) -> Result<(), TelegramRuleError> {
    if part_size == 0 {
        return Err(TelegramRuleError::ZeroPartSize);
    }
    if !part_size.is_multiple_of(1024) {
        return Err(TelegramRuleError::BadUploadAlignment { part_size });
    }
    if !RECOMMENDED_UPLOAD_CHUNK_SIZE.is_multiple_of(part_size) {
        return Err(TelegramRuleError::BadUploadGranularity { part_size });
    }
    Ok(())
}

pub fn validate_upload(
    total_bytes: u64,
    part_size: usize,
    part_limit: usize,
) -> Result<usize, TelegramRuleError> {
    validate_upload_part_size(part_size)?;

    let total_parts = total_bytes.div_ceil(part_size as u64) as usize;
    if total_parts > part_limit {
        return Err(TelegramRuleError::TooManyUploadParts {
            total_parts,
            part_limit,
        });
    }

    Ok(total_parts)
}

pub fn validate_download_request(
    offset: u64,
    limit: usize,
    precise: bool,
) -> Result<(), TelegramRuleError> {
    if limit == 0 {
        return Err(TelegramRuleError::ZeroDownloadLimit);
    }
    if limit > MAX_DOWNLOAD_CHUNK_SIZE {
        return Err(TelegramRuleError::DownloadLimitTooLarge {
            limit,
            max_limit: MAX_DOWNLOAD_CHUNK_SIZE,
        });
    }

    let alignment = if precise {
        PRECISE_DOWNLOAD_ALIGNMENT
    } else {
        4 * 1024
    };

    if !offset.is_multiple_of(alignment as u64) {
        return Err(TelegramRuleError::BadDownloadOffsetAlignment { offset, alignment });
    }
    if !limit.is_multiple_of(alignment) {
        return Err(TelegramRuleError::BadDownloadLimitAlignment { limit, alignment });
    }

    if precise {
        let chunk_window = MAX_DOWNLOAD_CHUNK_SIZE as u64;
        let end = offset + limit as u64 - 1;
        if offset / chunk_window != end / chunk_window {
            return Err(TelegramRuleError::PreciseWindowCrossed { offset, limit });
        }
    }

    Ok(())
}
