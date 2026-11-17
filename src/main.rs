#[cfg(not(target_os = "macos"))]
compile_error!("Peep currently supports macOS only.");

#[cfg(target_os = "macos")]
fn main() {
    peep::app::run();
}
