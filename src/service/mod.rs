pub mod units;

#[cfg(target_os = "linux")]
pub mod wizard;

#[cfg(not(target_os = "linux"))]
pub fn install() -> Result<(), i32> {
    eprintln!("ERROR: 'pike service-install' is supported on Linux with systemd only.");
    Err(1)
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall(_purge: bool) -> Result<(), i32> {
    eprintln!("ERROR: 'pike service-uninstall' is supported on Linux with systemd only.");
    Err(1)
}
