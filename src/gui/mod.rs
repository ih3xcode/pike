mod config;
mod lifecycle;
mod running;
mod starting;
mod theme;
mod widgets;

use eframe::egui;

use config::{config_from_saved, draw_config, new_config_state, ConfigState, SavedConfig};
use running::{draw_running, RunningState};
use starting::{draw_starting, StartingState};
use theme::apply_theme;

enum Screen {
    Config(ConfigState),
    Starting(StartingState),
    Running(RunningState),
}

enum Action {
    None,
    StartingReady,
    StopServer(Box<SavedConfig>),
}

pub struct MindeliverApp {
    screen: Screen,
    runtime: tokio::runtime::Runtime,
}

impl MindeliverApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        Self {
            screen: Screen::Config(new_config_state()),
            runtime: tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"),
        }
    }
}

impl eframe::App for MindeliverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut action = Action::None;

        match &mut self.screen {
            Screen::Config(config) => draw_config(ctx, config),
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
            .with_min_inner_size([420.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "pike",
        options,
        Box::new(|cc| Ok(Box::new(MindeliverApp::new(cc)))),
    )
    .expect("Failed to run GUI");
}
