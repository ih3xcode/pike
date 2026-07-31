//! Точки входу команд. Тут живе розбір аргументів і складання застосунку;
//! самі підсистеми одна про одну не знають.

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
    /// Запустити GUI (типово, якщо команду не вказано)
    Gui,
    /// Запустити HTTP-сервер розгортання
    Serve(crate::config::ServeArgs),
    /// Перевірити наявність оновлень і за потреби встановити
    Update {
        #[arg(long)]
        apply: bool,
    },
    /// Встановити pike як systemd-сервіс (Linux, потребує root)
    ServiceInstall,
    /// Видалити systemd-сервіс
    ServiceUninstall {
        /// Також видалити конфіг, кеш і системного користувача
        #[arg(long)]
        purge: bool,
    },
}

/// Розбирає аргументи й віддає керування потрібній команді.
/// Виходить з процесу з ненульовим кодом, якщо команда провалилась.
pub fn run() {
    let cli = Cli::parse();

    let result = match cli.command {
        None | Some(Command::Gui) => {
            crate::gui::run_gui();
            Ok(())
        }
        Some(Command::Update { apply }) => update::run_update_command(apply),
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
        // Стара форма `pike --sensor x --cid y` більше не підтримується
        assert!(Cli::try_parse_from(["pike", "--sensor", "x.deb"]).is_err());
    }

    #[test]
    fn update_subcommand_still_parses() {
        let cli = Cli::parse_from(["pike", "update", "--apply"]);
        assert!(matches!(cli.command, Some(Command::Update { apply: true })));
    }
}
