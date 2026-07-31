use eframe::egui;

use crate::gui::state::{ConfigState, CLOUD_OPTIONS};
use crate::gui::theme::*;
use crate::gui::widgets::*;
use crate::sensors::loading::detect_sensor_type;
use crate::sensors::matching::check_sensor_filename;
use crate::update;

pub(crate) fn draw_config(
    ctx: &egui::Context,
    config: &mut ConfigState,
    update_info: Option<&update::UpdateInfo>,
    update_status: Option<&str>,
) {
    // Handle file dialog results
    {
        let mut pending = config.pending_files.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.is_empty() {
            for p in pending.drain(..) {
                if !config.sensor_paths.contains(&p) {
                    config.sensor_paths.push(p);
                }
            }
            config.file_dialog_open = false;
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(24)))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    heading(ui, "pike");
                    ui.label(
                        egui::RichText::new(format!("v{}", update::CURRENT_VERSION))
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                });

                // Update banner
                if let Some(status) = update_status {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(PANEL_BG)
                        .corner_radius(egui::CornerRadius::same(6))
                        .stroke(egui::Stroke::new(1.0, GREEN))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(status)
                                    .size(12.0)
                                    .color(GREEN),
                            );
                        });
                } else if let Some(info) = update_info {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(PANEL_BG)
                        .corner_radius(egui::CornerRadius::same(6))
                        .stroke(egui::Stroke::new(1.0, GREEN))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Update available: v{} \u{2192} v{}",
                                        info.current_version, info.latest_version
                                    ))
                                    .size(12.0)
                                    .color(GREEN),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if accent_button(ui, "Update").clicked() {
                                            config.update_requested = true;
                                        }
                                    },
                                );
                            });
                        });
                }

                ui.add_space(16.0);

                // === Tabbed card (API / Files) ===
                card_frame(ui, |ui| {
                    // Tab bar
                    ui.horizontal(|ui| {
                        if tab_button(ui, "API", config.config_tab == 0).clicked() {
                            config.config_tab = 0;
                        }
                        if tab_button(ui, "Files", config.config_tab == 1).clicked() {
                            config.config_tab = 1;
                        }
                    });
                    ui.add_space(8.0);

                    match config.config_tab {
                        0 => draw_api_tab(ui, config),
                        1 => draw_files_tab(ui, ctx, config),
                        _ => {}
                    }
                });

                ui.add_space(12.0);

                // === Sensor card ===
                card_frame(ui, |ui| {
                    section_label(ui, "SENSOR");
                    ui.add_space(4.0);

                    let label_width = 100.0;

                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                            ui.label(
                                egui::RichText::new("Tags").size(13.0).color(TEXT),
                            );
                        });
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::TextEdit::singleline(&mut config.tags)
                                .hint_text("comma-separated, max 256 chars"),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(label_width, 20.0), |_ui| {});
                        let hint = if config.no_default_tag {
                            "sensor grouping tags"
                        } else {
                            "deployment/pike added automatically"
                        };
                        ui.label(
                            egui::RichText::new(hint)
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    });
                });

                ui.add_space(12.0);

                // === Network card ===
                card_frame(ui, |ui| {
                    section_label(ui, "NETWORK");
                    ui.add_space(4.0);

                    let label_width = 100.0;

                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                            ui.label(
                                egui::RichText::new("Address").size(13.0).color(TEXT),
                            );
                        });
                        let current_label =
                            &config.available_addrs[config.selected_addr_idx].0;
                        egui::ComboBox::from_id_salt("addr")
                            .selected_text(current_label.as_str())
                            .width(ui.available_width() - 8.0)
                            .show_ui(ui, |ui| {
                                for (i, (label, _ip)) in
                                    config.available_addrs.iter().enumerate()
                                {
                                    ui.selectable_value(
                                        &mut config.selected_addr_idx,
                                        i,
                                        label.as_str(),
                                    );
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                            ui.label(
                                egui::RichText::new("Port").size(13.0).color(TEXT),
                            );
                        });
                        ui.add_sized(
                            [80.0, 24.0],
                            egui::TextEdit::singleline(&mut config.port),
                        );
                    });

                    ui.add_space(4.0);

                    let was_custom = config.custom_url_enabled;
                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                            ui.label(
                                egui::RichText::new("Custom URL")
                                    .size(13.0)
                                    .color(TEXT),
                            );
                        });
                        ui.checkbox(&mut config.custom_url_enabled, "");
                        ui.label(
                            egui::RichText::new("Override public base URL")
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    });

                    // Populate default URL on toggle
                    if config.custom_url_enabled && !was_custom {
                        let ip =
                            &config.available_addrs[config.selected_addr_idx].1;
                        config.custom_url =
                            format!("http://{}:{}", ip, config.port);
                    }

                    if config.custom_url_enabled {
                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(label_width, 20.0), |_ui| {});
                            ui.add_sized(
                                [ui.available_width(), 24.0],
                                egui::TextEdit::singleline(&mut config.custom_url)
                                    .hint_text("http://hostname:port"),
                            );
                        });
                    }
                });

                ui.add_space(8.0);

                // === Advanced (collapsible) ===
                let arrow = if config.show_advanced {
                    "▾"
                } else {
                    "▸"
                };
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} Advanced", arrow))
                                .size(12.0)
                                .color(TEXT_DIM)
                                .strong(),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .clicked()
                {
                    config.show_advanced = !config.show_advanced;
                }

                if config.show_advanced {
                    ui.add_space(4.0);
                    card_frame(ui, |ui| {
                        let label_width = 100.0;

                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Timeout")
                                        .size(13.0)
                                        .color(TEXT),
                                );
                            });
                            ui.add_sized(
                                [60.0, 24.0],
                                egui::TextEdit::singleline(&mut config.timeout),
                            );
                            ui.label(
                                egui::RichText::new("min").size(12.0).color(TEXT_DIM),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Max downloads")
                                        .size(13.0)
                                        .color(TEXT),
                                );
                            });
                            ui.add_sized(
                                [60.0, 24.0],
                                egui::TextEdit::singleline(&mut config.max_downloads),
                            );
                            ui.label(
                                egui::RichText::new("0 = unlimited")
                                    .size(11.0)
                                    .color(TEXT_DIM),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Token auth")
                                        .size(13.0)
                                        .color(TEXT),
                                );
                            });
                            ui.checkbox(&mut config.auth_enabled, "");
                            let hint = if config.auth_enabled {
                                "URLs require token"
                            } else {
                                "open access, no token"
                            };
                            ui.label(
                                egui::RichText::new(hint).size(11.0).color(TEXT_DIM),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
                                ui.label(
                                    egui::RichText::new("Default tag")
                                        .size(13.0)
                                        .color(TEXT),
                                );
                            });
                            let mut include_default = !config.no_default_tag;
                            if ui.checkbox(&mut include_default, "").changed() {
                                config.no_default_tag = !include_default;
                            }
                            ui.label(
                                egui::RichText::new("add deployment/pike tag")
                                    .size(11.0)
                                    .color(TEXT_DIM),
                            );
                        });
                    });
                }

                ui.add_space(12.0);

                // === Error message ===
                if let Some(err) = &config.error {
                    ui.label(egui::RichText::new(err).size(12.0).color(ACCENT));
                    ui.add_space(4.0);
                }

                // === Start button ===
                let has_api_creds = !config.client_id.trim().is_empty()
                    && !config.client_secret.trim().is_empty();
                let has_sensors_and_cid =
                    !config.sensor_paths.is_empty() && !config.cid.trim().is_empty();
                let can_start = has_sensors_and_cid || has_api_creds;

                ui.add_enabled_ui(can_start, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        if accent_button(ui, "Start Server").clicked() {
                            if config.port.parse::<u16>().is_err() {
                                config.error = Some("Invalid port number".into());
                            } else if config.timeout.parse::<u64>().is_err() {
                                config.error = Some("Invalid timeout".into());
                            } else if config.max_downloads.parse::<u32>().is_err() {
                                config.error = Some("Invalid max downloads".into());
                            } else {
                                config.error = None;
                                config.start_requested = true;
                            }
                        }
                    });
                });
            });
        });

    if config.file_dialog_open {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

fn draw_api_tab(ui: &mut egui::Ui, config: &mut ConfigState) {
    let label_width = 100.0;

    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
            ui.label(egui::RichText::new("Client ID").size(13.0).color(TEXT));
        });
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut config.client_id)
                .hint_text("API Client ID"),
        );
    });

    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
            ui.label(
                egui::RichText::new("Client Secret").size(13.0).color(TEXT),
            );
        });
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut config.client_secret)
                .password(true)
                .hint_text("API Client Secret"),
        );
    });

    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
            ui.label(egui::RichText::new("Cloud").size(13.0).color(TEXT));
        });
        egui::ComboBox::from_id_salt("cloud")
            .selected_text(CLOUD_OPTIONS[config.cloud_idx])
            .show_ui(ui, |ui| {
                for (i, opt) in CLOUD_OPTIONS.iter().enumerate() {
                    ui.selectable_value(&mut config.cloud_idx, i, *opt);
                }
            });
    });

    if !config.client_id.trim().is_empty() && !config.client_secret.trim().is_empty() {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(
                "CID and sensors will be fetched automatically from API",
            )
            .size(11.0)
            .color(TEXT_DIM)
            .italics(),
        );
    }
}

fn draw_files_tab(ui: &mut egui::Ui, ctx: &egui::Context, config: &mut ConfigState) {
    let label_width = 100.0;

    // CID field (moved here from settings)
    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
            ui.label(egui::RichText::new("CID").size(13.0).color(TEXT));
        });
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut config.cid)
                .hint_text("Customer ID"),
        );
    });

    ui.add_space(4.0);

    // Sensor files
    ui.horizontal(|ui| {
        section_label(ui, "SENSORS");
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let btn_enabled = !config.file_dialog_open;
                ui.add_enabled_ui(btn_enabled, |ui| {
                    if ui.small_button("+ Add files").clicked() {
                        config.file_dialog_open = true;
                        let pending = config.pending_files.clone();
                        let repaint_ctx = ctx.clone();
                        std::thread::spawn(move || {
                            if let Some(paths) = rfd::FileDialog::new()
                                .add_filter(
                                    "Sensor installers",
                                    &["deb", "rpm", "exe"],
                                )
                                .pick_files()
                            {
                                let mut lock = pending.lock().unwrap_or_else(|e| e.into_inner());
                                lock.extend(paths);
                            }
                            repaint_ctx.request_repaint();
                        });
                    }
                });
            },
        );
    });

    if config.file_dialog_open {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Waiting for file selection...")
                .size(12.0)
                .color(TEXT_DIM)
                .italics(),
        );
    } else if config.sensor_paths.is_empty() {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "No sensor files. Click \"+ Add files\" to select .deb, .rpm, or .exe.",
            )
            .size(12.0)
            .color(TEXT_DIM),
        );
    } else {
        ui.add_space(4.0);
        let mut to_remove = None;
        for (i, path) in config.sensor_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let ext = path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_uppercase();

                let badge_color = match ext.as_str() {
                    "DEB" | "RPM" => egui::Color32::from_rgb(60, 140, 200),
                    "EXE" => egui::Color32::from_rgb(180, 130, 60),
                    _ => TEXT_DIM,
                };

                let badge = egui::RichText::new(format!(" {ext} "))
                    .size(10.0)
                    .color(egui::Color32::WHITE)
                    .strong()
                    .background_color(badge_color);
                ui.label(badge);
                ui.label(
                    egui::RichText::new(name.to_string())
                        .size(13.0)
                        .color(TEXT),
                );

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui.small_button("x").clicked() {
                            to_remove = Some(i);
                        }
                    },
                );
            });
        }
        if let Some(i) = to_remove {
            config.sensor_paths.remove(i);
        }

        // Warn about sensor files with unrecognizable filenames
        let warnings: Vec<String> = config
            .sensor_paths
            .iter()
            .filter_map(|p| {
                let sensor_type = detect_sensor_type(p)?;
                let filename = p.file_name()?.to_string_lossy().to_string();
                check_sensor_filename(&filename, sensor_type)
            })
            .collect();

        if !warnings.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(PANEL_BG)
                .corner_radius(egui::CornerRadius::same(6))
                .stroke(egui::Stroke::new(1.0, YELLOW))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for warning in &warnings {
                        ui.label(
                            egui::RichText::new(warning)
                                .size(11.0)
                                .color(YELLOW),
                        );
                    }
                });
        }
    }
}
