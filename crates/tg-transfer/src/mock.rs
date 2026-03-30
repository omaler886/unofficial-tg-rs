use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    DownloadBackend, DownloadRequest, DownloadSink, TransferRuntimeError, UploadBackend,
    UploadPart, UploadSource,
};

#[derive(Debug, Clone)]
pub struct MemoryUploadSource {
    bytes: std::sync::Arc<Vec<u8>>,
}

impl MemoryUploadSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: std::sync::Arc::new(bytes),
        }
    }
}

#[async_trait]
impl UploadSource for MemoryUploadSource {
    async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, TransferRuntimeError> {
        let offset = offset as usize;
        let end = offset.saturating_add(len);
        let bytes = self.bytes.get(offset..end).ok_or_else(|| {
            TransferRuntimeError::Message("upload source out of bounds".to_string())
        })?;
        Ok(bytes.to_vec())
    }
}

#[derive(Debug, Default)]
pub struct RecordingUploadBackend {
    parts: Mutex<Vec<UploadPart>>,
}

impl RecordingUploadBackend {
    pub async fn assembled(&self) -> Vec<u8> {
        let mut parts = self.parts.lock().await.clone();
        parts.sort_by_key(|part| part.offset);

        let mut bytes = Vec::new();
        for part in parts {
            bytes.extend(part.bytes);
        }
        bytes
    }
}

#[async_trait]
impl UploadBackend for RecordingUploadBackend {
    async fn upload_part(&self, part: UploadPart) -> Result<(), TransferRuntimeError> {
        self.parts.lock().await.push(part);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MemoryDownloadBackend {
    bytes: std::sync::Arc<Vec<u8>>,
}

impl MemoryDownloadBackend {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: std::sync::Arc::new(bytes),
        }
    }
}

#[async_trait]
impl DownloadBackend for MemoryDownloadBackend {
    async fn download_part(
        &self,
        request: DownloadRequest,
    ) -> Result<Vec<u8>, TransferRuntimeError> {
        let offset = request.offset as usize;
        let end = offset.saturating_add(request.write_len);
        let bytes = self.bytes.get(offset..end).ok_or_else(|| {
            TransferRuntimeError::Message("download source out of bounds".to_string())
        })?;
        Ok(bytes.to_vec())
    }
}

#[derive(Debug)]
pub struct MemoryDownloadSink {
    bytes: Mutex<Vec<u8>>,
}

impl MemoryDownloadSink {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: Mutex::new(vec![0; size]),
        }
    }

    pub async fn bytes(&self) -> Vec<u8> {
        self.bytes.lock().await.clone()
    }
}

#[async_trait]
impl DownloadSink for MemoryDownloadSink {
    async fn write_at(&self, offset: u64, bytes: &[u8]) -> Result<(), TransferRuntimeError> {
        let offset = offset as usize;
        let end = offset.saturating_add(bytes.len());
        let mut sink = self.bytes.lock().await;
        let target = sink.get_mut(offset..end).ok_or_else(|| {
            TransferRuntimeError::Message("download sink out of bounds".to_string())
        })?;
        target.copy_from_slice(bytes);
        Ok(())
    }
}
