use std::path::PathBuf;

/// Аргументи `pike serve`. Усі опційні — дефолти живуть у
/// [`crate::config::resolve`], інакше значення за замовчуванням від clap
/// стало б невідрізнюваним від явно переданого прапорця і завжди
/// перебивало б конфіг.
#[derive(Debug, Default, clap::Args)]
pub struct ServeArgs {
    /// Шлях до конфіг-файлу (типово /etc/pike/pike.toml, якщо існує)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Адреса, на якій слухати
    #[arg(long)]
    pub bind: Option<String>,

    /// HTTP-порт
    #[arg(long)]
    pub port: Option<u16>,

    /// Адреса, яку показувати в ванлайнерах (автовизначення, якщо не задано)
    #[arg(long)]
    pub addr: Option<String>,

    /// Зовнішній URL (за reverse proxy), напр. https://pike.lab.local
    #[arg(long)]
    pub public_url: Option<String>,

    /// Токен автентифікації (генерується, якщо не задано)
    #[arg(long, env = "PIKE_TOKEN", hide_env_values = true)]
    pub token: Option<String>,

    /// Таймаут у хвилинах, 0 = без обмеження
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Ліміт завантажень сенсорів, 0 = без обмеження
    #[arg(long)]
    pub max_downloads: Option<u32>,

    /// CrowdStrike API Client ID
    #[arg(long, env = "PIKE_CLIENT_ID", hide_env_values = true)]
    pub client_id: Option<String>,

    /// CrowdStrike API Client Secret
    #[arg(long, env = "PIKE_CLIENT_SECRET", hide_env_values = true)]
    pub client_secret: Option<String>,

    /// Хмара: us-1, us-2, eu-1, us-gov-1, us-gov-2
    #[arg(long)]
    pub cloud: Option<String>,

    /// CrowdStrike Customer ID
    #[arg(long, env = "PIKE_CID", hide_env_values = true)]
    pub cid: Option<String>,

    /// Каталог кешу сенсорів
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Час життя списку сенсорів у хвилинах
    #[arg(long)]
    pub metadata_ttl: Option<u64>,

    /// Максимальний розмір кешу в байтах
    #[arg(long)]
    pub cache_max_bytes: Option<u64>,

    /// Теги групування, через кому
    #[arg(long)]
    pub tags: Option<String>,

    /// Не додавати типовий тег deployment/pike
    #[arg(long)]
    pub no_default_tag: bool,

    /// Локальний файл сенсора; можна вказати кілька разів
    #[arg(long = "sensor")]
    pub sensors: Vec<PathBuf>,

    /// Вимкнути автентифікацію за токеном
    #[arg(long)]
    pub no_auth: bool,
}

impl ServeArgs {
    /// Розбір лише для тестів — дає доступ до логіки clap разом із env.
    #[cfg(test)]
    pub fn parse_from_args(argv: &[&str]) -> Self {
        // `derive(Args)` не дає CommandFactory — команду будуємо самі
        use clap::{Args, FromArgMatches};
        let cmd = <Self as Args>::augment_args(clap::Command::new("pike"));
        let matches = cmd.get_matches_from(argv);
        Self::from_arg_matches(&matches).expect("valid args")
    }
}
