use eframe::egui::{self, RichText, TextEdit};
use tg_app::RewriteService;
use tg_core::{
    AccelerationPolicy, AccountTier, TransferDirection, TransferFeatureConfig, TransferJob,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Unofficial TG RS Desktop",
        options,
        Box::new(|_cc| Ok(Box::<DesktopApp>::default())),
    )
}

struct DesktopApp {
    service: RewriteService,
    file_name: String,
    total_bytes: String,
    premium: bool,
    direction: TransferDirection,
    policy: AccelerationPolicy,
    upload_path: String,
    chat_id: String,
    file_id: String,
    plan_json: String,
    bootstrap_json: String,
    transfer_json: String,
    probe_json: String,
    status_line: String,
}

impl Default for DesktopApp {
    fn default() -> Self {
        let service = RewriteService::default();
        let bootstrap_json = serde_json::to_string_pretty(&service.tdlib_bootstrap_preview())
            .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error));
        let transfer_json =
            serde_json::to_string_pretty(&service.tdlib_transfer_preview("sample.bin", 0, 0))
                .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error));

        Self {
            service,
            file_name: "payload.bin".to_string(),
            total_bytes: "8388608".to_string(),
            premium: false,
            direction: TransferDirection::Download,
            policy: AccelerationPolicy::Balanced,
            upload_path: "sample.bin".to_string(),
            chat_id: "0".to_string(),
            file_id: "0".to_string(),
            plan_json: String::new(),
            bootstrap_json,
            transfer_json,
            probe_json: String::new(),
            status_line: "Desktop shell ready. Add tdjson.dll to vendor/tdlib to enable probing."
                .to_string(),
        }
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(RichText::new("Unofficial TG RS").strong());
            ui.label(
                "Rust desktop shell for TDLib probing, request previewing, and transfer planning.",
            );
            ui.separator();

            ui.heading("TDLib");
            ui.label(self.service.tdlib_config().summary());
            if ui.button("Probe tdjson").clicked() {
                match self.service.probe_tdlib() {
                    Ok(probe) => {
                        self.probe_json = serde_json::to_string_pretty(&probe)
                            .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error));
                        self.status_line =
                            format!("TDLib loaded from {}", probe.library_path.display());
                    }
                    Err(error) => {
                        self.probe_json.clear();
                        self.status_line = format!("TDLib probe failed: {}", error);
                    }
                }
            }
            ui.label(&self.status_line);
            read_only_json(ui, "Probe Output", &mut self.probe_json);

            ui.separator();
            ui.heading("Transfer Planner");
            ui.horizontal(|ui| {
                ui.label("File name");
                ui.text_edit_singleline(&mut self.file_name);
                ui.label("Size");
                ui.text_edit_singleline(&mut self.total_bytes);
                ui.checkbox(&mut self.premium, "Premium");
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.direction, TransferDirection::Download, "Download");
                ui.selectable_value(&mut self.direction, TransferDirection::Upload, "Upload");
                ui.selectable_value(
                    &mut self.policy,
                    AccelerationPolicy::Conservative,
                    "Conservative",
                );
                ui.selectable_value(&mut self.policy, AccelerationPolicy::Balanced, "Balanced");
                ui.selectable_value(
                    &mut self.policy,
                    AccelerationPolicy::Aggressive,
                    "Aggressive",
                );
            });
            if ui.button("Generate Transfer Plan").clicked() {
                self.generate_plan();
            }
            read_only_json(ui, "Plan Output", &mut self.plan_json);

            ui.separator();
            ui.heading("TDLib Request Preview");
            ui.horizontal(|ui| {
                ui.label("Local path");
                ui.text_edit_singleline(&mut self.upload_path);
                ui.label("Chat ID");
                ui.text_edit_singleline(&mut self.chat_id);
                ui.label("File ID");
                ui.text_edit_singleline(&mut self.file_id);
            });
            if ui.button("Refresh TDLib Request Preview").clicked() {
                self.refresh_transfer_preview();
            }
            read_only_json(ui, "Bootstrap Requests", &mut self.bootstrap_json);
            read_only_json(ui, "Transfer Requests", &mut self.transfer_json);
        });
    }
}

impl DesktopApp {
    fn generate_plan(&mut self) {
        let Ok(total_bytes) = self.total_bytes.parse::<u64>() else {
            self.plan_json = "{\"error\":\"invalid size\"}".to_string();
            return;
        };

        let job = TransferJob::new(
            self.file_name.clone(),
            total_bytes,
            self.direction,
            if self.premium {
                AccountTier::Premium
            } else {
                AccountTier::Free
            },
        );

        let service = RewriteService::new(
            TransferFeatureConfig {
                policy: self.policy,
                ..Default::default()
            },
            Default::default(),
            self.service.tdlib_config().clone(),
            Default::default(),
        );

        self.plan_json = match service.plan_transfer(&job) {
            Ok(plan) => serde_json::to_string_pretty(&plan)
                .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error)),
            Err(error) => format!("{{\"error\":\"{}\"}}", error),
        };
    }

    fn refresh_transfer_preview(&mut self) {
        let chat_id = self.chat_id.parse::<i64>().unwrap_or_default();
        let file_id = self.file_id.parse::<i32>().unwrap_or_default();
        self.bootstrap_json = serde_json::to_string_pretty(&self.service.tdlib_bootstrap_preview())
            .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error));
        self.transfer_json = serde_json::to_string_pretty(&self.service.tdlib_transfer_preview(
            self.upload_path.clone(),
            chat_id,
            file_id,
        ))
        .unwrap_or_else(|error| format!("{{\"error\":\"{}\"}}", error));
    }
}

fn read_only_json(ui: &mut egui::Ui, title: &str, value: &mut String) {
    ui.label(RichText::new(title).strong());
    ui.add(
        TextEdit::multiline(value)
            .desired_rows(10)
            .font(egui::TextStyle::Monospace)
            .interactive(false),
    );
}
