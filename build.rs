//! Embeds the icon into the executable as a Win32 resource.
//!
//! Explorer shows the icon with the lowest resource ID, so this is all it takes
//! for the exe, shortcuts and pinned entries to pick it up. It has nothing to
//! do with the window itself, which is a WS_EX_TOOLWINDOW and never appears in
//! the taskbar or Alt-Tab.

fn main() {
    println!("cargo::rerun-if-changed=assets/topclock.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Missing icon or no resource compiler on the box is not worth failing a
    // build over -- you just get the default exe icon.
    if !std::path::Path::new("assets/topclock.ico").exists() {
        println!("cargo::warning=assets/topclock.ico not found; building without an icon");
        return;
    }
    if let Err(e) = winresource::WindowsResource::new()
        .set_icon("assets/topclock.ico")
        .compile()
    {
        println!("cargo::warning=could not embed the icon: {e}");
    }
}
