use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand, ValueEnum};
use tg_app::RewriteService;
use tg_core::{
    AccelerationPolicy, AccountTier, AppConfigHints, TransferDirection, TransferFeatureConfig,
    TransferJob,
};
use tg_tdlib::TdlibBootstrapConfig;
use tg_transfer::{
    MemoryDownloadBackend, MemoryDownloadSink, MemoryUploadSource, ParallelDownloadEngine,
    ParallelUploadEngine, RecordingUploadBackend, TransferPlanner,
};

const MAX_SIMULATION_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "tg-cli",
    about = "Rust rewrite companion CLI for planning, TDLib probing, and simulating Telegram transfer acceleration"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Manifest,
    TdlibProbe {
        #[arg(long)]
        tdjson: Option<String>,
    },
    BridgeDownload {
        #[arg(long)]
        tdjson: Option<String>,
        #[arg(long)]
        file_id: i32,
        #[arg(long, default_value_t = 0)]
        chat_id: i64,
        #[arg(long, default_value_t = 0)]
        message_id: i64,
        #[arg(long)]
        size: u64,
        #[arg(long, default_value = "download.bin")]
        name: String,
        #[arg(long, value_enum, default_value = "balanced")]
        policy: PolicyArg,
        #[arg(long, default_value_t = false)]
        premium: bool,
    },
    BridgeUpload {
        #[arg(long)]
        tdjson: Option<String>,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        chat_id: i64,
        #[arg(long, value_enum, default_value = "balanced")]
        policy: PolicyArg,
        #[arg(long, default_value_t = false)]
        premium: bool,
    },
    Plan {
        #[arg(long, value_enum)]
        direction: DirectionArg,
        #[arg(long)]
        size: u64,
        #[arg(long, default_value = "payload.bin")]
        name: String,
        #[arg(long, value_enum, default_value = "balanced")]
        policy: PolicyArg,
        #[arg(long, default_value_t = false)]
        premium: bool,
        #[arg(long)]
        small_queue_limit: Option<usize>,
        #[arg(long)]
        large_queue_limit: Option<usize>,
    },
    Simulate {
        #[arg(long, value_enum)]
        direction: DirectionArg,
        #[arg(long)]
        size: u64,
        #[arg(long, default_value = "payload.bin")]
        name: String,
        #[arg(long, value_enum, default_value = "balanced")]
        policy: PolicyArg,
        #[arg(long, default_value_t = false)]
        premium: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DirectionArg {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PolicyArg {
    Conservative,
    Balanced,
    Aggressive,
}

impl From<DirectionArg> for TransferDirection {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Upload => TransferDirection::Upload,
            DirectionArg::Download => TransferDirection::Download,
        }
    }
}

impl From<PolicyArg> for AccelerationPolicy {
    fn from(value: PolicyArg) -> Self {
        match value {
            PolicyArg::Conservative => AccelerationPolicy::Conservative,
            PolicyArg::Balanced => AccelerationPolicy::Balanced,
            PolicyArg::Aggressive => AccelerationPolicy::Aggressive,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Manifest => {
            let manifest = RewriteService::default().manifest();
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Command::TdlibProbe { tdjson } => {
            let service = service_from(PolicyArg::Balanced, tdjson);

            let result = service.probe_tdlib()?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::BridgeDownload {
            tdjson,
            file_id,
            chat_id,
            message_id,
            size,
            name,
            policy,
            premium,
        } => {
            let service = service_from(policy, tdjson);
            let transfer_job = job(name, size, DirectionArg::Download, premium);
            let result =
                service.bridge_logged_in_download(&transfer_job, file_id, chat_id, message_id)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::BridgeUpload {
            tdjson,
            path,
            chat_id,
            policy,
            premium,
        } => {
            let metadata = std::fs::metadata(&path)?;
            let service = service_from(policy, tdjson);
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("upload.bin")
                .to_string();
            let transfer_job = TransferJob::new(
                file_name,
                metadata.len(),
                TransferDirection::Upload,
                if premium {
                    AccountTier::Premium
                } else {
                    AccountTier::Free
                },
            );
            let result = service.bridge_logged_in_upload(&path, &transfer_job, chat_id)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Plan {
            direction,
            size,
            name,
            policy,
            premium,
            small_queue_limit,
            large_queue_limit,
        } => {
            let planner = planner_from(policy, small_queue_limit, large_queue_limit);
            let plan = planner.plan(&job(name, size, direction, premium))?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Command::Simulate {
            direction,
            size,
            name,
            policy,
            premium,
        } => {
            if size > MAX_SIMULATION_BYTES {
                return Err(format!(
                    "simulation size {} exceeds the safety cap of {} bytes",
                    size, MAX_SIMULATION_BYTES
                )
                .into());
            }

            let planner = planner_from(policy, None, None);
            let transfer_job = job(name, size, direction, premium);
            let plan = planner.plan(&transfer_job)?;
            let data = patterned_bytes(size);

            let report = match TransferDirection::from(direction) {
                TransferDirection::Upload => {
                    let source = Arc::new(MemoryUploadSource::new(data));
                    let backend = Arc::new(RecordingUploadBackend::default());
                    let report = ParallelUploadEngine::new(plan)
                        .execute(source, backend, transfer_job.total_bytes)
                        .await?;
                    serde_json::to_value(report)?
                }
                TransferDirection::Download => {
                    let backend = Arc::new(MemoryDownloadBackend::new(data));
                    let sink = Arc::new(MemoryDownloadSink::new(size as usize));
                    let report = ParallelDownloadEngine::new(plan)
                        .execute(backend, sink, transfer_job.total_bytes)
                        .await?;
                    serde_json::to_value(report)?
                }
            };

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }

    Ok(())
}

fn planner_from(
    policy: PolicyArg,
    small_queue_limit: Option<usize>,
    large_queue_limit: Option<usize>,
) -> TransferPlanner {
    TransferPlanner::new(
        TransferFeatureConfig {
            policy: policy.into(),
            ..Default::default()
        },
        AppConfigHints {
            small_queue_max_active_operations_count: small_queue_limit,
            large_queue_max_active_operations_count: large_queue_limit,
            ..Default::default()
        },
    )
}

fn service_from(policy: PolicyArg, tdjson: Option<String>) -> RewriteService {
    RewriteService::new(
        TransferFeatureConfig {
            policy: policy.into(),
            ..Default::default()
        },
        AppConfigHints::default(),
        TdlibBootstrapConfig {
            custom_tdjson_path: tdjson.map(Into::into),
            ..Default::default()
        },
        Default::default(),
    )
}

fn job(name: String, size: u64, direction: DirectionArg, premium: bool) -> TransferJob {
    TransferJob::new(
        name,
        size,
        direction.into(),
        if premium {
            AccountTier::Premium
        } else {
            AccountTier::Free
        },
    )
}

fn patterned_bytes(size: u64) -> Vec<u8> {
    (0..size as usize)
        .map(|index| (index % 251) as u8)
        .collect()
}
