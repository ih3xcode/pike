use eframe::egui;

use crate::error::AppError;
use crate::falcon_api::FalconClient;
use crate::types::Sensor;

use super::config::SavedConfig;
use super::theme::*;

pub(super) struct StartingState {
    pub init_handle: tokio::task::JoinHandle<Result<InitResult, AppError>>,
    pub sensors: Vec<Sensor>,
    pub port: u16,
    pub timeout: u64,
    pub max_downloads: u32,
    pub cloud: Option<String>,
    pub cid_explicit: String,
    pub auth_enabled: bool,
    pub tags: Option<String>,
    pub addr: String,
    pub public_url: Option<String>,
    pub saved_config: SavedConfig,
}

pub(super) struct InitResult {
    pub falcon_client: Option<FalconClient>,
    pub api_cid: Option<String>,
}

pub(super) fn draw_starting(ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.spinner();
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Connecting...")
                        .size(18.0)
                        .color(TEXT)
                        .strong(),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Authenticating with CrowdStrike API")
                        .size(13.0)
                        .color(TEXT_DIM),
                );
            });
        });

    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}
