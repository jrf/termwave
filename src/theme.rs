//! Color themes for the visualizer.
//!
//! Themes are loaded from TOML files in `~/.config/termwave/themes/`. Each file
//! defines a `[colors]` table of named hex colors and a `[visualizer]` table
//! that references those names for the gradient, wave, and scope colors.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use ratatui::style::Color;

/// Number of entries in the precomputed gradient lookup table.
const GRADIENT_LUT_SIZE: usize = 256;

/// A color theme defines the gradient stops for spectrum bars and the line
/// color used for waveform/oscilloscope modes.
#[derive(Clone)]
pub struct Theme {
    pub name: String,
    /// Original gradient stop colors. Must have at least one entry.
    pub gradient: Vec<Color>,
    /// Precomputed LUT: 256 colors uniformly spaced by perceptual arc length.
    gradient_lut: Vec<Color>,
    /// Line color for waveform mode.
    pub wave_color: Color,
    /// Line color for oscilloscope mode.
    pub scope_color: Color,
}

/// Convert a ratatui Color to (r, g, b).
fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (255, 255, 255),
    }
}

// ---------------------------------------------------------------------------
// Oklab perceptually uniform color space
// ---------------------------------------------------------------------------

struct Oklab {
    l: f32,
    a: f32,
    b: f32,
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[allow(clippy::excessive_precision)]
fn rgb_to_oklab(r: u8, g: u8, b: u8) -> Oklab {
    let r = srgb_to_linear(r as f32 / 255.0);
    let g = srgb_to_linear(g as f32 / 255.0);
    let b = srgb_to_linear(b as f32 / 255.0);

    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    Oklab {
        l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    }
}

#[allow(clippy::excessive_precision)]
fn oklab_to_rgb(lab: &Oklab) -> (u8, u8, u8) {
    let l_ = lab.l + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    let m_ = lab.l - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    let s_ = lab.l - 0.0894841775 * lab.a - 1.2914855480 * lab.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

    (
        (linear_to_srgb(r.clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(g.clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(b.clamp(0.0, 1.0)) * 255.0).round() as u8,
    )
}

/// Interpolate between two colors in Oklab space for perceptually uniform
/// transitions. `t` is 0.0–1.0.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let (r0, g0, b0) = color_to_rgb(a);
    let (r1, g1, b1) = color_to_rgb(b);
    let t = t.clamp(0.0, 1.0);
    let lab0 = rgb_to_oklab(r0, g0, b0);
    let lab1 = rgb_to_oklab(r1, g1, b1);
    let blended = Oklab {
        l: lab0.l + (lab1.l - lab0.l) * t,
        a: lab0.a + (lab1.a - lab0.a) * t,
        b: lab0.b + (lab1.b - lab0.b) * t,
    };
    let (r, g, b) = oklab_to_rgb(&blended);
    Color::Rgb(r, g, b)
}

/// Build a LUT of `GRADIENT_LUT_SIZE` colors uniformly spaced by perceptual
/// arc length in Oklab space. This ensures each step in the LUT represents
/// an equal perceptual color difference, concentrating resolution where
/// the gradient changes fastest.
fn build_gradient_lut(gradient: &[Color]) -> Vec<Color> {
    if gradient.len() <= 1 {
        return gradient.to_vec();
    }

    let n = gradient.len();

    // Convert stops to Oklab
    let labs: Vec<Oklab> = gradient
        .iter()
        .map(|&c| {
            let (r, g, b) = color_to_rgb(c);
            rgb_to_oklab(r, g, b)
        })
        .collect();

    // Compute cumulative arc length (Euclidean distance in Oklab)
    let mut cum = vec![0.0f32; n];
    for i in 1..n {
        let dl = labs[i].l - labs[i - 1].l;
        let da = labs[i].a - labs[i - 1].a;
        let db = labs[i].b - labs[i - 1].b;
        cum[i] = cum[i - 1] + (dl * dl + da * da + db * db).sqrt();
    }
    let total = cum[n - 1];

    // If all stops are the same color, just repeat it
    if total < f32::EPSILON {
        return vec![gradient[0]; GRADIENT_LUT_SIZE];
    }

    // Sample uniformly by arc length
    let mut lut = Vec::with_capacity(GRADIENT_LUT_SIZE);
    let mut seg = 0;
    for i in 0..GRADIENT_LUT_SIZE {
        let target = (i as f32 / (GRADIENT_LUT_SIZE - 1) as f32) * total;

        // Advance to the segment containing this arc-length position
        while seg + 1 < n - 1 && cum[seg + 1] < target {
            seg += 1;
        }

        let seg_len = cum[seg + 1] - cum[seg];
        let t = if seg_len > f32::EPSILON {
            (target - cum[seg]) / seg_len
        } else {
            0.0
        };
        lut.push(lerp_color(gradient[seg], gradient[seg + 1], t));
    }

    lut
}

impl Theme {
    /// Pick a gradient color based on a normalized value (0.0–1.0),
    /// using the precomputed perceptually-uniform LUT.
    pub fn bar_color(&self, normalized: f32) -> Color {
        let v = normalized.clamp(0.0, 1.0);
        let idx = (v * (self.gradient_lut.len() - 1) as f32).round() as usize;
        self.gradient_lut[idx]
    }
}

// ---------------------------------------------------------------------------
// Theme loading from TOML files
// ---------------------------------------------------------------------------

/// Parse a hex color string like "#82aaff" into a ratatui Color.
fn parse_hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Resolve a color value — either a direct hex string or a reference to a
/// named color in the palette.
fn resolve_color(value: &str, palette: &HashMap<String, Color>) -> Option<Color> {
    if value.starts_with('#') {
        parse_hex(value)
    } else {
        palette.get(value).copied()
    }
}

/// Load a single theme from a TOML string. The file name (without extension)
/// is used as the theme name.
fn parse_theme(name: &str, content: &str) -> Option<Theme> {
    let table: toml::Table = content.parse().ok()?;

    // Parse [colors] into a palette
    let colors_table = table.get("colors")?.as_table()?;
    let mut palette = HashMap::new();
    for (key, val) in colors_table {
        if let Some(hex) = val.as_str().and_then(parse_hex) {
            palette.insert(key.clone(), hex);
        }
    }

    // Parse [visualizer]
    let vis = table.get("visualizer")?.as_table()?;

    let gradient_arr = vis.get("gradient")?.as_array()?;
    let gradient: Vec<Color> = gradient_arr
        .iter()
        .filter_map(|v| v.as_str().and_then(|s| resolve_color(s, &palette)))
        .collect();

    if gradient.is_empty() {
        return None;
    }

    let wave_color = vis
        .get("wave_color")
        .and_then(|v| v.as_str())
        .and_then(|s| resolve_color(s, &palette))
        .unwrap_or(gradient[gradient.len() / 2]);

    let scope_color = vis
        .get("scope_color")
        .and_then(|v| v.as_str())
        .and_then(|s| resolve_color(s, &palette))
        .unwrap_or(gradient[gradient.len() / 2]);

    let gradient_lut = build_gradient_lut(&gradient);
    Some(Theme {
        name: name.to_string(),
        gradient,
        gradient_lut,
        wave_color,
        scope_color,
    })
}

/// Get the themes directory path (~/.config/termwave/themes/).
fn themes_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("termwave")
        .join("themes")
}

/// Load all themes from the themes directory. Returns an empty vec if no
/// valid theme files are found.
pub fn load_themes() -> Vec<Theme> {
    let dir = themes_dir();
    let mut themes = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Some(theme) = parse_theme(&name, &content) {
                        themes.push(theme);
                    }
                }
            }
        }
    }

    themes
}
