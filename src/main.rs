mod analysis;
mod audio;
mod config;
mod render;
mod theme;

use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::event::KeyCode;

#[derive(Parser)]
#[command(name = "termwave", about = "Terminal audio visualizer")]
struct Cli {
    /// Visualization mode
    #[arg(short, long)]
    mode: Option<String>,

    /// Audio input device (defaults to system audio via ScreenCaptureKit)
    #[arg(short, long)]
    device: Option<String>,

    /// Color theme (classic, fire, ocean, purple, matrix, mono)
    #[arg(short, long)]
    theme: Option<String>,

    /// Target frames per second
    #[arg(long)]
    fps: Option<u64>,

    /// Low frequency cutoff in Hz
    #[arg(long)]
    low_freq: Option<f32>,

    /// High frequency cutoff in Hz
    #[arg(long)]
    high_freq: Option<f32>,

    /// Noise floor threshold (0.0–1.0, bars below this are zeroed)
    #[arg(long)]
    noise_floor: Option<f32>,

    /// Enable monstercat smoothing (connects bar tops in a smooth curve)
    #[arg(long)]
    monstercat: bool,

    /// Temporal smoothing factor (0.0 = none, 0.9 = heavy)
    #[arg(long)]
    smoothing: Option<f32>,

    /// Color bars by position instead of amplitude
    #[arg(long)]
    gradient_by_position: bool,

    /// Width of each bar in terminal columns (1–8)
    #[arg(long)]
    bar_width: Option<usize>,

    /// Spacing between bars in terminal columns (0–4)
    #[arg(long)]
    bar_spacing: Option<usize>,

    /// List available audio input devices
    #[arg(long)]
    list_devices: bool,
}

#[derive(Clone, PartialEq)]
enum Mode {
    Spectrum,
    Wave,
    Scope,
    Stereo,
}

impl Mode {
    fn from_str(s: &str) -> Self {
        match s {
            "wave" => Mode::Wave,
            "scope" => Mode::Scope,
            "stereo" => Mode::Stereo,
            _ => Mode::Spectrum,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Mode::Spectrum => "spectrum",
            Mode::Wave => "wave",
            Mode::Scope => "scope",
            Mode::Stereo => "stereo",
        }
    }

    fn next(&self) -> Self {
        match self {
            Mode::Spectrum => Mode::Wave,
            Mode::Wave => Mode::Scope,
            Mode::Scope => Mode::Stereo,
            Mode::Stereo => Mode::Spectrum,
        }
    }
}

const DEFAULT_SAMPLE_RATE: u32 = 48000;
const MONSTERCAT_STRENGTH: f32 = 0.75;
const MIN_BARS: usize = 8;
const MAX_BARS: usize = 256;
const BAR_STEP: usize = 8;
const SENS_STEP: u32 = 10;
/// Gravity acceleration in units/s². At 60fps (dt≈0.017s), a bar at height 1.0
/// takes about 0.25s to fall — similar feel to the old per-frame 0.01 value.
const GRAVITY_ACCEL: f32 = 5.0;

fn start_audio(
    mono_buf: &audio::SampleBuffer,
    stereo: &audio::StereoPair,
    device: Option<&str>,
    last_write: &audio::LastWriteTime,
) -> Result<(u32, audio::CaptureHandle)> {
    if device.is_none()
        || device == Some(audio::SYSTEM_AUDIO_LABEL)
        || device == Some("system")
    {
        audio::start_tap(
            mono_buf.clone(),
            (stereo.0.clone(), stereo.1.clone()),
            DEFAULT_SAMPLE_RATE,
            last_write,
        )
    } else {
        audio::start_capture(
            mono_buf.clone(),
            (stereo.0.clone(), stereo.1.clone()),
            device,
            last_write,
        )
    }
}

fn save_state(
    cfg: &mut config::Config,
    settings: &render::Settings,
    theme_name: &str,
    mode: &Mode,
) {
    cfg.smoothing = settings.smoothing;
    cfg.monstercat = settings.monstercat;
    cfg.noise_floor = settings.noise_floor;
    cfg.theme = theme_name.to_string();
    cfg.bars = 0;
    cfg.mode = mode.as_str().to_string();

    cfg.gradient_by_position = settings.gradient_by_position;
    cfg.bar_width = settings.bar_width;
    cfg.bar_spacing = settings.bar_spacing;
    cfg.sensitivity = settings.sensitivity;

    let _ = config::save(cfg);
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.list_devices {
        for name in audio::list_devices()? {
            println!("{}", name);
        }
        return Ok(());
    }

    // Load config, then let CLI args override
    let mut cfg = config::load();
    if let Some(ref m) = cli.mode {
        cfg.mode = m.clone();
    }
    if let Some(ref t) = cli.theme {
        cfg.theme = t.clone();
    }
    if let Some(f) = cli.fps {
        cfg.fps = f;
    }
    if let Some(f) = cli.low_freq {
        cfg.low_freq = f;
    }
    if let Some(f) = cli.high_freq {
        cfg.high_freq = f;
    }
    if let Some(s) = cli.smoothing {
        cfg.smoothing = s;
    }
    if cli.monstercat {
        cfg.monstercat = true;
    }
    if let Some(n) = cli.noise_floor {
        cfg.noise_floor = n;
    }
    if cli.gradient_by_position {
        cfg.gradient_by_position = true;
    }
    if let Some(w) = cli.bar_width {
        cfg.bar_width = w.clamp(1, 8);
    }
    if let Some(s) = cli.bar_spacing {
        cfg.bar_spacing = s.clamp(0, 4);
    }


    let mut mode = Mode::from_str(&cfg.mode);

    // Start audio capture
    let mono_buf = audio::new_buffer(analysis::FFT_SIZE);
    let stereo = audio::new_stereo_buffers(analysis::FFT_SIZE);
    let last_write = audio::LastWriteTime::new();
    let now_playing = audio::new_now_playing();
    let mut device_name = cli
        .device
        .clone()
        .unwrap_or_else(|| audio::SYSTEM_AUDIO_LABEL.to_string());
    let (mut sample_rate, mut capture) =
        start_audio(&mono_buf, &stereo, cli.device.as_deref(), &last_write)?;

    // Poll now-playing in a background thread via osascript (separate process
    // so TCC checks don't interfere with ScreenCaptureKit)
    audio::start_now_playing_poller(now_playing.clone(), Duration::from_secs(3));

    // Init terminal
    let mut terminal = render::init()?;
    let fps = cfg.fps.max(1);
    let frame_duration = Duration::from_millis(1000 / fps);
    let mut desired_bars = MAX_BARS;
    let mut num_bars = MIN_BARS;
    let mut prev_bars: Vec<f32> = vec![0.0; num_bars];
    let mut prev_left: Vec<f32> = vec![0.0; num_bars];
    let mut prev_right: Vec<f32> = vec![0.0; num_bars];
    let low_freq = cfg.low_freq;
    let high_freq = cfg.high_freq;
    let mut theme_idx = theme::THEMES
        .iter()
        .position(|t| t.name == cfg.theme)
        .unwrap_or(0);
    let mut current_theme = &theme::THEMES[theme_idx];

    let mut settings = render::Settings {
        smoothing: cfg.smoothing.clamp(0.0, 0.99),
        monstercat: cfg.monstercat,
        noise_floor: cfg.noise_floor,
        theme_idx,
        gradient_by_position: cfg.gradient_by_position,
        bar_width: cfg.bar_width.clamp(1, 8),
        bar_spacing: cfg.bar_spacing.clamp(0, 4),
        sensitivity: cfg.sensitivity.clamp(10, 500),
    };

    let analyzer = analysis::SpectrumAnalyzer::new();
    let mut autosens = analysis::AutoSensitivity::new();
    let mut autosens_l = analysis::AutoSensitivity::new();
    let mut autosens_r = analysis::AutoSensitivity::new();


    // Gravity for bar fall-off
    let mut gravity = analysis::Gravity::new(GRAVITY_ACCEL);
    let mut gravity_l = analysis::Gravity::new(GRAVITY_ACCEL);
    let mut gravity_r = analysis::Gravity::new(GRAVITY_ACCEL);

    // FPS tracking
    let mut frame_count: u32 = 0;
    let mut fps_timer = Instant::now();
    let mut actual_fps: Option<u32> = None;

    // Track frame time for frame-rate independent gravity
    let mut last_frame_time = Instant::now();

    // Auto-restart: track when we last attempted a tap restart to avoid hammering
    let mut last_restart_attempt: Option<Instant> = None;
    const RESTART_COOLDOWN: Duration = Duration::from_secs(2);

    // Theme picker overlay state
    let mut theme_picker: Option<render::ThemePicker> = None;

    loop {
        let frame_start = Instant::now();

        // FPS counter: update once per second
        frame_count += 1;
        if fps_timer.elapsed() >= Duration::from_secs(1) {
            actual_fps = Some(frame_count);
            frame_count = 0;
            fps_timer = Instant::now();
        }

        // Compute max bars that fit the terminal width.
        let term_w = terminal.size()?.width.saturating_sub(2) as usize;
        let stride = settings.bar_width + settings.bar_spacing;
        let max_fit = if stride > 0 { (term_w + settings.bar_spacing) / stride } else { term_w };
        // Auto-fill up to desired_bars (user can reduce with Left arrow).
        let effective_bars = desired_bars.min(max_fit).max(MIN_BARS);
        if effective_bars != num_bars {
            num_bars = effective_bars;
            prev_bars = vec![0.0; num_bars];
            prev_left = vec![0.0; num_bars];
            prev_right = vec![0.0; num_bars];
        }

        // When theme picker is open, handle its input separately
        if let Some(ref mut picker) = theme_picker {
            if let Some(key) = render::poll_key(Duration::ZERO)? {
                match key {
                    KeyCode::Up | KeyCode::Char('k') => picker.up(),
                    KeyCode::Down | KeyCode::Char('j') => picker.down(),
                    KeyCode::Enter => {
                        theme_idx = picker.selected;
                        current_theme = &theme::THEMES[theme_idx];
                        settings.theme_idx = theme_idx;
                        save_state(&mut cfg, &settings, current_theme.name, &mode);
                        theme_picker = None;
                    }
                    KeyCode::Esc | KeyCode::Char('t') => {
                        // Revert to the theme that was active before opening the picker
                        current_theme = &theme::THEMES[theme_idx];
                        theme_picker = None;
                    }
                    KeyCode::Char('q') => break,
                    _ => {}
                }
                // Live-preview the selected theme while browsing
                if let Some(ref picker) = theme_picker {
                    current_theme = &theme::THEMES[picker.selected];
                }
            }
        } else {

        match render::poll_input(Duration::ZERO)? {
            render::Action::Quit => break,
            render::Action::CycleMode => {
                mode = mode.next();
                prev_bars = vec![0.0; num_bars];
                prev_left = vec![0.0; num_bars];
                prev_right = vec![0.0; num_bars];
                save_state(&mut cfg, &settings, current_theme.name, &mode);
                continue;
            }
            render::Action::SelectDevice => {
                let devices = audio::list_devices()?;
                match render::device_menu(&mut terminal, &devices, current_theme)? {
                    render::DeviceMenuResult::Selected(new_device) => {
                        drop(capture);
                        let (sr, handle) =
                            start_audio(&mono_buf, &stereo, new_device.as_deref(), &last_write)?;
                        sample_rate = sr;
                        capture = handle;
                        device_name =
                            new_device.unwrap_or_else(|| audio::SYSTEM_AUDIO_LABEL.to_string());
                        prev_bars = vec![0.0; num_bars];
                        prev_left = vec![0.0; num_bars];
                        prev_right = vec![0.0; num_bars];
                        autosens = analysis::AutoSensitivity::new();
                        autosens_l = analysis::AutoSensitivity::new();
                        autosens_r = analysis::AutoSensitivity::new();
                    }
                    render::DeviceMenuResult::Quit => break,
                    render::DeviceMenuResult::Cancelled => {}
                }
                continue;
            }
            render::Action::SelectTheme => {
                theme_picker = Some(render::ThemePicker::new(theme_idx, theme::THEMES.len()));
                current_theme = &theme::THEMES[theme_idx];
            }
            render::Action::Settings => {
                match render::settings_menu(&mut terminal, &settings, theme::THEMES)? {
                    Some(new_settings) => {
                        settings = new_settings;
                        theme_idx = settings.theme_idx;
                        current_theme = &theme::THEMES[theme_idx];
                        save_state(&mut cfg, &settings, current_theme.name, &mode);
                    }
                    None => break,
                }
                continue;
            }
            render::Action::Help => {
                render::help(&mut terminal, current_theme)?;
                continue;
            }
            render::Action::SensUp => {
                settings.sensitivity = (settings.sensitivity + SENS_STEP).min(500);
                save_state(&mut cfg, &settings, current_theme.name, &mode);
                continue;
            }
            render::Action::SensDown => {
                settings.sensitivity = settings.sensitivity.saturating_sub(SENS_STEP).max(10);
                save_state(&mut cfg, &settings, current_theme.name, &mode);
                continue;
            }
            render::Action::MoreBars => {
                desired_bars = (num_bars + BAR_STEP).min(max_fit).min(MAX_BARS);
                num_bars = desired_bars;
                prev_bars = vec![0.0; num_bars];
                prev_left = vec![0.0; num_bars];
                prev_right = vec![0.0; num_bars];
                continue;
            }
            render::Action::FewerBars => {
                desired_bars = num_bars.saturating_sub(BAR_STEP).max(MIN_BARS);
                num_bars = desired_bars;
                prev_bars = vec![0.0; num_bars];
                prev_left = vec![0.0; num_bars];
                prev_right = vec![0.0; num_bars];
                continue;
            }
            render::Action::None => {}
        }

        } // end else (theme picker not open)

        let dt = last_frame_time.elapsed().as_secs_f32().clamp(0.001, 0.1);
        last_frame_time = Instant::now();

        // Detect stale audio: if the audio thread hasn't written new samples
        // recently (e.g. ScreenCaptureKit connection lost), zero the buffers
        // so we don't keep visualizing stale data.
        if last_write.elapsed() > Duration::from_millis(100) {
            mono_buf.lock().unwrap().fill(0.0);
            stereo.0.lock().unwrap().fill(0.0);
            stereo.1.lock().unwrap().fill(0.0);
        }

        // Auto-restart tap if it died (e.g. macOS revoked Screen Recording)
        if capture.tap_exited() {
            let should_restart = last_restart_attempt
                .map(|t| t.elapsed() >= RESTART_COOLDOWN)
                .unwrap_or(true);
            if should_restart {
                last_restart_attempt = Some(Instant::now());
                drop(capture);
                match start_audio(&mono_buf, &stereo, Some(&device_name), &last_write) {
                    Ok((sr, handle)) => {
                        sample_rate = sr;
                        capture = handle;
                        autosens = analysis::AutoSensitivity::new();
                        autosens_l = analysis::AutoSensitivity::new();
                        autosens_r = analysis::AutoSensitivity::new();
                    }
                    Err(_) => {
                        // Create a dummy handle so we keep looping and retry later.
                        // Use a no-op child that's already exited.
                        capture = audio::CaptureHandle::Tap(
                            std::process::Command::new("true")
                                .spawn()
                                .expect("failed to spawn dummy process"),
                        );
                    }
                }
            }
        }

        // Detect silence: if the buffer's energy is negligible, zero the
        // smoothing state so bars fall via gravity instead of freezing.
        {
            let buf = mono_buf.lock().unwrap();
            let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
            if rms < 1e-6 {
                drop(buf);
                prev_bars.fill(0.0);
                prev_left.fill(0.0);
                prev_right.fill(0.0);
                autosens = analysis::AutoSensitivity::new();
                autosens_l = analysis::AutoSensitivity::new();
                autosens_r = analysis::AutoSensitivity::new();
            }
        }

        // Build status line: device name + now playing track (if available)
        let status = match now_playing.lock().unwrap().as_deref() {
            Some(track) => format!("{} | {}", device_name, track),
            None => device_name.clone(),
        };

        match mode {
            Mode::Spectrum => {
                let samples = {
                    let buf = mono_buf.lock().unwrap();
                    buf.clone()
                };
                let magnitudes = analyzer.spectrum(&samples);
                let bars = analysis::bin_spectrum(
                    &magnitudes, num_bars, sample_rate, low_freq, high_freq,
                );

                let mut smoothed = analysis::smooth(&prev_bars, &bars, settings.smoothing, dt);
                if settings.monstercat {
                    analysis::monstercat(&mut smoothed, MONSTERCAT_STRENGTH);
                }
                if settings.noise_floor > 0.0 {
                    analysis::noise_gate(&mut smoothed, settings.noise_floor);
                }
                autosens.apply(&mut smoothed, dt);
                let sens = settings.sensitivity as f32 / 100.0;
                if sens != 1.0 {
                    for v in smoothed.iter_mut() { *v *= sens; }
                }
                // Store prev_bars before gravity so gravity doesn't feed back into smoothing
                prev_bars = smoothed.clone();
                gravity.apply(&mut smoothed, dt);
                render::draw_spectrum(&mut terminal, &smoothed, current_theme, &status, settings.gradient_by_position, actual_fps, settings.bar_width, settings.bar_spacing, settings.sensitivity)?;
            }
            Mode::Stereo => {
                let left_samples = {
                    let buf = stereo.0.lock().unwrap();
                    buf.clone()
                };
                let right_samples = {
                    let buf = stereo.1.lock().unwrap();
                    buf.clone()
                };

                let left_mag = analyzer.spectrum(&left_samples);
                let right_mag = analyzer.spectrum(&right_samples);

                let left_bars = analysis::bin_spectrum(
                    &left_mag, num_bars, sample_rate, low_freq, high_freq,
                );
                let right_bars = analysis::bin_spectrum(
                    &right_mag, num_bars, sample_rate, low_freq, high_freq,
                );


                let mut smooth_l =
                    analysis::smooth(&prev_left, &left_bars, settings.smoothing, dt);
                let mut smooth_r =
                    analysis::smooth(&prev_right, &right_bars, settings.smoothing, dt);

                if settings.monstercat {
                    analysis::monstercat(&mut smooth_l, MONSTERCAT_STRENGTH);
                    analysis::monstercat(&mut smooth_r, MONSTERCAT_STRENGTH);
                }

                if settings.noise_floor > 0.0 {
                    analysis::noise_gate(&mut smooth_l, settings.noise_floor);
                    analysis::noise_gate(&mut smooth_r, settings.noise_floor);
                }

                autosens_l.apply(&mut smooth_l, dt);
                autosens_r.apply(&mut smooth_r, dt);
                let sens = settings.sensitivity as f32 / 100.0;
                if sens != 1.0 {
                    for v in smooth_l.iter_mut() { *v *= sens; }
                    for v in smooth_r.iter_mut() { *v *= sens; }
                }
                // Store before gravity so gravity doesn't feed back into smoothing
                prev_left = smooth_l.clone();
                prev_right = smooth_r.clone();
                gravity_l.apply(&mut smooth_l, dt);
                gravity_r.apply(&mut smooth_r, dt);

                render::draw_stereo(
                    &mut terminal, &smooth_l, &smooth_r, current_theme, &status, settings.gradient_by_position, actual_fps, settings.bar_width, settings.bar_spacing, settings.sensitivity,
                )?;
            }
            Mode::Wave => {
                let samples = {
                    let buf = mono_buf.lock().unwrap();
                    buf.clone()
                };
                render::draw_wave(&mut terminal, &samples, current_theme, &status, actual_fps)?;
            }
            Mode::Scope => {
                let samples = {
                    let buf = mono_buf.lock().unwrap();
                    buf.clone()
                };
                render::draw_scope(&mut terminal, &samples, current_theme, &status, actual_fps)?;
            }
        }

        // Draw theme picker overlay on top of the visualizer
        if let Some(ref picker) = theme_picker {
            render::draw_theme_overlay(&mut terminal, theme::THEMES, picker)?;
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    render::cleanup(&mut terminal)?;
    Ok(())
}
