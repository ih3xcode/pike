#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod cli;
mod common;
mod config;
mod falcon;
mod gui;
mod scripts;
mod sensors;
mod server;
mod service;
mod update;

fn main() {
    // Re-attach to parent console so CLI output works even with windows_subsystem = "windows".
    // Succeeds when launched from a terminal; silently fails on double-click (no parent console).
    #[cfg(target_os = "windows")]
    {
        unsafe extern "system" {
            fn AttachConsole(id: u32) -> i32;
        }
        unsafe {
            AttachConsole(0xFFFFFFFF);
        } // ATTACH_PARENT_PROCESS
    }

    cli::run();
}
