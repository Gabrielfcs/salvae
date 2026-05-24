//! Salvaê's embedded images: the bot avatar to download, the app/window icon,
//! and the welcome-screen logo (with its flat background knocked out to alpha).

use image::{Rgba, RgbaImage};

/// The default bot avatar as PNG bytes (embedded at build time). Has a solid
/// background — meant for the Discord bot avatar (circle-masked there).
pub fn bot_icon_png() -> &'static [u8] {
    include_bytes!("../assets/bot-icon.png")
}

/// The app/window (taskbar) icon as PNG bytes.
pub fn app_icon_png() -> &'static [u8] {
    include_bytes!("../assets/app-icon.png")
}

/// The mascot logo as PNG bytes — for the welcome screen.
pub fn bot_logo_png() -> &'static [u8] {
    include_bytes!("../assets/bot-logo.png")
}

/// Decode the welcome logo and knock out its flat background — the source ships
/// with a light-gray background instead of real transparency. Returns
/// `(rgba, width, height)` ready for an `egui::ColorImage`.
pub fn welcome_logo_rgba() -> (Vec<u8>, u32, u32) {
    let mut img = image::load_from_memory(bot_logo_png())
        .expect("decode logo")
        .to_rgba8();
    knock_out_background(&mut img);
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}

/// Flood-fill from the image edges, turning the connected flat background
/// (matched against the top-left corner colour, within a tolerance) fully
/// transparent. The mascot's dark outline stops the fill, so interior light
/// areas stay opaque.
fn knock_out_background(img: &mut RgbaImage) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let bg = *img.get_pixel(0, 0);
    let tol = 40i32;
    let matches = |p: &Rgba<u8>| {
        (p[0] as i32 - bg[0] as i32).abs() <= tol
            && (p[1] as i32 - bg[1] as i32).abs() <= tol
            && (p[2] as i32 - bg[2] as i32).abs() <= tol
    };

    let mut visited = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let idx = (y * w + x) as usize;
        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        let p = *img.get_pixel(x, y);
        if !matches(&p) {
            continue; // robot outline — stop here
        }
        img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 0]));
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
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

    #[test]
    fn knock_out_makes_flat_background_transparent_but_keeps_interior() {
        // 5x5 light-gray field with a dark pixel enclosed by a dark ring.
        let mut img = RgbaImage::from_pixel(5, 5, Rgba([242, 242, 242, 255]));
        // Dark ring around the center so the interior isn't reachable from edges.
        for (x, y) in [
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (3, 2),
            (1, 3),
            (2, 3),
            (3, 3),
        ] {
            img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
        }
        // Center: a light pixel that must stay opaque (enclosed by the ring).
        img.put_pixel(2, 2, Rgba([250, 250, 250, 255]));

        knock_out_background(&mut img);

        assert_eq!(img.get_pixel(0, 0)[3], 0, "corner background → transparent");
        assert_eq!(img.get_pixel(2, 1)[3], 255, "outline stays opaque");
        assert_eq!(
            img.get_pixel(2, 2)[3],
            255,
            "enclosed interior stays opaque"
        );
    }
}
