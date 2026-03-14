//! Terminal rendering using ratatui + crossterm.

use crate::theme::Theme;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Padding},
    widgets::canvas::{Canvas, Line as CanvasLine},
    Terminal,
};
use std::time::Duration;

pub type Term = Terminal<CrosstermBackend<std::io::Stdout>>;

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
    Help,
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
                KeyCode::Char('?') => return Ok(Action::Help),
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
                    .title(" sonitus — select audio device ")
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
                    .title(" sonitus — select theme ")
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

/// Show help overlay. Blocks until any key is pressed.
pub fn help(terminal: &mut Term) -> Result<()> {
    let bindings = [
        ("?", "Show this help"),
        ("d", "Select audio device"),
        ("t", "Select color theme"),
        ("q / Esc", "Quit"),
        ("Ctrl+C", "Quit"),
    ];

    let modes = [
        ("--mode spectrum", "Frequency spectrum bars (default)"),
        ("--mode wave", "Waveform amplitude plot"),
        ("--mode scope", "Oscilloscope (triggered waveform)"),
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
                .title(" sonitus — help ")
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

/// Draw spectrum bars.
pub fn draw_spectrum(terminal: &mut Term, bars: &[f32], theme: &Theme) -> Result<()> {
    let theme_name = theme.name;
    terminal.draw(|frame| {
        let area = frame.area();
        let max_val = bars.iter().cloned().fold(0.0f32, f32::max).max(0.001);

        let bar_width = ((area.width as usize).saturating_sub(2) / bars.len().max(1)).max(1) as u16;

        let ratatui_bars: Vec<Bar> = bars
            .iter()
            .enumerate()
            .map(|(_i, &v)| {
                let normalized = v / max_val;
                let height = (normalized * 100.0) as u64;
                Bar::default()
                    .value(height)
                    .text_value(String::new())
                    .style(Style::default().fg(theme.bar_color(normalized)))
            })
            .collect();

        let chart = BarChart::default()
            .block(
                Block::default()
                    .title(format!(" sonitus — spectrum [{}] ", theme_name))
                    .borders(Borders::ALL),
            )
            .data(BarGroup::default().bars(&ratatui_bars))
            .bar_width(bar_width)
            .bar_gap(0)
            .max(100);

        frame.render_widget(chart, area);
    })?;

    Ok(())
}

/// Draw waveform.
pub fn draw_wave(terminal: &mut Term, samples: &[f32], theme: &Theme) -> Result<()> {
    let color = theme.wave_color;
    draw_wave_inner(terminal, samples, " sonitus — waveform ", color)
}

/// Draw oscilloscope (zero-crossing triggered waveform).
pub fn draw_scope(terminal: &mut Term, samples: &[f32], theme: &Theme) -> Result<()> {
    let trigger_offset = samples
        .windows(2)
        .position(|w| w[0] <= 0.0 && w[1] > 0.0)
        .unwrap_or(0);

    let triggered = &samples[trigger_offset..];
    draw_wave_inner(terminal, triggered, " sonitus — oscilloscope ", theme.scope_color)
}

fn draw_wave_inner(terminal: &mut Term, samples: &[f32], title: &str, color: Color) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let inner = Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));

        let canvas = Canvas::default()
            .block(Block::default().title(title).borders(Borders::ALL))
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
