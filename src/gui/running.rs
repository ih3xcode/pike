use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use crate::types::{AppState, HostStatus};

use super::config::SavedConfig;
use super::theme::*;
use super::widgets::*;
use super::Action;

pub(super) struct RunningState {
    pub app_state: Arc<AppState>,
    pub started_at: Instant,
    pub timeout_minutes: u64,
    pub server_handle: tokio::task::JoinHandle<()>,
    pub shutdown_triggered: bool,
    pub copied_at: std::collections::HashMap<String, Instant>,
    pub saved_config: SavedConfig,
    pub has_api: bool,
}

pub(super) fn draw_running(ctx: &egui::Context, running: &mut RunningState) -> Action {
    let state = &running.app_state;
    let elapsed = running.started_at.elapsed();
    let timeout_secs = running.timeout_minutes * 60;
    let remaining = timeout_secs.saturating_sub(elapsed.as_secs());
    let remaining_min = remaining / 60;
    let remaining_sec = remaining % 60;
    let downloads = state.download_count.load(Ordering::Relaxed);

    let base_url = state.base_url();

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24)))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("●").size(16.0).color(GREEN));
                    heading(ui, "Server running");
                });

                let subtitle_text = if running.has_api {
                    format!(
                        "Listening on {}:{} | API connected",
                        state.addr, state.port
                    )
                } else {
                    format!("Listening on {}:{}", state.addr, state.port)
                };
                subtitle(ui, &subtitle_text);
                ui.add_space(16.0);

                // Always show commands (callback-based flow)
                let linux_cmd = format!("curl -fsS {}/lin | sudo bash", base_url);
                cmd_box(ui, "LINUX", &linux_cmd, &mut running.copied_at);
                ui.add_space(8.0);

                let win_cmd = format!("irm {}/win | iex", base_url);
                cmd_box(
                    ui,
                    "WINDOWS (run as Administrator)",
                    &win_cmd,
                    &mut running.copied_at,
                );
                ui.add_space(8.0);

                ui.add_space(4.0);

                // Stats card
                card_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            section_label(ui, "DOWNLOADS");
                            let dl_text = if state.max_downloads > 0 {
                                format!("{} / {}", downloads, state.max_downloads)
                            } else {
                                format!("{}", downloads)
                            };
                            ui.label(
                                egui::RichText::new(dl_text)
                                    .size(20.0)
                                    .color(TEXT)
                                    .strong(),
                            );
                        });

                        ui.add_space(40.0);

                        ui.vertical(|ui| {
                            section_label(ui, "TIME LEFT");
                            let time_color = if remaining < 60 { ACCENT } else { TEXT };
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:02}:{:02}",
                                    remaining_min, remaining_sec
                                ))
                                .size(20.0)
                                .color(time_color)
                                .strong(),
                            );
                        });

                        ui.add_space(40.0);

                        ui.vertical(|ui| {
                            let label = if running.has_api {
                                "SOURCE"
                            } else {
                                "SENSORS"
                            };
                            section_label(ui, label);
                            let text = if running.has_api { "API" } else { "Local" };
                            ui.label(
                                egui::RichText::new(text)
                                    .size(20.0)
                                    .color(TEXT)
                                    .strong(),
                            );
                        });
                    });
                });

                ui.add_space(12.0);

                // Hosts tracking card
                let hosts = state.hosts.lock().unwrap_or_else(|e| e.into_inner()).clone();
                card_frame(ui, |ui| {
                    section_label(ui, "HOSTS");
                    ui.add_space(4.0);

                    if hosts.is_empty() {
                        ui.label(
                            egui::RichText::new("No hosts have connected yet.")
                                .size(12.0)
                                .color(TEXT_DIM),
                        );
                    } else {
                        // Table header
                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(24.0, 16.0), |ui| {
                                ui.label(
                                    egui::RichText::new("").size(11.0).color(TEXT_DIM),
                                );
                            });
                            ui.allocate_ui(egui::vec2(140.0, 16.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Hostname")
                                        .size(11.0)
                                        .color(TEXT_DIM)
                                        .strong(),
                                );
                            });
                            ui.allocate_ui(egui::vec2(60.0, 16.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Platform")
                                        .size(11.0)
                                        .color(TEXT_DIM)
                                        .strong(),
                                );
                            });
                            ui.label(
                                egui::RichText::new("Time")
                                    .size(11.0)
                                    .color(TEXT_DIM)
                                    .strong(),
                            );
                        });

                        ui.add_space(2.0);

                        for host in hosts.iter().rev() {
                            let (icon, icon_color) = match &host.status {
                                HostStatus::Registered => {
                                    (host.status.icon(), TEXT_DIM)
                                }
                                HostStatus::SensorReady => {
                                    (host.status.icon(), YELLOW)
                                }
                                HostStatus::Installed => {
                                    (host.status.icon(), GREEN)
                                }
                                HostStatus::Failed(_) => {
                                    (host.status.icon(), ACCENT)
                                }
                            };

                            ui.horizontal(|ui| {
                                ui.allocate_ui(egui::vec2(24.0, 16.0), |ui| {
                                    ui.label(
                                        egui::RichText::new(icon)
                                            .size(13.0)
                                            .color(icon_color),
                                    );
                                });
                                ui.allocate_ui(egui::vec2(140.0, 16.0), |ui| {
                                    ui.label(
                                        egui::RichText::new(&host.hostname)
                                            .size(12.0)
                                            .color(TEXT),
                                    );
                                });
                                ui.allocate_ui(egui::vec2(60.0, 16.0), |ui| {
                                    ui.label(
                                        egui::RichText::new(&host.platform)
                                            .size(12.0)
                                            .color(TEXT_DIM),
                                    );
                                });
                                ui.label(
                                    egui::RichText::new(
                                        host.time.format("%H:%M:%S").to_string(),
                                    )
                                    .size(12.0)
                                    .color(TEXT_DIM),
                                );
                            });
                        }
                    }
                });

                ui.add_space(16.0);

                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let btn = egui::Button::new(
                        egui::RichText::new("Stop Server").color(TEXT).strong(),
                    )
                    .fill(FIELD_BG)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(egui::CornerRadius::same(6));

                    if ui.add(btn).clicked() {
                        eprintln!("[gui] Stop button clicked, shutting down server...");
                        running.app_state.shutdown_notify.notify_one();
                        running.shutdown_triggered = true;
                    }
                });
            });
        });

    ctx.request_repaint_after(std::time::Duration::from_secs(1));

    if running.shutdown_triggered || running.server_handle.is_finished() {
        return Action::StopServer(Box::new(running.saved_config.clone()));
    }

    Action::None
}
