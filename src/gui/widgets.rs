use eframe::egui;
use std::time::Instant;

use super::theme::*;

pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(22.0).color(TEXT).strong());
}

pub fn subtitle(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(13.0).color(TEXT_DIM));
}

pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .color(TEXT_DIM)
            .strong(),
    );
}

pub fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(
        egui::RichText::new(text)
            .color(egui::Color32::WHITE)
            .strong(),
    )
    .fill(ACCENT)
    .stroke(egui::Stroke::new(1.0, ACCENT_HOVER))
    .corner_radius(egui::CornerRadius::same(6));
    ui.add(btn)
}

pub fn card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL_BG)
        .corner_radius(egui::CornerRadius::same(8))
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

pub fn copy_button(
    ui: &mut egui::Ui,
    id: &str,
    text_to_copy: &str,
    copied_at: &mut std::collections::HashMap<String, Instant>,
) {
    let recently_copied = copied_at
        .get(id)
        .map(|t| t.elapsed().as_secs_f32() < 1.5)
        .unwrap_or(false);

    if recently_copied {
        ui.label(egui::RichText::new("Copied!").size(11.0).color(GREEN));
    } else if ui.small_button("Copy").clicked() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text_to_copy);
        }
        copied_at.insert(id.to_string(), Instant::now());
    }
}

pub fn cmd_box(
    ui: &mut egui::Ui,
    label: &str,
    cmd: &str,
    copied_at: &mut std::collections::HashMap<String, Instant>,
) {
    section_label(ui, label);
    ui.add_space(2.0);
    egui::Frame::new()
        .fill(FIELD_BG)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                    |ui| {
                        ui.set_width(ui.available_width() - 55.0);
                        ui.label(
                            egui::RichText::new(cmd)
                                .monospace()
                                .size(12.0)
                                .color(TEXT),
                        );
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    copy_button(ui, label, cmd, copied_at);
                });
            });
        });
}

pub fn tab_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let text = egui::RichText::new(label)
        .size(12.0)
        .color(if selected {
            egui::Color32::WHITE
        } else {
            TEXT_DIM
        })
        .strong();
    let btn = egui::Button::new(text)
        .fill(if selected { ACCENT } else { FIELD_BG })
        .corner_radius(egui::CornerRadius::same(4))
        .stroke(if selected {
            egui::Stroke::NONE
        } else {
            egui::Stroke::new(1.0, BORDER)
        });
    ui.add(btn)
}
