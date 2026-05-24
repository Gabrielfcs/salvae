//! Salvaê's default bot avatar, embedded so the user can download a ready-made
//! icon for their bot without designing one.

/// The default bot avatar as PNG bytes (embedded at build time). Has a solid
/// background — meant for the Discord bot avatar (circle-masked there).
pub fn bot_icon_png() -> &'static [u8] {
    include_bytes!("../assets/bot-icon.png")
}

/// The transparent mascot logo as PNG bytes — for the window icon and the
/// welcome screen.
pub fn bot_logo_png() -> &'static [u8] {
    include_bytes!("../assets/bot-logo.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_is_a_png() {
        let png = bot_icon_png();
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(png.len() > 1000);
    }
}
