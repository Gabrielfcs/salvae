//! Salvaê's default bot avatar, drawn in code and encoded as PNG bytes — a
//! flat "save/floppy" mark in white on the brand green. No image assets, so
//! the user can download a ready-made icon for their bot without designing one.

/// Edge length of the generated square icon, in pixels.
const SIZE: u32 = 512;

const GREEN: [u8; 4] = [0x2e, 0x7d, 0x32, 0xff];
const WHITE: [u8; 4] = [0xfa, 0xfa, 0xfa, 0xff];

/// Render the default Salvaê bot icon and encode it as PNG bytes.
pub fn bot_icon_png() -> Vec<u8> {
    encode_png(&render_rgba(), SIZE, SIZE)
}

/// A flat floppy-disk silhouette (white body, green shutter + label slots, a
/// cut top-right corner) centred on a solid green background. Discord masks bot
/// avatars to a circle, so the square is filled edge to edge.
fn render_rgba() -> Vec<u8> {
    let s = SIZE as i32;
    let mut buf = vec![0u8; (SIZE * SIZE * 4) as usize];

    let put = |buf: &mut [u8], x: i32, y: i32, c: [u8; 4]| {
        if x >= 0 && x < s && y >= 0 && y < s {
            let i = ((y * s + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&c);
        }
    };

    for y in 0..s {
        for x in 0..s {
            // Background.
            let mut c = GREEN;
            // White floppy body.
            let in_body = (128..384).contains(&x) && (128..384).contains(&y);
            // Cut the top-right corner (diagonal fold).
            let in_fold = x > 320 && y < 192 && (x - 320) > (192 - y);
            if in_body && !in_fold {
                c = WHITE;
            }
            // Green metal "shutter" near the top.
            if (288..360).contains(&x) && (150..236).contains(&y) {
                c = GREEN;
            }
            // Green "label" block near the bottom.
            if (168..344).contains(&x) && (262..356).contains(&y) {
                c = GREEN;
            }
            put(&mut buf, x, y, c);
        }
    }
    buf
}

/// Encode an RGBA8 buffer as PNG bytes.
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_valid_png() {
        let png = bot_icon_png();
        // PNG signature.
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        // Non-trivial size (a real encoded image).
        assert!(png.len() > 100);
    }

    #[test]
    fn render_is_fully_opaque_and_sized() {
        let rgba = render_rgba();
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
        // Every pixel is one of the two opaque brand colours.
        assert!(rgba.chunks_exact(4).all(|p| p[3] == 0xff));
    }
}
