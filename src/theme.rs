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

impl Theme {
    /// Pick a gradient color based on normalized amplitude (0.0–1.0).
    pub fn bar_color(&self, normalized: f32) -> Color {
        let v = normalized.clamp(0.0, 1.0);
        let idx = (v * (self.gradient.len() - 1) as f32) as usize;
        self.gradient[idx.min(self.gradient.len() - 1)]
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

/// Find a theme by name, or return the first (classic) theme.
pub fn by_name(name: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.name == name).unwrap_or(&THEMES[0])
}