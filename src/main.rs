#[cfg(windows)]
fn main() {
    wmic_compat::platform::windows::entry::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("wmic requires Windows and the native WMI service.");
    std::process::exit(1);
}
