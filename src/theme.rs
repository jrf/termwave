//! Color themes for the visualizer.

use ratatui::style::Color;

/// A color theme defines the gradient stops for spectrum bars and the line
/// color used for waveform/oscilloscope modes.
#[derive(Clone)]
pub struct Theme {
    pub name: &'static str,
    /// Colors from low to high amplitude. Must have at least one entry.
    pub gradient: &'static [Color],
    /// Line color for waveform mode.
    pub wave_color: Color,
    /// Line color for oscilloscope mode.
    pub scope_color: Color,
}

/// Convert a ratatui Color to (r, g, b). Named ANSI colors are mapped to
/// typical terminal defaults.
fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 0, 0),
        Color::Green => (0, 205, 0),
        Color::Yellow => (205, 205, 0),
        Color::Blue => (0, 0, 238),
        Color::Magenta => (205, 0, 205),
        Color::Cyan => (0, 205, 205),
        Color::White => (255, 255, 255),
        Color::Gray => (128, 128, 128),
        _ => (255, 255, 255),
    }
}

/// Linearly interpolate between two colors. `t` is 0.0–1.0.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let (r0, g0, b0) = color_to_rgb(a);
    let (r1, g1, b1) = color_to_rgb(b);
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (r0 as f32 + (r1 as f32 - r0 as f32) * t) as u8,
        (g0 as f32 + (g1 as f32 - g0 as f32) * t) as u8,
        (b0 as f32 + (b1 as f32 - b0 as f32) * t) as u8,
    )
}

/// Sample a color from the gradient at position `v` (0.0–1.0), interpolating
/// between stops.
fn sample_gradient(gradient: &[Color], v: f32) -> Color {
    let v = v.clamp(0.0, 1.0);
    let last = (gradient.len() - 1) as f32;
    let pos = v * last;
    let lo = pos as usize;
    let hi = (lo + 1).min(gradient.len() - 1);
    let frac = pos - lo as f32;
    lerp_color(gradient[lo], gradient[hi], frac)
}

impl Theme {
    /// Pick a gradient color based on a normalized value (0.0–1.0),
    /// interpolating between gradient stops.
    pub fn bar_color(&self, normalized: f32) -> Color {
        sample_gradient(self.gradient, normalized)
    }
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "classic",
        gradient: &[Color::Blue, Color::Cyan, Color::Green, Color::Yellow, Color::Red],
        wave_color: Color::Cyan,
        scope_color: Color::Green,
    },
    Theme {
        name: "fire",
        gradient: &[
            Color::Rgb(128, 0, 0),
            Color::Rgb(200, 50, 0),
            Color::Rgb(255, 100, 0),
            Color::Rgb(255, 180, 0),
            Color::Rgb(255, 255, 100),
        ],
        wave_color: Color::Rgb(255, 100, 0),
        scope_color: Color::Rgb(255, 180, 0),
    },
    Theme {
        name: "ocean",
        gradient: &[
            Color::Rgb(0, 20, 60),
            Color::Rgb(0, 60, 120),
            Color::Rgb(0, 120, 180),
            Color::Rgb(0, 200, 220),
            Color::Rgb(150, 255, 255),
        ],
        wave_color: Color::Rgb(0, 200, 220),
        scope_color: Color::Rgb(150, 255, 255),
    },
    Theme {
        name: "purple",
        gradient: &[
            Color::Rgb(40, 0, 60),
            Color::Rgb(80, 0, 120),
            Color::Rgb(140, 0, 200),
            Color::Rgb(200, 80, 255),
            Color::Rgb(255, 180, 255),
        ],
        wave_color: Color::Rgb(200, 80, 255),
        scope_color: Color::Rgb(255, 180, 255),
    },
    Theme {
        name: "matrix",
        gradient: &[
            Color::Rgb(0, 40, 0),
            Color::Rgb(0, 80, 0),
            Color::Rgb(0, 140, 0),
            Color::Rgb(0, 200, 0),
            Color::Rgb(0, 255, 0),
        ],
        wave_color: Color::Rgb(0, 200, 0),
        scope_color: Color::Rgb(0, 255, 0),
    },
    Theme {
        name: "synthwave",
        gradient: &[
            Color::Rgb(15, 0, 40),
            Color::Rgb(75, 0, 130),
            Color::Rgb(180, 0, 180),
            Color::Rgb(255, 20, 147),
            Color::Rgb(255, 100, 50),
        ],
        wave_color: Color::Rgb(255, 20, 147),
        scope_color: Color::Rgb(180, 0, 180),
    },
    Theme {
        name: "mono",
        gradient: &[
            Color::Rgb(80, 80, 80),
            Color::Rgb(120, 120, 120),
            Color::Rgb(160, 160, 160),
            Color::Rgb(200, 200, 200),
            Color::White,
        ],
        wave_color: Color::Rgb(200, 200, 200),
        scope_color: Color::White,
    },
];
