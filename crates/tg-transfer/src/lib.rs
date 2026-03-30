mod mock;
mod planner;
mod runtime;

pub use mock::{
    MemoryDownloadBackend, MemoryDownloadSink, MemoryUploadSource, RecordingUploadBackend,
};
pub use planner::TransferPlanner;
pub use runtime::{
    DownloadBackend, DownloadRequest, DownloadSink, ParallelDownloadEngine, ParallelUploadEngine,
    TransferReport, TransferRuntimeError, UploadBackend, UploadPart, UploadSource,
};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tg_core::{
        AccelerationPolicy, AccountTier, AppConfigHints, TransferDirection, TransferFeatureConfig,
        TransferJob,
    };

    use crate::{
        MemoryDownloadBackend, MemoryDownloadSink, MemoryUploadSource, ParallelDownloadEngine,
        ParallelUploadEngine, RecordingUploadBackend, TransferPlanner,
    };

    #[tokio::test]
    async fn planner_uses_parallel_upload_for_big_files() {
        let planner = TransferPlanner::new(
            TransferFeatureConfig {
                policy: AccelerationPolicy::Aggressive,
                ..Default::default()
            },
            AppConfigHints::default(),
        );
        let job = TransferJob::new(
            "archive.bin",
            128 * 1024 * 1024,
            TransferDirection::Upload,
            AccountTier::Premium,
        );

        let plan = planner.plan(&job).expect("plan");

        assert!(plan.big_file_api);
        assert!(plan.worker_count >= 4);
        assert_eq!(plan.part_size, 512 * 1024);
    }

    #[tokio::test]
    async fn upload_engine_sends_all_bytes() {
        let planner = TransferPlanner::default();
        let job = TransferJob::new(
            "video.mp4",
            12 * 1024 * 1024,
            TransferDirection::Upload,
            AccountTier::Free,
        );
        let plan = planner.plan(&job).expect("plan");

        let data: Vec<u8> = (0..job.total_bytes as usize)
            .map(|index| (index % 251) as u8)
            .collect();
        let source = Arc::new(MemoryUploadSource::new(data.clone()));
        let backend = Arc::new(RecordingUploadBackend::default());

        let report = ParallelUploadEngine::new(plan)
            .execute(source, backend.clone(), job.total_bytes)
            .await
            .expect("upload");

        assert_eq!(report.total_bytes, job.total_bytes);
        assert_eq!(backend.assembled().await, data);
    }

    #[tokio::test]
    async fn download_engine_writes_all_bytes() {
        let planner = TransferPlanner::new(
            TransferFeatureConfig {
                policy: AccelerationPolicy::Aggressive,
                ..Default::default()
            },
            AppConfigHints::default(),
        );
        let job = TransferJob::new(
            "dataset.zip",
            6 * 1024 * 1024 + 333,
            TransferDirection::Download,
            AccountTier::Premium,
        );
        let plan = planner.plan(&job).expect("plan");

        let data: Vec<u8> = (0..job.total_bytes as usize)
            .map(|index| (index % 239) as u8)
            .collect();
        let backend = Arc::new(MemoryDownloadBackend::new(data.clone()));
        let sink = Arc::new(MemoryDownloadSink::new(job.total_bytes as usize));

        let report = ParallelDownloadEngine::new(plan)
            .execute(backend, sink.clone(), job.total_bytes)
            .await
            .expect("download");

        assert_eq!(report.total_bytes, job.total_bytes);
        assert_eq!(sink.bytes().await, data);
    }
}
