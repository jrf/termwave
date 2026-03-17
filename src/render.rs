//! Terminal rendering using ratatui + crossterm.

use crate::theme::Theme;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
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
    SelectTheme,
    Settings,
    Help,
    MoreBars,
    FewerBars,
    CycleMode,
}

/// Poll for input events. Returns the action to take.
pub fn poll_input(timeout: Duration) -> Result<Action> {
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                return Ok(Action::Quit);
            }
            match key.code {
                KeyCode::Char('d') => return Ok(Action::SelectDevice),
                KeyCode::Char('t') => return Ok(Action::SelectTheme),
                KeyCode::Char('s') => return Ok(Action::Settings),
                KeyCode::Char('m') => return Ok(Action::CycleMode),
                KeyCode::Char('?') => return Ok(Action::Help),
                KeyCode::Up | KeyCode::Char('+') => return Ok(Action::MoreBars),
                KeyCode::Down | KeyCode::Char('-') => return Ok(Action::FewerBars),
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
pub fn device_menu(terminal: &mut Term, devices: &[String]) -> Result<DeviceMenuResult> {
    let mut selected: usize = 0;
    let total = devices.len() + 1;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            let items: Vec<ListItem> = std::iter::once(ListItem::new(Line::from(vec![
                Span::raw("  Default device"),
            ])))
            .chain(devices.iter().map(|name| {
                ListItem::new(Line::from(vec![Span::raw(format!("  {}", name))]))
            }))
            .enumerate()
            .map(|(i, item)| {
                if i == selected {
                    item.style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    item
                }
            })
            .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(" termwave — select audio device ")
                    .title_bottom(" ↑/↓ navigate  Enter select  Esc back  q quit ")
                    .borders(Borders::ALL)
                    .padding(Padding::vertical(1)),
            );

            frame.render_widget(list, area);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
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

/// Result of the theme menu interaction.
pub enum ThemeMenuResult {
    Selected(usize),
    Cancelled,
    Quit,
}

/// Show an interactive theme selection menu with preview swatches.
pub fn theme_menu(terminal: &mut Term, themes: &[Theme], current_idx: usize) -> Result<ThemeMenuResult> {
    let mut selected = current_idx;
    let total = themes.len();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            let items: Vec<ListItem> = themes
                .iter()
                .enumerate()
                .map(|(i, theme)| {
                    // Build a swatch showing the gradient colors
                    let mut spans: Vec<Span> = vec![Span::raw("  ")];
                    for &color in theme.gradient {
                        spans.push(Span::styled("██", Style::default().fg(color)));
                    }
                    spans.push(Span::raw(format!("  {}", theme.name)));

                    let item = ListItem::new(Line::from(spans));
                    if i == selected {
                        item.style(
                            Style::default()
                                .bg(Color::Rgb(40, 40, 40))
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        item
                    }
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(" termwave — select theme ")
                    .title_bottom(" ↑/↓ navigate  Enter select  Esc back  q quit ")
                    .borders(Borders::ALL)
                    .padding(Padding::vertical(1)),
            );

            frame.render_widget(list, area);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
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
                        return Ok(ThemeMenuResult::Selected(selected));
                    }
                    KeyCode::Esc | KeyCode::Char('t') => {
                        return Ok(ThemeMenuResult::Cancelled);
                    }
                    KeyCode::Char('q') => {
                        return Ok(ThemeMenuResult::Quit);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(ThemeMenuResult::Quit);
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
}

/// Show settings menu. Returns updated settings.
pub fn settings_menu(terminal: &mut Term, settings: &Settings, themes: &[Theme]) -> Result<Option<Settings>> {
    let mut current = settings.clone();
    let mut selected: usize = 0;
    let num_items = 5;

    loop {
        let theme = &themes[current.theme_idx.min(themes.len() - 1)];

        terminal.draw(|frame| {
            let area = frame.area();

            let smoothing_bar = slider_bar(current.smoothing, 0.0, 0.99, 20);
            let noise_bar = slider_bar(current.noise_floor, 0.0, 0.05, 20);

            // Theme swatch
            let mut theme_spans: Vec<Span> = vec![Span::styled(
                format!("  {:16}", "Theme"),
                Style::default().fg(Color::Cyan),
            )];
            for &color in theme.gradient {
                theme_spans.push(Span::styled("██", Style::default().fg(color)));
            }
            theme_spans.push(Span::raw(format!("  {}", theme.name)));

            let items: Vec<ListItem> = vec![
                ListItem::new(Line::from(theme_spans)),
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:16}", "Smoothing"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(format!("{} {:.2}", smoothing_bar, current.smoothing)),
                ])),
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:16}", "Monstercat"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(if current.monstercat { "[ON]" } else { "[OFF]" }),
                ])),
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:16}", "Noise floor"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(format!("{} {:.4}", noise_bar, current.noise_floor)),
                ])),
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:16}", "Gradient"),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(if current.gradient_by_position { "[position]" } else { "[amplitude]" }),
                ])),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                if i == selected {
                    item.style(
                        Style::default()
                            .bg(Color::Rgb(40, 40, 40))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    item
                }
            })
            .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(" termwave — settings ")
                    .title_bottom(" ↑/↓ navigate  ←/→ adjust  Enter/Space toggle  Esc back ")
                    .borders(Borders::ALL)
                    .padding(Padding::new(2, 2, 1, 1)),
            );

            frame.render_widget(list, area);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected + 1 < num_items {
                            selected += 1;
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        adjust_setting(&mut current, selected, -1, themes.len());
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        adjust_setting(&mut current, selected, 1, themes.len());
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if selected == 2 {
                            current.monstercat = !current.monstercat;
                        } else if selected == 4 {
                            current.gradient_by_position = !current.gradient_by_position;
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('s') => {
                        return Ok(Some(current));
                    }
                    KeyCode::Char('q') => {
                        return Ok(None);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    _ => {}
                }
            }
        }
    }
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
        3 => {
            // Noise floor: step by 0.002
            settings.noise_floor =
                (settings.noise_floor + direction as f32 * 0.002).clamp(0.0, 0.05);
        }
        4 => {
            // Gradient mode: toggle
            settings.gradient_by_position = !settings.gradient_by_position;
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
pub fn help(terminal: &mut Term) -> Result<()> {
    let bindings = [
        ("?", "Show this help"),
        ("d", "Select audio device"),
        ("t", "Select color theme"),
        ("s", "Settings (smoothing, monstercat, noise)"),
        ("m", "Cycle visualization mode"),
        ("Up / +", "More bars"),
        ("Down / -", "Fewer bars"),
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
    ];

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
                Span::styled(format!("  {:12}", key), Style::default().fg(Color::Cyan)),
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
                Span::styled(format!("  {:20}", flag), Style::default().fg(Color::Cyan)),
                Span::raw(*desc),
            ]));
        }

        let paragraph = ratatui::widgets::Paragraph::new(lines).block(
            Block::default()
                .title(" termwave — help ")
                .title_bottom(" press any key to close ")
                .borders(Borders::ALL)
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

/// Draw spectrum bars using Unicode block elements (▁▂▃▄▅▆▇█) for 1/8th-cell
/// vertical resolution.
pub fn draw_spectrum(
    terminal: &mut Term,
    bars: &[f32],
    theme: &Theme,
    device: &str,
    gradient_by_position: bool,
    actual_fps: Option<u32>,
) -> Result<()> {
    let theme_name = theme.name;
    let num_bars = bars.len();
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — spectrum [{}] ({} bars){} ", theme_name, num_bars, fps_str);
    let bottom = format!(" {} | ? help ", device);

    terminal.draw(|frame| {
        let area = frame.area();
        let border = Block::default()
            .title(title.as_str())
            .title_bottom(bottom.as_str())
            .borders(Borders::ALL);
        let inner = border.inner(area);
        frame.render_widget(border, area);

        let buf = frame.buffer_mut();
        let max_val = bars.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let bar_w = (inner.width as usize / num_bars.max(1)).max(1);

        for (i, &v) in bars.iter().enumerate() {
            let normalized = v / max_val;
            let color_val = if gradient_by_position {
                i as f32 / (num_bars - 1).max(1) as f32
            } else {
                normalized
            };
            let color = theme.bar_color(color_val);

            // Total height in 1/8ths of a cell
            let eighths = (normalized * inner.height as f32 * 8.0) as usize;
            let full_cells = eighths / 8;
            let remainder = eighths % 8;

            let x_start = inner.x + (i * bar_w) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

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

                for x in x_start..x_end {
                    let cell = &mut buf[(x, y)];
                    cell.set_char(ch);
                    if ch != ' ' {
                        cell.set_fg(color);
                    }
                }
            }
        }
    })?;

    Ok(())
}

/// Draw waveform.
pub fn draw_wave(terminal: &mut Term, samples: &[f32], theme: &Theme, device: &str, actual_fps: Option<u32>) -> Result<()> {
    let color = theme.wave_color;
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — waveform{} ", fps_str);
    let bottom = format!(" {} | ? help ", device);
    draw_wave_inner(terminal, samples, &title, &bottom, color)
}

/// Draw oscilloscope (zero-crossing triggered waveform).
pub fn draw_scope(terminal: &mut Term, samples: &[f32], theme: &Theme, device: &str, actual_fps: Option<u32>) -> Result<()> {
    let trigger_offset = samples
        .windows(2)
        .position(|w| w[0] <= 0.0 && w[1] > 0.0)
        .unwrap_or(0);

    let triggered = &samples[trigger_offset..];
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — oscilloscope{} ", fps_str);
    let bottom = format!(" {} | ? help ", device);
    draw_wave_inner(terminal, triggered, &title, &bottom, theme.scope_color)
}

fn draw_wave_inner(terminal: &mut Term, samples: &[f32], title: &str, bottom: &str, color: Color) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));

        let canvas = Canvas::default()
            .block(Block::default().title(title).title_bottom(bottom).borders(Borders::ALL))
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
    })?;

    Ok(())
}

/// Draw stereo spectrum: left channel bars grow up from center, right channel grows down.
/// Uses Unicode block elements for the upper half (▁▂▃▄▅▆▇█) and full blocks for the lower half.
pub fn draw_stereo(
    terminal: &mut Term,
    left_bars: &[f32],
    right_bars: &[f32],
    theme: &Theme,
    device: &str,
    gradient_by_position: bool,
    actual_fps: Option<u32>,
) -> Result<()> {
    let theme_name = theme.name;
    let num_bars = left_bars.len();
    let fps_str = actual_fps.map(|f| format!(" {}fps", f)).unwrap_or_default();
    let title = format!(" termwave — stereo [{}] ({} bars){} ", theme_name, num_bars, fps_str);
    let bottom = format!(" {} | ? help ", device);

    terminal.draw(|frame| {
        let area = frame.area();
        let border = Block::default()
            .title(title.as_str())
            .title_bottom(bottom.as_str())
            .borders(Borders::ALL);
        let inner = border.inner(area);
        frame.render_widget(border, area);

        let buf = frame.buffer_mut();
        let left_max = left_bars.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let right_max = right_bars.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let bar_w = (inner.width as usize / num_bars.max(1)).max(1);

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
        for (i, &v) in left_bars.iter().enumerate() {
            let normalized = (v / left_max).clamp(0.0, 1.0);
            let color_val = if gradient_by_position {
                i as f32 / (num_bars - 1).max(1) as f32
            } else {
                normalized
            };
            let color = theme.bar_color(color_val);

            let eighths = (normalized * half_h as f32 * 8.0) as usize;
            let full_cells = eighths / 8;
            let remainder = eighths % 8;

            let x_start = inner.x + (i * bar_w) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

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

                for x in x_start..x_end {
                    let cell = &mut buf[(x, y)];
                    cell.set_char(ch);
                    if ch != ' ' {
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
        for (i, &v) in right_bars.iter().enumerate() {
            let normalized = (v / right_max).clamp(0.0, 1.0);
            let color_val = if gradient_by_position {
                i as f32 / (num_bars - 1).max(1) as f32
            } else {
                normalized
            };
            let color = theme.bar_color(color_val);

            // Height in half-cells (2x resolution via ▀)
            let halves = (normalized * lower_h as f32 * 2.0) as usize;
            let full_cells = halves / 2;
            let has_half = halves % 2 == 1;

            let x_start = inner.x + (i * bar_w) as u16;
            let x_end = (x_start + bar_w as u16).min(inner.x + inner.width);

            for row in 0..lower_h {
                let y = center_y + 1 + row;

                if (row as usize) < full_cells {
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char(BLOCK_CHARS[8]);
                        cell.set_fg(color);
                    }
                } else if (row as usize) == full_cells && has_half {
                    // Tip: ▀ (upper half block) — top half is fg (bar color),
                    // bottom half inherits terminal background naturally.
                    for x in x_start..x_end {
                        let cell = &mut buf[(x, y)];
                        cell.set_char('▀');
                        cell.set_fg(color);
                    }
                }
            }
        }
    })?;

    Ok(())
}
