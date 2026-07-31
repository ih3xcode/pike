pub struct UnitParams {
    pub exec_path: String,
    pub config_path: String,
    pub cache_dir: String,
    pub user: String,
    pub port: u16,
}

pub fn main_unit(p: &UnitParams) -> String {
    // Під непривілейованим користувачем прив'язка до порту < 1024
    // неможлива без цієї capability — інакше сервіс просто не підніметься.
    let caps = if p.port < 1024 {
        "AmbientCapabilities=CAP_NET_BIND_SERVICE\n\
         CapabilityBoundingSet=CAP_NET_BIND_SERVICE\n"
    } else {
        ""
    };

    format!(
        "[Unit]\n\
         Description=Pike — CrowdStrike sensor deployment server\n\
         Documentation=https://github.com/ih3xcode/pike\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=exec\n\
         User={user}\n\
         Group={user}\n\
         ExecStart={exec} serve --config {config}\n\
         Restart=always\n\
         RestartSec=5\n\
         NoNewPrivileges=true\n\
         ProtectSystem=strict\n\
         ProtectHome=true\n\
         PrivateTmp=true\n\
         PrivateDevices=true\n\
         ProtectKernelTunables=true\n\
         ProtectControlGroups=true\n\
         RestrictAddressFamilies=AF_INET AF_INET6\n\
         RestrictNamespaces=true\n\
         LockPersonality=true\n\
         SystemCallFilter=@system-service\n\
         SystemCallArchitectures=native\n\
         {caps}\
         ReadWritePaths={cache}\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        user = p.user,
        exec = p.exec_path,
        config = p.config_path,
        cache = p.cache_dir,
        caps = caps,
    )
}

pub fn update_unit(exec_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Pike self-update\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         User=root\n\
         ExecStart={exec_path} update --apply\n\
         ExecStartPost=/bin/systemctl try-restart pike.service\n"
    )
}

pub fn update_timer() -> String {
    "[Unit]\n\
     Description=Weekly pike update check\n\
     \n\
     [Timer]\n\
     OnCalendar=Sun 04:00\n\
     RandomizedDelaySec=1h\n\
     Persistent=true\n\
     \n\
     [Install]\n\
     WantedBy=timers.target\n"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(port: u16) -> UnitParams {
        UnitParams {
            exec_path: "/usr/local/bin/pike".into(),
            config_path: "/etc/pike/pike.toml".into(),
            cache_dir: "/var/cache/pike".into(),
            user: "pike".into(),
            port,
        }
    }

    #[test]
    fn main_unit_has_exec_and_restart() {
        let unit = main_unit(&params(8080));
        assert!(unit.contains("ExecStart=/usr/local/bin/pike serve --config /etc/pike/pike.toml"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("User=pike"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn main_unit_is_hardened() {
        let unit = main_unit(&params(8080));
        for directive in [
            "NoNewPrivileges=true",
            "ProtectSystem=strict",
            "ProtectHome=true",
            "PrivateTmp=true",
            "RestrictAddressFamilies=AF_INET AF_INET6",
            "SystemCallFilter=@system-service",
            "LockPersonality=true",
        ] {
            assert!(unit.contains(directive), "missing {directive}");
        }
    }

    #[test]
    fn cache_dir_is_the_only_writable_path() {
        let unit = main_unit(&params(8080));
        assert!(unit.contains("ReadWritePaths=/var/cache/pike"));
        // Сервіс не має права переписувати власний бінар
        assert!(!unit.contains("ReadWritePaths=/usr/local/bin"));
    }

    #[test]
    fn privileged_port_gets_capability() {
        let unit = main_unit(&params(80));
        assert!(unit.contains("AmbientCapabilities=CAP_NET_BIND_SERVICE"));
        assert!(unit.contains("CapabilityBoundingSet=CAP_NET_BIND_SERVICE"));
    }

    #[test]
    fn unprivileged_port_gets_no_capability() {
        let unit = main_unit(&params(8080));
        assert!(!unit.contains("CAP_NET_BIND_SERVICE"));
    }

    #[test]
    fn update_unit_runs_as_root_and_restarts_service() {
        let unit = update_unit("/usr/local/bin/pike");
        assert!(unit.contains("Type=oneshot"));
        assert!(unit.contains("User=root"));
        assert!(unit.contains("ExecStart=/usr/local/bin/pike update --apply"));
        assert!(unit.contains("ExecStartPost=/bin/systemctl try-restart pike.service"));
    }

    #[test]
    fn timer_is_weekly_with_jitter() {
        let timer = update_timer();
        assert!(timer.contains("OnCalendar=Sun 04:00"));
        assert!(timer.contains("RandomizedDelaySec=1h"));
        assert!(timer.contains("WantedBy=timers.target"));
    }
}
