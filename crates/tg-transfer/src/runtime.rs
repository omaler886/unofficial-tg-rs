use std::{
    cmp::min,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;
use tokio::task::JoinSet;

use tg_core::{TransferDirection, TransferPlan, validate_download_request};

#[derive(Debug, Error)]
pub enum TransferRuntimeError {
    #[error("transfer plan direction does not match the requested engine")]
    DirectionMismatch,
    #[error("transfer plan describes zero parts")]
    EmptyPlan,
    #[error("source returned {actual} bytes for part {part_index}, expected {expected}")]
    ShortRead {
        part_index: usize,
        expected: usize,
        actual: usize,
    },
    #[error("download backend returned {actual} bytes for part {part_index}, expected {expected}")]
    ShortDownload {
        part_index: usize,
        expected: usize,
        actual: usize,
    },
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Rules(#[from] tg_core::TelegramRuleError),
    #[error("worker task failed to join: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Debug, Clone)]
pub struct UploadPart {
    pub job_id: uuid::Uuid,
    pub part_index: usize,
    pub offset: u64,
    pub total_parts: usize,
    pub is_big_file: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub job_id: uuid::Uuid,
    pub part_index: usize,
    pub offset: u64,
    pub len: usize,
    pub write_len: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TransferReport {
    pub job_id: uuid::Uuid,
    pub total_bytes: u64,
    pub total_parts: usize,
    pub workers_used: usize,
    pub peak_in_flight_parts: usize,
    pub elapsed_ms: u128,
}

#[async_trait]
pub trait UploadSource: Send + Sync {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, TransferRuntimeError>;
}

#[async_trait]
pub trait UploadBackend: Send + Sync {
    async fn upload_part(&self, part: UploadPart) -> Result<(), TransferRuntimeError>;
}

#[async_trait]
pub trait DownloadBackend: Send + Sync {
    async fn download_part(
        &self,
        request: DownloadRequest,
    ) -> Result<Vec<u8>, TransferRuntimeError>;
}

#[async_trait]
pub trait DownloadSink: Send + Sync {
    async fn write_at(&self, offset: u64, bytes: &[u8]) -> Result<(), TransferRuntimeError>;
}

pub struct ParallelUploadEngine {
    plan: TransferPlan,
}

impl ParallelUploadEngine {
    pub fn new(plan: TransferPlan) -> Self {
        Self { plan }
    }

    pub async fn execute<S, B>(
        self,
        source: Arc<S>,
        backend: Arc<B>,
        total_bytes: u64,
    ) -> Result<TransferReport, TransferRuntimeError>
    where
        S: UploadSource + 'static,
        B: UploadBackend + 'static,
    {
        if self.plan.direction != TransferDirection::Upload {
            return Err(TransferRuntimeError::DirectionMismatch);
        }
        if self.plan.total_parts == 0 {
            return Err(TransferRuntimeError::EmptyPlan);
        }

        let started = Instant::now();
        let next_part = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let part_size = self.plan.part_size as u64;
        let total_parts = self.plan.total_parts;

        let mut tasks = JoinSet::new();
        for _ in 0..self.plan.worker_count {
            let source = Arc::clone(&source);
            let backend = Arc::clone(&backend);
            let next_part = Arc::clone(&next_part);
            let in_flight = Arc::clone(&in_flight);
            let peak = Arc::clone(&peak);
            let plan = self.plan.clone();

            tasks.spawn(async move {
                loop {
                    let index = next_part.fetch_add(1, Ordering::Relaxed);
                    if index >= total_parts {
                        return Ok::<(), TransferRuntimeError>(());
                    }

                    let offset = index as u64 * part_size;
                    let expected = min(part_size, total_bytes.saturating_sub(offset)) as usize;
                    let current = in_flight.fetch_add(1, Ordering::Relaxed) + 1;
                    peak.fetch_max(current, Ordering::Relaxed);

                    let bytes = source.read_at(offset, expected).await?;
                    if bytes.len() != expected {
                        in_flight.fetch_sub(1, Ordering::Relaxed);
                        return Err(TransferRuntimeError::ShortRead {
                            part_index: index,
                            expected,
                            actual: bytes.len(),
                        });
                    }

                    backend
                        .upload_part(UploadPart {
                            job_id: plan.job_id,
                            part_index: index,
                            offset,
                            total_parts: plan.total_parts,
                            is_big_file: plan.big_file_api,
                            bytes,
                        })
                        .await?;

                    in_flight.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result??;
        }

        Ok(TransferReport {
            job_id: self.plan.job_id,
            total_bytes,
            total_parts: self.plan.total_parts,
            workers_used: self.plan.worker_count,
            peak_in_flight_parts: peak.load(Ordering::Relaxed),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

pub struct ParallelDownloadEngine {
    plan: TransferPlan,
}

impl ParallelDownloadEngine {
    pub fn new(plan: TransferPlan) -> Self {
        Self { plan }
    }

    pub async fn execute<B, S>(
        self,
        backend: Arc<B>,
        sink: Arc<S>,
        total_bytes: u64,
    ) -> Result<TransferReport, TransferRuntimeError>
    where
        B: DownloadBackend + 'static,
        S: DownloadSink + 'static,
    {
        if self.plan.direction != TransferDirection::Download {
            return Err(TransferRuntimeError::DirectionMismatch);
        }
        if self.plan.total_parts == 0 {
            return Err(TransferRuntimeError::EmptyPlan);
        }

        let started = Instant::now();
        let next_part = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let part_size = self.plan.part_size as u64;
        let total_parts = self.plan.total_parts;

        let mut tasks = JoinSet::new();
        for _ in 0..self.plan.worker_count {
            let backend = Arc::clone(&backend);
            let sink = Arc::clone(&sink);
            let next_part = Arc::clone(&next_part);
            let in_flight = Arc::clone(&in_flight);
            let peak = Arc::clone(&peak);
            let plan = self.plan.clone();

            tasks.spawn(async move {
                loop {
                    let index = next_part.fetch_add(1, Ordering::Relaxed);
                    if index >= total_parts {
                        return Ok::<(), TransferRuntimeError>(());
                    }

                    let offset = index as u64 * part_size;
                    let write_len = min(part_size, total_bytes.saturating_sub(offset)) as usize;
                    let requested_len = align_precise_limit(write_len);
                    validate_download_request(offset, requested_len, true)?;

                    let current = in_flight.fetch_add(1, Ordering::Relaxed) + 1;
                    peak.fetch_max(current, Ordering::Relaxed);

                    let bytes = backend
                        .download_part(DownloadRequest {
                            job_id: plan.job_id,
                            part_index: index,
                            offset,
                            len: requested_len,
                            write_len,
                        })
                        .await?;
                    if bytes.len() != write_len {
                        in_flight.fetch_sub(1, Ordering::Relaxed);
                        return Err(TransferRuntimeError::ShortDownload {
                            part_index: index,
                            expected: write_len,
                            actual: bytes.len(),
                        });
                    }

                    sink.write_at(offset, &bytes).await?;
                    in_flight.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result??;
        }

        Ok(TransferReport {
            job_id: self.plan.job_id,
            total_bytes,
            total_parts: self.plan.total_parts,
            workers_used: self.plan.worker_count,
            peak_in_flight_parts: peak.load(Ordering::Relaxed),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

fn align_precise_limit(write_len: usize) -> usize {
    const ALIGNMENT: usize = 1024;
    write_len.div_ceil(ALIGNMENT) * ALIGNMENT
}
