use serde::{Deserialize, Serialize};

use tg_core::{
    AppConfigHints, TelegramRuleError, TransferFeatureConfig, TransferJob, TransferPlan,
};
use tg_tdlib::{
    TdlibBootstrapConfig, TdlibBootstrapPreview, TdlibProbe, TdlibRuntimeError,
    TdlibTransferPreview, TransferIntegration, bootstrap_preview, probe_tdjson, transfer_preview,
};
use tg_transfer::TransferPlanner;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Official,
    Inspiration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLink {
    pub name: String,
    pub url: String,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectManifest {
    pub project_name: String,
    pub summary: String,
    pub platforms: Vec<String>,
    pub sources: Vec<SourceLink>,
    pub tdlib_summary: String,
    pub transfer_notes: Vec<String>,
}

pub struct RewriteService {
    planner: TransferPlanner,
    tdlib: TdlibBootstrapConfig,
    transfer: TransferIntegration,
}

impl Default for RewriteService {
    fn default() -> Self {
        Self::new(
            TransferFeatureConfig::default(),
            AppConfigHints::default(),
            TdlibBootstrapConfig::default(),
            TransferIntegration::default(),
        )
    }
}

impl RewriteService {
    pub fn new(
        feature: TransferFeatureConfig,
        hints: AppConfigHints,
        tdlib: TdlibBootstrapConfig,
        transfer: TransferIntegration,
    ) -> Self {
        Self {
            planner: TransferPlanner::new(feature, hints),
            tdlib,
            transfer,
        }
    }

    pub fn plan_transfer(&self, job: &TransferJob) -> Result<TransferPlan, TelegramRuleError> {
        self.planner.plan(job)
    }

    pub fn tdlib_config(&self) -> &TdlibBootstrapConfig {
        &self.tdlib
    }

    pub fn probe_tdlib(&self) -> Result<TdlibProbe, TdlibRuntimeError> {
        probe_tdjson(&self.tdlib)
    }

    pub fn tdlib_bootstrap_preview(&self) -> TdlibBootstrapPreview {
        bootstrap_preview(&self.tdlib)
    }

    pub fn tdlib_transfer_preview(
        &self,
        local_path: impl Into<String>,
        chat_id: i64,
        file_id: i32,
    ) -> TdlibTransferPreview {
        transfer_preview(local_path, chat_id, file_id)
    }

    pub fn manifest(&self) -> ProjectManifest {
        ProjectManifest {
            project_name: "Unofficial TG RS".to_string(),
            summary: "Rust rewrite workspace for a Telegram client with transfer acceleration as a dedicated feature."
                .to_string(),
            platforms: vec![
                "Windows".to_string(),
                "macOS".to_string(),
                "Linux".to_string(),
                "Android".to_string(),
                "iOS".to_string(),
            ],
            sources: vec![
                SourceLink {
                    name: "Telegram Desktop".to_string(),
                    url: "https://github.com/telegramdesktop/tdesktop".to_string(),
                    kind: SourceKind::Official,
                },
                SourceLink {
                    name: "Telegram Android".to_string(),
                    url: "https://github.com/DrKLO/Telegram".to_string(),
                    kind: SourceKind::Official,
                },
                SourceLink {
                    name: "Telegram iOS".to_string(),
                    url: "https://github.com/TelegramMessenger/Telegram-iOS".to_string(),
                    kind: SourceKind::Official,
                },
                SourceLink {
                    name: "TDLib".to_string(),
                    url: "https://github.com/tdlib/td".to_string(),
                    kind: SourceKind::Official,
                },
                SourceLink {
                    name: "Telegram file API".to_string(),
                    url: "https://core.telegram.org/api/files".to_string(),
                    kind: SourceKind::Official,
                },
                SourceLink {
                    name: "grammers".to_string(),
                    url: "https://github.com/Lonami/grammers".to_string(),
                    kind: SourceKind::Inspiration,
                },
                SourceLink {
                    name: "gotd/td".to_string(),
                    url: "https://github.com/gotd/td".to_string(),
                    kind: SourceKind::Inspiration,
                },
                SourceLink {
                    name: "tg-upload".to_string(),
                    url: "https://github.com/TheCaduceus/tg-upload".to_string(),
                    kind: SourceKind::Inspiration,
                },
            ],
            tdlib_summary: self.tdlib.summary(),
            transfer_notes: self.transfer.notes.clone(),
        }
    }
}
