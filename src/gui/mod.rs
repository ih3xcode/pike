mod lifecycle;
mod persist;
mod screens;
mod state;
mod theme;
mod widgets;

use eframe::egui;

use persist::{config_from_saved, SavedConfig};
use screens::config::draw_config;
use screens::running::{draw_running, RunningState};
use screens::starting::{draw_starting, StartingState};
use state::{new_config_state, ConfigState};
use theme::apply_theme;

use crate::update;

enum Screen {
    Config(ConfigState),
    Starting(StartingState),
    Running(RunningState),
}

enum Action {
    None,
    StartingReady,
    StopServer(Box<SavedConfig>),
    BeginUpdate,
}

pub struct MindeliverApp {
    screen: Screen,
    runtime: tokio::runtime::Runtime,
    update_check: Option<tokio::task::JoinHandle<Option<update::UpdateInfo>>>,
    update_apply:
        Option<tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>>,
    update_info: Option<update::UpdateInfo>,
    update_status: Option<String>,
}

impl MindeliverApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let update_check =
            Some(runtime.spawn(async { update::check_for_update().await.ok().flatten() }));
        Self {
            screen: Screen::Config(new_config_state()),
            runtime,
            update_check,
            update_apply: None,
            update_info: None,
            update_status: None,
        }
    }
}

impl eframe::App for MindeliverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background update check
        if let Some(handle) = &self.update_check {
            if handle.is_finished() {
                let handle = self.update_check.take().unwrap();
                if let Ok(info) = self.runtime.block_on(handle) {
                    self.update_info = info;
                }
            }
        }

        // Poll update apply result
        if let Some(handle) = &self.update_apply {
            if handle.is_finished() {
                let handle = self.update_apply.take().unwrap();
                match self.runtime.block_on(handle) {
                    Ok(Ok(())) => {
                        if let Some(info) = &self.update_info {
                            self.update_status = Some(format!(
                                "Updated to pike {}! Restart to use the new version.",
                                info.latest_version
                            ));
                        }
                        self.update_info = None;
                    }
                    Ok(Err(e)) => {
                        self.update_status = Some(format!("Update failed: {e}"));
                    }
                    Err(e) => {
                        self.update_status = Some(format!("Update failed: {e}"));
                    }
                }
            }
        }

        let mut action = Action::None;

        match &mut self.screen {
            Screen::Config(config) => {
                draw_config(
                    ctx,
                    config,
                    self.update_info.as_ref(),
                    self.update_status.as_deref(),
                );
                if config.update_requested {
                    config.update_requested = false;
                    action = Action::BeginUpdate;
                }
            }
            Screen::Starting(starting) => {
                draw_starting(ctx);
                if starting.init_handle.is_finished() {
                    action = Action::StartingReady;
                }
            }
            Screen::Running(running) => {
                action = draw_running(ctx, running);
            }
        }

        match action {
            Action::None => {}
            Action::StartingReady => {
                self.finish_starting();
            }
            Action::StopServer(saved) => {
                self.screen = Screen::Config(config_from_saved(*saved));
            }
            Action::BeginUpdate => {
                if let Some(info) = self.update_info.clone() {
                    self.update_status =
                        Some(format!("Downloading pike {}...", info.latest_version));
                    self.update_apply = Some(
                        self.runtime
                            .spawn(async move { update::apply_update(&info).await }),
                    );
                }
            }
        }

        if let Screen::Config(config) = &self.screen
            && config.start_requested
        {
            self.begin_starting();
        }
    }
}

pub fn run_gui() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 620.0])
            .with_resizable(false)
            .with_maximize_button(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "pike",
        options,
        Box::new(|cc| Ok(Box::new(MindeliverApp::new(cc)))),
    )
    .expect("Failed to run GUI");
}
