/// The IPv4 addresses worth offering as "the server's address".
/// The order is stable: real interfaces first, then 0.0.0.0 and loopback.
pub fn detect_available_addrs() -> Vec<(String, String)> {
    let mut addrs = Vec::new();
    if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in interfaces {
            if ip.is_ipv4() && !ip.is_loopback() {
                let label = format!("{} ({})", ip, name);
                addrs.push((label, ip.to_string()));
            }
        }
    }
    addrs.sort_by(|a, b| a.1.cmp(&b.1));
    addrs.dedup_by(|a, b| a.1 == b.1);
    addrs.push(("0.0.0.0 (all interfaces)".into(), "0.0.0.0".into()));
    addrs.push(("127.0.0.1 (loopback)".into(), "127.0.0.1".into()));
    addrs
}

pub fn detect_addr() -> String {
    match local_ip_address::local_ip() {
        Ok(ip) => ip.to_string(),
        Err(_) => {
            eprintln!("WARNING: Could not detect local IP, falling back to 127.0.0.1");
            "127.0.0.1".to_string()
        }
    }
}
