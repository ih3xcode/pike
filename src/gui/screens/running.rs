use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use crate::gui::persist::SavedConfig;
use crate::gui::theme::*;
use crate::gui::widgets::*;
use crate::gui::Action;
use crate::server::state::{AppState, HostStatus};

pub(crate) struct RunningState {
    pub app_state: Arc<AppState>,
    pub started_at: Instant,
    pub timeout_minutes: u64,
    pub server_handle: tokio::task::JoinHandle<()>,
    pub shutdown_triggered: bool,
    pub copied_at: std::collections::HashMap<String, Instant>,
    pub saved_config: SavedConfig,
    pub has_api: bool,
}

pub(crate) fn draw_running(ctx: &egui::Context, running: &mut RunningState) -> Action {
    let state = &running.app_state;
    let elapsed = running.started_at.elapsed();
    let downloads = state.download_count.load(Ordering::Relaxed);
    let no_timeout = running.timeout_minutes == 0;

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
                            if no_timeout {
                                section_label(ui, "UPTIME");
                                let secs = elapsed.as_secs();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:02}:{:02}",
                                        secs / 60,
                                        secs % 60
                                    ))
                                    .size(20.0)
                                    .color(TEXT)
                                    .strong(),
                                );
                            } else {
                                section_label(ui, "TIME LEFT");
                                let timeout_secs = running.timeout_minutes * 60;
                                let remaining = timeout_secs.saturating_sub(elapsed.as_secs());
                                let time_color = if remaining < 60 { ACCENT } else { TEXT };
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:02}:{:02}",
                                        remaining / 60,
                                        remaining % 60
                                    ))
                                    .size(20.0)
                                    .color(time_color)
                                    .strong(),
                                );
                            }
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
                        let table_width = ui.available_width();

                        let col_gap = 12.0;
                        let col_status = 100.0;
                        let col_platform = 80.0;
                        let col_time = 80.0;
                        let col_hostname = table_width
                            - col_status
                            - col_platform
                            - col_time
                            - col_gap * 3.0;

                        for (i, host) in hosts.iter().rev().enumerate() {
                            let status_color = match &host.status {
                                HostStatus::Registered => TEXT_DIM,
                                HostStatus::SensorReady => YELLOW,
                                HostStatus::Installed => GREEN,
                                HostStatus::Failed(_) => ACCENT,
                            };

                            // Alternating row background
                            let row_rect = ui.available_rect_before_wrap();
                            let row_rect = egui::Rect::from_min_size(
                                row_rect.min,
                                egui::vec2(table_width, 22.0),
                            );
                            if i % 2 == 1 {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(2),
                                    ROW_ALT,
                                );
                            }

                            ui.horizontal(|ui| {
                                ui.set_height(22.0);
                                ui.set_width(table_width);
                                ui.spacing_mut().item_spacing.x = col_gap;

                                // Status: icon + text
                                ui.allocate_ui(egui::vec2(col_status, 22.0), |ui| {
                                    ui.add_space(8.0);
                                    ui.horizontal_centered(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} {}",
                                                host.status.icon(),
                                                host.status.text()
                                            ))
                                            .size(12.0)
                                            .color(status_color),
                                        );
                                    });
                                });

                                // Hostname
                                ui.allocate_ui(egui::vec2(col_hostname, 22.0), |ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&host.hostname)
                                                        .size(12.0)
                                                        .color(TEXT),
                                                );
                                            },
                                        );
                                    });
                                });

                                // Platform
                                ui.allocate_ui(egui::vec2(col_platform, 22.0), |ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&host.platform)
                                                        .size(12.0)
                                                        .color(TEXT_DIM),
                                                );
                                            },
                                        );
                                    });
                                });

                                // Time
                                ui.allocate_ui(egui::vec2(col_time, 22.0), |ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(
                                                        host.time
                                                            .format("%H:%M:%S")
                                                            .to_string(),
                                                    )
                                                    .size(12.0)
                                                    .color(TEXT_DIM),
                                                );
                                            },
                                        );
                                    });
                                });
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
