#[cfg(not(target_os = "macos"))]
compile_error!("Peep currently supports macOS only.");

#[cfg(target_os = "macos")]
fn main() {
    gpui::Application::new().run(|_cx| {
        // Application lifecycle only; views and windows will be added later.
    });
}
