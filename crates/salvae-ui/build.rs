//! Embed the Windows executable icon (so Explorer/taskbar show the app icon
//! instead of a blank square). No-op on non-Windows / if a resource compiler
//! isn't available.
fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed app icon: {e}");
        }
    }
}
