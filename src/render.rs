use crate::args::{ColorStyle, Settings};
use font8x8::{UnicodeFonts, BASIC_FONTS};

pub fn render(src: &[u8], dst: &mut [u8], width: usize, height: usize, settings: &Settings) {
    dst.fill(0);
    let cell = settings.cell;
    let cols = width / cell;
    let rows = height / cell;
    for cy in 0..rows {
        for cx in 0..cols {
            let mut rgb = [0u64; 3];
            let mut pixels = 0u64;
            for y in cy * cell..((cy + 1) * cell).min(height) {
                for x in cx * cell..((cx + 1) * cell).min(width) {
                    let pixel = (y * width + x) * 3;
                    for channel in 0..3 {
                        rgb[channel] += src[pixel + channel] as u64;
                    }
                    pixels += 1;
                }
            }
            for channel in &mut rgb {
                *channel /= pixels.max(1);
            }
            let mut luminance = ((rgb[0] * 2126 + rgb[1] * 7152 + rgb[2] * 722) / 10000) as usize;
            if settings.invert {
                luminance = 255 - luminance;
            }
            let glyph_char = settings.ramp[luminance * (settings.ramp.len() - 1) / 255];
            if let Some(glyph) = BASIC_FONTS.get(glyph_char) {
                for (glyph_y, row) in glyph.iter().enumerate() {
                    for glyph_x in 0..8 {
                        if row & (1 << glyph_x) == 0 {
                            continue;
                        }
                        let x = cx * cell + glyph_x * cell / 8;
                        let y = cy * cell + glyph_y * cell / 8;
                        if x >= width || y >= height {
                            continue;
                        }
                        let out_x = if settings.mirror { width - 1 - x } else { x };
                        let pixel = (y * width + out_x) * 3;
                        let source_rgb = [rgb[0] as u8, rgb[1] as u8, rgb[2] as u8];
                        let source_rgb = if settings.invert {
                            source_rgb.map(|value| 255 - value)
                        } else {
                            source_rgb
                        };
                        let color = style_color(
                            vivid_color(source_rgb, settings.saturation, settings.brightness),
                            settings.color,
                        );
                        dst[pixel..pixel + 3].copy_from_slice(&color);
                    }
                }
            }
        }
    }
}

fn style_color(rgb: [u8; 3], style: ColorStyle) -> [u8; 3] {
    if matches!(style, ColorStyle::Camera) {
        return rgb;
    }
    let luminance = (rgb[0] as u32 * 2126 + rgb[1] as u32 * 7152 + rgb[2] as u32 * 722) / 10000;
    let level = (48.0 + (luminance as f32 / 255.0).sqrt() * 207.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    match style {
        ColorStyle::Camera => rgb,
        ColorStyle::Green => [level / 4, level, level / 3],
        ColorStyle::Amber => [level, (level as u16 * 3 / 4) as u8, level / 6],
        ColorStyle::Blue => [level / 5, (level as u16 * 2 / 3) as u8, level],
        ColorStyle::Gray => [level; 3],
    }
}

fn vivid_color(rgb: [u8; 3], saturation_gain: f32, brightness: f32) -> [u8; 3] {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let value = (max * brightness).clamp(0.0, 1.0);
    if delta <= f32::EPSILON || max <= f32::EPSILON {
        let gray = (value * 255.0) as u8;
        return [gray; 3];
    }

    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    let saturation = ((delta / max) * saturation_gain).clamp(0.0, 1.0);
    let chroma = value * saturation;
    let secondary = chroma * (1.0 - ((hue * 6.0).rem_euclid(2.0) - 1.0).abs());
    let offset = value - chroma;
    let sector = hue * 6.0;
    let (red, green, blue) = if sector < 1.0 {
        (chroma, secondary, 0.0)
    } else if sector < 2.0 {
        (secondary, chroma, 0.0)
    } else if sector < 3.0 {
        (0.0, chroma, secondary)
    } else if sector < 4.0 {
        (0.0, secondary, chroma)
    } else if sector < 5.0 {
        (secondary, 0.0, chroma)
    } else {
        (chroma, 0.0, secondary)
    };
    [red, green, blue].map(|component| ((component + offset) * 255.0).clamp(0.0, 255.0) as u8)
}
