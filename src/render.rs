//! Terminal rendering using ratatui + crossterm.

use crate::theme::Theme;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    widgets::canvas::{Canvas, Line as CanvasLine},
    Terminal,
};
use std::time::Duration;

pub type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

/// Unicode block elements from 1/8 to full block.
const BLOCK_CHARS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Initialize the terminal for raw-mode rendering.
pub fn init() -> Result<Term> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode.
pub fn cleanup(terminal: &mut Term) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show,
    )?;
    Ok(())
}

/// Action returned from input polling.
pub enum Action {
    None,
    Quit,
    SelectDevice,
    Settings,
    Help,
    CycleMode,
    SensUp,
    SensDown,
    MoreBars,
    FewerBars,
}

/// Poll for input events. Returns the action to take.
pub fn poll_input(timeout: Duration) -> Result<Action> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                return Ok(Action::None);
            }
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(Action::Quit);
            }
            match key.code {
                KeyCode::Char('d') => return Ok(Action::SelectDevice),
                KeyCode::Char('s') => return Ok(Action::Settings),
                KeyCode::Char('m') => return Ok(Action::CycleMode),
                KeyCode::Char('?') => return Ok(Action::Help),
                KeyCode::Up => return Ok(Action::SensUp),
                KeyCode::Down => return Ok(Action::SensDown),
                KeyCode::Right => return Ok(Action::MoreBars),
                KeyCode::Left => return Ok(Action::FewerBars),
                _ => {}
            }
        }
    }
    Ok(Action::None)
}

/// Result of the device menu interaction.
pub enum DeviceMenuResult {
    /// User selected a device (None = default device).
    Selected(Option<String>),
    /// User cancelled (Esc/d) — go back to visualizer.
    Cancelled,
    /// User wants to quit entirely.
    Quit,
}

/// Show an interactive device selection menu.
pub fn device_menu(terminal: &mut Term, devices: &[String], theme: &Theme) -> Result<DeviceMenuResult> {
    let mut selected: usize = 0;
    let total = devices.len() + 1;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            let accent = theme.wave_color;
            let items: Vec<ListItem> = std::iter::once(ListItem::new(Line::from(vec![
                Span::styled("  Default device", Style::default().fg(accent)),
            ])))
            .chain(devices.iter().map(|name| {
                ListItem::new(Line::from(vec![Span::styled(format!("  {}", name), Style::default().fg(accent))]))
            }))
            .enumerate()
            .map(|(i, item)| {
                if i == selected {
                    item.style(
                        Style::default()
                            .fg(accent)
                            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                    )
                } else {
                    item
                }
            })
            .collect();

            let border_color = theme.gradient[theme.gradient.len() / 2];
            let list = List::new(items).block(
                Block::default()
                    .title(Span::styled(" termwave — select audio device ", Style::default().fg(border_color)))
                    .title_bottom(Span::styled(
                        " ↑/↓ navigate  Enter select  Esc back  q quit ",
                        Style::default().fg(border_color),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .padding(Padding::vertical(1)),
            );

            frame.render_widget(list, area);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected + 1 < total {
                            selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        return if selected == 0 {
                            Ok(DeviceMenuResult::Selected(None))
                        } else {
                            Ok(DeviceMenuResult::Selected(Some(
                                devices[selected - 1].clone(),
                            )))
                        };
                    }
                    KeyCode::Esc | KeyCode::Char('d') => {
                        return Ok(DeviceMenuResult::Cancelled);
                    }
                    KeyCode::Char('q') => {
                        return Ok(DeviceMenuResult::Quit);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(DeviceMenuResult::Quit);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Mutable settings that can be changed at runtime.
#[derive(Clone)]
pub struct Settings {
    pub smoothing: f32,
    pub monstercat: bool,
    pub noise_floor: f32,
    pub theme_idx: usize,
    /// If true, color by bar position; if false, color by amplitude.
    pub gradient_by_position: bool,
    /// Width of each bar in terminal columns (1–8).
    pub bar_width: usize,
    /// Spacing between bars in terminal columns (0–4).
    pub bar_spacing: usize,
    /// Sensitivity in percent (100 = normal).
    pub sensitivity: u32,
}

/// Non-blocking settings overlay state.
pub struct SettingsState {
    pub selected: usize,
    pub num_items: usize,
}

/// Result of handling a key in the settings overlay.
pub enum SettingsAction {
    /// Keep the overlay open.
    None,
    /// Close the overlay (Esc/s).
    Close,
    /// Quit the application.
    Quit,
}

impl SettingsState {
    pub fn new() -> Self {
        Self { selected: 0, num_items: 8 }
    }

    /// Handle a key event. Mutates settings in place, returns what to do.
    pub fn handle_key(&mut self, key: KeyCode, settings: &mut Settings, num_themes: usize) -> SettingsAction {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.num_items {
                    self.selected += 1;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                adjust_setting(settings, self.selected, -1, num_themes);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                adjust_setting(settings, self.selected, 1, num_themes);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.selected == 2 {
                    settings.monstercat = !settings.monstercat;
                } else if self.selected == 4 {
                    settings.gradient_by_position = !settings.gradient_by_position;
                }
            }
            KeyCode::Esc | KeyCode::Char('s') => return SettingsAction::Close,
            KeyCode::Char('q') => return SettingsAction::Quit,
            _ => {}
        }
        SettingsAction::None
    }
}

/// Render settings overlay (centered, 50% of terminal) on top of the current frame.
pub fn render_settings(frame: &mut ratatui::Frame, settings: &Settings, themes: &[Theme], state: &SettingsState) {
    let full = frame.area();
    // Use 80% of terminal width (min 40 cols) so the overlay fits in narrow terminals
    let w = (full.width * 4 / 5).max(40).min(full.width);
    let h = full.height / 2;
    let area = Rect::new(
        full.width.saturating_sub(w) / 2,
        full.height / 4,
        w,
        h,
    );

    frame.render_widget(ratatui::widgets::Clear, area);

    let theme = &themes[settings.theme_idx.min(themes.len() - 1)];
    let accent = theme.wave_color;
    let border_color = theme.gradient[theme.gradient.len() / 2];

    let smoothing_bar = slider_bar(settings.smoothing, 0.0, 0.99, 20);
    let noise_bar = slider_bar(settings.noise_floor, 0.0, 0.05, 20);

    // Helper: build a settings row with cursor indicator for selected item
    let label = |name: &str, idx: usize| -> Span {
        let cursor = if idx == state.selected { "▸ " } else { "  " };
        let style = if idx == state.selected {
            Style::default().fg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(accent)
        };
        Span::styled(format!("{}{:16}", cursor, name), style)
    };

    // Theme swatch
    let mut theme_spans: Vec<Span> = vec![label("Theme", 0)];
    for &color in &theme.gradient {
        theme_spans.push(Span::styled("█", Style::default().fg(color)));
    }
    theme_spans.push(Span::raw(format!(" {}", theme.name)));

    let items: Vec<ListItem> = vec![
        ListItem::new(Line::from(theme_spans)),
        ListItem::new(Line::from(vec![
            label("Smoothing", 1),
            Span::raw(format!("{} {:.2}", smoothing_bar, settings.smoothing)),
        ])),
        ListItem::new(Line::from(vec![
            label("Monstercat", 2),
            Span::raw(if settings.monstercat { "[ON]" } else { "[OFF]" }),
        ])),
        ListItem::new(Line::from(vec![
            label("Noise floor", 3),
            Span::raw(format!("{} {:.4}", noise_bar, settings.noise_floor)),
        ])),
        ListItem::new(Line::from(vec![
            label("Gradient", 4),
            Span::raw(if settings.gradient_by_position { "[position]" } else { "[amplitude]" }),
        ])),
        ListItem::new(Line::from(vec![
            label("Bar width", 5),
            Span::raw(format!("{}", settings.bar_width)),
        ])),
        ListItem::new(Line::from(vec![
            label("Bar spacing", 6),
            Span::raw(format!("{}", settings.bar_spacing)),
        ])),
        ListItem::new(Line::from(vec![
            label("Sensitivity", 7),
            Span::raw(format!("{}%", settings.sensitivity)),
        ])),
    ];

    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(" termwave — settings ", Style::default().fg(border_color)))
            .title_bottom(Span::styled(
                " ↑/↓ navigate  ←/→ adjust  Enter/Space toggle  Esc back ",
                Style::default().fg(border_color),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .padding(Padding::new(2, 2, 1, 1)),
    );

    frame.render_widget(list, area);
}

/// Poll for a key press without mapping to an action.
pub fn poll_key(timeout: Duration) -> Result<Option<KeyCode>> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some(KeyCode::Char('q')));
                }
                return Ok(Some(key.code));
            }
        }
    }
    Ok(None)
}

fn adjust_setting(settings: &mut Settings, idx: usize, direction: i32, num_themes: usize) {
    match idx {
        0 => {
            // Theme: cycle through themes
            if direction > 0 {
                settings.theme_idx = (settings.theme_idx + 1) % num_themes;
            } else if settings.theme_idx == 0 {
                settings.theme_idx = num_themes - 1;
            } else {
                settings.theme_idx -= 1;
            }
        }
        1 => {
            // Smoothing: step by 0.05
            settings.smoothing = (settings.smoothing + direction as f32 * 0.05).clamp(0.0, 0.99);
        }
        2 => {
            // Monstercat: toggle
            settings.monstercat = !settings.monstercat;
        }
        3 => {
            // Noise floor: step by 0.001
            settings.noise_floor =
                (settings.noise_floor + direction as f32 * 0.001).clamp(0.0, 0.05);
        }
        4 => {
            // Gradient mode: toggle
            settings.gradient_by_position = !settings.gradient_by_position;
        }
        5 => {
            // Bar width: 1–8
            settings.bar_width =
                (settings.bar_width as i32 + direction).clamp(1, 8) as usize;
        }
        6 => {
            // Bar spacing: 0–4
            settings.bar_spacing =
                (settings.bar_spacing as i32 + direction).clamp(0, 4) as usize;
        }
        7 => {
            // Sensitivity: 10–500 in steps of 10
            settings.sensitivity =
                (settings.sensitivity as i32 + direction * 10).clamp(10, 500) as u32;
        }
        _ => {}
    }
}

fn slider_bar(value: f32, min: f32, max: f32, width: usize) -> String {
    let ratio = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let filled = (ratio * width as f32) as usize;
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Show help overlay. Blocks until any key is pressed.
pub fn help(terminal: &mut Term, theme: &Theme) -> Result<()> {
    let bindings = [
        ("?", "Show this help"),
        ("d", "Select audio device"),
        ("s", "Settings (theme, smoothing, noise, bar width/spacing)"),
        ("m", "Cycle visualization mode"),
        ("Up / Down", "Increase / decrease sensitivity"),
        ("Right / Left", "Wider / narrower bars"),
        ("q / Esc", "Quit"),
        ("Ctrl+C", "Quit"),
    ];

    let modes = [
        ("--mode spectrum", "Frequency spectrum bars (default)"),
        ("--mode wave", "Waveform amplitude plot"),
        ("--mode scope", "Oscilloscope (triggered waveform)"),
        ("--fps N", "Set target framerate (default: 60)"),
        ("--low-freq N", "Low frequency cutoff in Hz (default: 20)"),
        ("--high-freq N", "High frequency cutoff in Hz (default: 20000)"),
        ("--noise-floor N", "Noise gate threshold (default: 0.0)"),
        ("--bar-width N", "Bar width in columns, 1–8 (default: 2)"),
        ("--bar-spacing N", "Bar spacing in columns, 0–4 (default: 1)"),
    ];

    let accent = theme.wave_color;
    let border_color = theme.gradient[theme.gradient.len() / 2];

    terminal.draw(|frame| {
        let area = frame.area();

        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (key, desc) in &bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:12}", key), Style::default().fg(accent)),
                Span::raw(*desc),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Modes",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (flag, desc) in &modes {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:20}", flag), Style::default().fg(accent)),
                Span::raw(*desc),
            ]));
        }

        let paragraph = ratatui::widgets::Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(" termwave — help ", Style::default().fg(border_color)))
                .title_bottom(Span::styled(
                    " press any key to close ",
                    Style::default().fg(border_color),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .padding(Padding::new(2, 2, 1, 1)),
        );

        frame.render_widget(paragraph, area);
    })?;

    // Wait for any key press
    loop {
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(_) = event::read()? {
                return Ok(());
            }
        }
    }
}

/// Bundled parameters for spectrum/stereo rendering.
pub struct RenderContext<'a> {
    pub theme: &'a Theme,
    pub device: &'a str,
    pub gradient_by_position: bool,
    pub actual_fps: Option<u32>,
    pub bar_width: usize,
    pub bar_spacing: usize,
    pub sensitivity: u32,
}

/// Draw spectrum bars using Unicode block elements (▁▂▃▄▅▆▇█) for 1/8th-cell
/// vertical resolution.
pub fn render_spectrum(
    frame: &mut ratatui::Frame,
    bars: &[f32],
    ctx: &RenderContext,
) {
    let theme = ctx.theme;
    let theme_name = &theme.name;
    let num_bars = bars.len();
    let fps_str = ctx.actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let sens_str = format!(" {}% sensitivity", ctx.sensitivity);
    let title = format!(" termwave — spectrum [{}] ({} bars{}){} ", theme_name, num_bars, sens_str, fps_str);
    let bottom = format!(" {} | ? help ", ctx.device);

    {
        let area = frame.area();
        let border_color = theme.gradient[theme.gradient.len() / 2];
        let border = Block::default()
            .title(Span::styled(title.as_str(), Style::default().fg(border_color)))
            .title_bottom(Span::styled(bottom.as_str(), Style::default().fg(border_color)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = border.inner(area);
        frame.render_widget(border, area);

        let buf = frame.buffer_mut();
        let bar_w = ctx.bar_width;
        let stride = bar_w + ctx.bar_spacing;
        // Center bars within the available width
        let total_w = if num_bars > 0 { num_bars * bar_w + (num_bars - 1) * ctx.bar_spacing } else { 0 };
        let x_offset = inner.x + ((inner.width as usize).saturating_sub(total_w) / 2) as u16;

        let height = inner.height as f32;

        for (i, &v) in bars.iter().enumerate() {
            let normalized = v.clamp(0.0, 1.0);

            // Total height in 1/8ths of a cell
            let eighths = (normalized * height * 8.0) as usize;
            let full_cells = eighths / 8;
            let remainder = eighths % 8;

            let x_start = x_offset + (i * stride) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

            // Horizontal position for gradient_by_position mode
            let h_color = if ctx.gradient_by_position {
                Some(theme.bar_color(i as f32 / (num_bars - 1).max(1) as f32))
            } else {
                None
            };

            // Draw from bottom up
            for row in 0..inner.height {
                let y = inner.y + inner.height - 1 - row;
                let ch = if (row as usize) < full_cells {
                    BLOCK_CHARS[8] // full block
                } else if (row as usize) == full_cells && remainder > 0 {
                    BLOCK_CHARS[remainder]
                } else {
                    ' '
                };

                if ch != ' ' {
                    // Color by vertical position (bottom=0, top=1) or horizontal position
                    let color = h_color.unwrap_or_else(|| {
                        theme.bar_color(row as f32 / (height - 1.0).max(1.0))
                    });
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char(ch);
                        cell.set_fg(color);
                    }
                }
            }
        }
    }
}

/// Draw waveform.
pub fn render_wave(frame: &mut ratatui::Frame, samples: &[f32], theme: &Theme, device: &str, actual_fps: Option<u32>) {
    let color = theme.wave_color;
    let border_color = theme.gradient[theme.gradient.len() / 2];
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — waveform{} ", fps_str);
    let bottom = format!(" {} | ? help ", device);
    render_wave_inner(frame, samples, &title, &bottom, color, border_color);
}

/// Draw oscilloscope (zero-crossing triggered waveform).
pub fn render_scope(frame: &mut ratatui::Frame, samples: &[f32], theme: &Theme, device: &str, actual_fps: Option<u32>) {
    let trigger_offset = samples
        .windows(2)
        .position(|w| w[0] <= 0.0 && w[1] > 0.0)
        .unwrap_or(0);

    let triggered = &samples[trigger_offset..];
    let border_color = theme.gradient[theme.gradient.len() / 2];
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — oscilloscope{} ", fps_str);
    let bottom = format!(" {} | ? help ", device);
    render_wave_inner(frame, triggered, &title, &bottom, theme.scope_color, border_color);
}

fn render_wave_inner(frame: &mut ratatui::Frame, samples: &[f32], title: &str, bottom: &str, color: Color, border_color: Color) {
    let area = frame.area();
    let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));

    let canvas = Canvas::default()
        .block(Block::default()
            .title(Span::styled(title, Style::default().fg(border_color)))
            .title_bottom(Span::styled(bottom, Style::default().fg(border_color)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)))
        .x_bounds([0.0, inner.width as f64])
        .y_bounds([-1.0, 1.0])
        .paint(|ctx| {
            if samples.len() < 2 {
                return;
            }
            let step = samples.len() as f64 / inner.width as f64;
            for i in 0..inner.width.saturating_sub(1) as usize {
                let idx0 = (i as f64 * step) as usize;
                let idx1 = ((i + 1) as f64 * step) as usize;
                let y0 = samples.get(idx0).copied().unwrap_or(0.0) as f64;
                let y1 = samples.get(idx1).copied().unwrap_or(0.0) as f64;
                ctx.draw(&CanvasLine {
                    x1: i as f64,
                    y1: y0,
                    x2: (i + 1) as f64,
                    y2: y1,
                    color,
                });
            }
        });

    frame.render_widget(canvas, area);
}

/// Draw stereo spectrum: left channel bars grow up from center, right channel grows down.
/// Uses Unicode block elements for the upper half (▁▂▃▄▅▆▇█) and full blocks for the lower half.
pub fn render_stereo(
    frame: &mut ratatui::Frame,
    left_bars: &[f32],
    right_bars: &[f32],
    ctx: &RenderContext,
) {
    let theme = ctx.theme;
    let theme_name = &theme.name;
    let num_bars = left_bars.len();
    let fps_str = ctx.actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let sens_str = format!(" {}% sensitivity", ctx.sensitivity);
    let title = format!(" termwave — stereo [{}] ({} bars{}){} ", theme_name, num_bars, sens_str, fps_str);
    let bottom = format!(" {} | ? help ", ctx.device);

    {
        let area = frame.area();
        let border_color = theme.gradient[theme.gradient.len() / 2];
        let border = Block::default()
            .title(Span::styled(title.as_str(), Style::default().fg(border_color)))
            .title_bottom(Span::styled(bottom.as_str(), Style::default().fg(border_color)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = border.inner(area);
        frame.render_widget(border, area);

        let buf = frame.buffer_mut();
        let bar_w = ctx.bar_width;
        let stride = bar_w + ctx.bar_spacing;
        let total_w = if num_bars > 0 { num_bars * bar_w + (num_bars - 1) * ctx.bar_spacing } else { 0 };
        let x_offset = inner.x + ((inner.width as usize).saturating_sub(total_w) / 2) as u16;

        // Split inner area into upper half (left channel) and lower half (right channel)
        let half_h = inner.height / 2;
        let center_y = inner.y + half_h;

        // Center line
        for x in inner.x..inner.x + inner.width {
            let cell = &mut buf[(x, center_y)];
            cell.set_char('─');
            cell.set_fg(Color::DarkGray);
        }

        // Left channel: bars grow upward from center using block elements
        let half_h_f = half_h as f32;
        for (i, &v) in left_bars.iter().enumerate() {
            let normalized = v.clamp(0.0, 1.0);

            let eighths = (normalized * half_h_f * 8.0) as usize;
            let full_cells = eighths / 8;
            let remainder = eighths % 8;

            let x_start = x_offset + (i * stride) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

            let h_color = if ctx.gradient_by_position {
                Some(theme.bar_color(i as f32 / (num_bars - 1).max(1) as f32))
            } else {
                None
            };

            // Draw from center upward
            for row in 0..half_h {
                let y = center_y - 1 - row;
                let ch = if (row as usize) < full_cells {
                    BLOCK_CHARS[8]
                } else if (row as usize) == full_cells && remainder > 0 {
                    BLOCK_CHARS[remainder]
                } else {
                    ' '
                };

                if ch != ' ' {
                    let color = h_color.unwrap_or_else(|| {
                        theme.bar_color(row as f32 / (half_h_f - 1.0).max(1.0))
                    });
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char(ch);
                        cell.set_fg(color);
                    }
                }
            }
        }

        // Right channel: bars grow downward from center.
        // Full cells use █. The tip cell uses ▀ (upper half block) for half-cell
        // precision — we can't use the full ▁▂▃▄▅▆▇ set for downward bars because
        // those fill from the bottom, and without knowing the terminal background
        // color we can't invert them cleanly.
        let lower_h = inner.height - half_h - 1; // -1 for center line
        let lower_h_f = lower_h as f32;
        for (i, &v) in right_bars.iter().enumerate() {
            let normalized = v.clamp(0.0, 1.0);

            // Height in half-cells (2x resolution via ▀)
            let halves = (normalized * lower_h_f * 2.0) as usize;
            let full_cells = halves / 2;
            let has_half = halves % 2 == 1;

            let x_start = x_offset + (i * stride) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

            let h_color = if ctx.gradient_by_position {
                Some(theme.bar_color(i as f32 / (num_bars - 1).max(1) as f32))
            } else {
                None
            };

            for row in 0..lower_h {
                let y = center_y + 1 + row;
                let color = h_color.unwrap_or_else(|| {
                    theme.bar_color(row as f32 / (lower_h_f - 1.0).max(1.0))
                });

                if (row as usize) < full_cells {
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char(BLOCK_CHARS[8]);
                        cell.set_fg(color);
                    }
                } else if (row as usize) == full_cells && has_half {
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char('▀');
                        cell.set_fg(color);
                    }
                }
            }
        }
    }
}
