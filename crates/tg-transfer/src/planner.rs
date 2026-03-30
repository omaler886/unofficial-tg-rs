use tg_core::{
    AccelerationPolicy, AppConfigHints, RECOMMENDED_DOWNLOAD_CHUNK_SIZE,
    RECOMMENDED_UPLOAD_CHUNK_SIZE, SMALL_UPLOAD_CUTOFF, TelegramRuleError, TransferDirection,
    TransferFeatureConfig, TransferJob, TransferPlan, validate_upload,
};

pub struct TransferPlanner {
    feature: TransferFeatureConfig,
    hints: AppConfigHints,
}

impl Default for TransferPlanner {
    fn default() -> Self {
        Self::new(TransferFeatureConfig::default(), AppConfigHints::default())
    }
}

impl TransferPlanner {
    pub fn new(feature: TransferFeatureConfig, hints: AppConfigHints) -> Self {
        Self { feature, hints }
    }

    pub fn plan(&self, job: &TransferJob) -> Result<TransferPlan, TelegramRuleError> {
        match job.direction {
            TransferDirection::Upload => self.plan_upload(job),
            TransferDirection::Download => self.plan_download(job),
        }
    }

    fn plan_upload(&self, job: &TransferJob) -> Result<TransferPlan, TelegramRuleError> {
        let part_size = RECOMMENDED_UPLOAD_CHUNK_SIZE;
        let total_parts = validate_upload(
            job.total_bytes,
            part_size,
            self.hints.upload_part_limit(job.tier),
        )?;
        let big_file_api = job.total_bytes > SMALL_UPLOAD_CUTOFF;
        let worker_count = if !big_file_api {
            1
        } else {
            self.clamp_workers(match self.feature.policy {
                AccelerationPolicy::Conservative => 2,
                AccelerationPolicy::Balanced => {
                    if job.total_bytes >= 512 * 1024 * 1024 {
                        4
                    } else {
                        3
                    }
                }
                AccelerationPolicy::Aggressive => {
                    if job.total_bytes >= 1024 * 1024 * 1024 {
                        8
                    } else if job.total_bytes >= 256 * 1024 * 1024 {
                        6
                    } else {
                        4
                    }
                }
            })
        };
        let parallel_file_budget = self
            .hints
            .queue_limit_for(job.total_bytes)
            .unwrap_or(if big_file_api { 1 } else { 2 });

        let mut notes = vec![
            "Upload uses official 512 KiB Telegram part sizing.".to_string(),
            "Big-file acceleration is modelled after grammers multi-worker saveBigFilePart."
                .to_string(),
            "gotd/td style worker limits are exposed as policy-driven heuristics.".to_string(),
        ];
        if self.hints.upload_max_fileparts_default.is_none()
            && self.hints.upload_max_fileparts_premium.is_none()
        {
            notes.push(
                "No live appConfig hints were provided, so upload part limits currently fall back to a conservative legacy default."
                    .to_string(),
            );
        }
        if !big_file_api {
            notes.push(
                "Small uploads stay single-threaded so final MD5 and server finalize flow remain simple."
                    .to_string(),
            );
        }

        Ok(TransferPlan {
            job_id: job.id,
            direction: job.direction,
            part_size,
            total_parts,
            worker_count,
            parallel_file_budget,
            big_file_api,
            needs_md5_for_finalize: !big_file_api,
            verify_hashes: false,
            allow_cdn: false,
            notes,
        })
    }

    fn plan_download(&self, job: &TransferJob) -> Result<TransferPlan, TelegramRuleError> {
        let part_size = match self.feature.policy {
            AccelerationPolicy::Conservative => 512 * 1024,
            AccelerationPolicy::Balanced | AccelerationPolicy::Aggressive => {
                RECOMMENDED_DOWNLOAD_CHUNK_SIZE
            }
        };
        let total_parts = job.total_bytes.div_ceil(part_size as u64) as usize;
        let worker_count = self.clamp_workers(match self.feature.policy {
            AccelerationPolicy::Conservative => 2,
            AccelerationPolicy::Balanced => {
                if job.total_bytes >= 256 * 1024 * 1024 {
                    4
                } else {
                    3
                }
            }
            AccelerationPolicy::Aggressive => {
                if job.total_bytes >= 1024 * 1024 * 1024 {
                    8
                } else if job.total_bytes >= 256 * 1024 * 1024 {
                    6
                } else {
                    4
                }
            }
        });
        let parallel_file_budget = self.hints.queue_limit_for(job.total_bytes).unwrap_or(
            if job.total_bytes > 64 * 1024 * 1024 {
                1
            } else {
                2
            },
        );

        let notes = vec![
            "Download uses precise range requests capped to 1 MiB windows.".to_string(),
            "Worker fan-out is inspired by grammers concurrent download_media_concurrent."
                .to_string(),
            "Writer-at style sinks follow the gotd/td downloader split between fetch and write loops."
                .to_string(),
        ];

        Ok(TransferPlan {
            job_id: job.id,
            direction: job.direction,
            part_size,
            total_parts,
            worker_count,
            parallel_file_budget,
            big_file_api: false,
            needs_md5_for_finalize: false,
            verify_hashes: self.feature.verify_download_hashes,
            allow_cdn: self.feature.allow_cdn_redirects,
            notes,
        })
    }

    fn clamp_workers(&self, workers: usize) -> usize {
        workers.clamp(self.feature.min_workers, self.feature.max_workers)
    }
}
