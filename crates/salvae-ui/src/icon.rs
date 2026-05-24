//! Salvaê's embedded images: the bot avatar to download, the app/window icon,
//! and the (transparent) welcome-screen logo.

/// The default bot avatar as PNG bytes (embedded at build time). Has a solid
/// background — meant for the Discord bot avatar (circle-masked there).
pub fn bot_icon_png() -> &'static [u8] {
    include_bytes!("../assets/bot-icon.png")
}

/// The app/window (taskbar) icon as PNG bytes.
pub fn app_icon_png() -> &'static [u8] {
    include_bytes!("../assets/app-icon.png")
}

/// The transparent mascot logo as PNG bytes — for the welcome screen.
pub fn bot_logo_png() -> &'static [u8] {
    include_bytes!("../assets/bot-logo.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_images_are_pngs() {
        for png in [bot_icon_png(), app_icon_png(), bot_logo_png()] {
            assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
            assert!(png.len() > 1000);
        }
    }
}
