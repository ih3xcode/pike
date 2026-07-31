//! Command entry points. Argument parsing and application assembly live
//! here; the subsystems themselves know nothing about each other.

mod banner;
mod serve;
mod update;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pike", about = "CrowdStrike sensor deployment tool", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the GUI (the default when no subcommand is given)
    Gui,
    /// Run the deployment HTTP server
    Serve(crate::config::ServeArgs),
    /// Check for updates and optionally install them
    Update {
        #[arg(long)]
        apply: bool,
        /// Restart pike.service after an update was actually installed
        /// (used by the auto-update timer)
        #[arg(long, requires = "apply")]
        restart_service: bool,
    },
    /// Install pike as a systemd service (Linux, requires root)
    ServiceInstall,
    /// Remove the systemd service
    ServiceUninstall {
        /// Also remove the config, the cache and the system user
        #[arg(long)]
        purge: bool,
    },
}

/// Parses the arguments and hands control to the matching command.
/// Exits the process with a non-zero code when the command fails.
pub fn run() {
    let cli = Cli::parse();

    let result = match cli.command {
        None | Some(Command::Gui) => {
            crate::gui::run_gui();
            Ok(())
        }
        Some(Command::Update {
            apply,
            restart_service,
        }) => update::run_update_command(apply, restart_service),
        Some(Command::Serve(args)) => serve::run_serve(args),
        Some(Command::ServiceInstall) => crate::service::install(),
        Some(Command::ServiceUninstall { purge }) => crate::service::uninstall(purge),
    };

    if let Err(code) = result {
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_means_gui() {
        let cli = Cli::parse_from(["pike"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn gui_subcommand_parses() {
        let cli = Cli::parse_from(["pike", "gui"]);
        assert!(matches!(cli.command, Some(Command::Gui)));
    }

    #[test]
    fn serve_subcommand_parses_flags() {
        let cli = Cli::parse_from(["pike", "serve", "--port", "9090", "--no-auth"]);
        let Some(Command::Serve(args)) = cli.command else {
            panic!("expected serve");
        };
        assert_eq!(args.port, Some(9090));
        assert!(args.no_auth);
    }

    #[test]
    fn old_top_level_flags_are_rejected() {
        // The old `pike --sensor x --cid y` form is no longer supported
        assert!(Cli::try_parse_from(["pike", "--sensor", "x.deb"]).is_err());
    }

    #[test]
    fn update_subcommand_still_parses() {
        let cli = Cli::parse_from(["pike", "update", "--apply"]);
        assert!(matches!(
            cli.command,
            Some(Command::Update {
                apply: true,
                restart_service: false
            })
        ));
    }

    #[test]
    fn update_restart_flag_parses_and_needs_apply() {
        let cli = Cli::parse_from(["pike", "update", "--apply", "--restart-service"]);
        assert!(matches!(
            cli.command,
            Some(Command::Update {
                restart_service: true,
                ..
            })
        ));
        // Restarting without installing anything makes no sense
        assert!(Cli::try_parse_from(["pike", "update", "--restart-service"]).is_err());
    }
}
