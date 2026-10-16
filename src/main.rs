#[cfg(windows)]
fn main() {
    wmic_compat::platform::windows::entry::run()
}
#[cfg(target_os = "macos")]
fn main() {
    wmic_compat::platform::macos::entry::run()
}
#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("wmic requires Windows and the native WMI service.");
    std::process::exit(1);
}
