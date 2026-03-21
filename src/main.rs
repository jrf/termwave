mod analysis;
mod audio;
mod config;
mod render;
mod theme;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;

/// Global flag set by SIGINT/SIGTERM handlers to trigger clean shutdown.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

const SIGINT: std::ffi::c_int = 2;
const SIGTERM: std::ffi::c_int = 15;

extern "C" {
    fn signal(sig: std::ffi::c_int, handler: usize) -> usize;
}

extern "C" fn shutdown_handler(_sig: std::ffi::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

#[derive(Parser)]
#[command(name = "termwave", about = "Terminal audio visualizer")]
struct Cli {
    /// Visualization mode
    #[arg(short, long)]
    mode: Option<String>,

    /// Audio input device (defaults to system audio via ScreenCaptureKit)
    #[arg(short, long)]
    device: Option<String>,

    /// Color theme (loaded from ~/.config/termwave/themes/)
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
const MIN_BARS: usize = 4;
const SENS_STEP: u32 = 10;
/// Gravity acceleration in units/s². At 60fps (dt≈0.017s), a bar at height 1.0
/// takes about 0.25s to fall — similar feel to the old per-frame 0.01 value.
const GRAVITY_ACCEL: f32 = 8.0;
/// Cooldown between tap restart attempts.
const RESTART_COOLDOWN: Duration = Duration::from_secs(2);
/// Audio is considered stale after this long without new samples.
const STALE_AUDIO_THRESHOLD: Duration = Duration::from_millis(100);

/// Bundles audio capture state passed between functions.
struct AudioState {
    mono_buf: audio::SampleBuffer,
    stereo: audio::StereoPair,
    last_write: audio::LastWriteTime,
    device_name: String,
    sample_rate: u32,
    capture: audio::CaptureHandle,
    last_restart_attempt: Option<Instant>,
}

/// Bundles all mutable DSP and bar state for the visualizer.
struct VisualizerState {
    num_bars: usize,
    prev_bars: Vec<f32>,
    prev_left: Vec<f32>,
    prev_right: Vec<f32>,
    autosens: analysis::AutoSensitivity,
    autosens_l: analysis::AutoSensitivity,
    autosens_r: analysis::AutoSensitivity,
    gravity: analysis::Gravity,
    gravity_l: analysis::Gravity,
    gravity_r: analysis::Gravity,
    analyzer: analysis::SpectrumAnalyzer,
}

impl VisualizerState {
    fn new() -> Self {
        Self {
            num_bars: MIN_BARS,
            prev_bars: vec![0.0; MIN_BARS],
            prev_left: vec![0.0; MIN_BARS],
            prev_right: vec![0.0; MIN_BARS],
            autosens: analysis::AutoSensitivity::new(),
            autosens_l: analysis::AutoSensitivity::new(),
            autosens_r: analysis::AutoSensitivity::new(),
            gravity: analysis::Gravity::new(GRAVITY_ACCEL),
            gravity_l: analysis::Gravity::new(GRAVITY_ACCEL),
            gravity_r: analysis::Gravity::new(GRAVITY_ACCEL),
            analyzer: analysis::SpectrumAnalyzer::new(),
        }
    }

    /// Reset bar buffers (e.g. on mode change, device switch, or bar count change).
    fn reset_bars(&mut self) {
        self.prev_bars = vec![0.0; self.num_bars];
        self.prev_left = vec![0.0; self.num_bars];
        self.prev_right = vec![0.0; self.num_bars];
    }

    /// Reset auto-sensitivity trackers (e.g. on device switch or silence).
    fn reset_sensitivity(&mut self) {
        self.autosens = analysis::AutoSensitivity::new();
        self.autosens_l = analysis::AutoSensitivity::new();
        self.autosens_r = analysis::AutoSensitivity::new();
    }

    /// Resize bar count if it differs from current, resetting buffers.
    fn resize_bars(&mut self, new_count: usize) {
        if new_count != self.num_bars {
            self.num_bars = new_count;
            self.reset_bars();
        }
    }

    /// Run the shared DSP pipeline on a single channel: monstercat → noise gate →
    /// auto-sensitivity → manual sensitivity → smooth → gravity.
    ///
    /// Smoothing runs after normalization so that `prev` and `current` are both
    /// in the same 0.0–1.0 scale, avoiding the scale mismatch that previously
    /// caused sluggish transient response.
    fn process_channel(
        prev: &mut Vec<f32>,
        raw_bars: &[f32],
        settings: &render::Settings,
        dt: f32,
        autosens: &mut analysis::AutoSensitivity,
        gravity: &mut analysis::Gravity,
    ) -> Vec<f32> {
        let mut bars = raw_bars.to_vec();
        if settings.monstercat {
            analysis::monstercat(&mut bars, MONSTERCAT_STRENGTH);
        }
        if settings.noise_floor > 0.0 {
            analysis::noise_gate(&mut bars, settings.noise_floor);
        }
        autosens.apply(&mut bars, dt);
        let sens = settings.sensitivity as f32 / 100.0;
        if sens != 1.0 {
            for v in bars.iter_mut() {
                *v *= sens;
            }
        }
        // Smooth after normalization so both prev and bars are in 0.0–1.0 scale
        let mut smoothed = analysis::smooth(prev, &bars, settings.smoothing, dt);
        *prev = smoothed.clone();
        gravity.apply(&mut smoothed, dt);
        smoothed
    }

    /// Process mono spectrum and return render-ready bars.
    fn process_spectrum(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        low_freq: f32,
        high_freq: f32,
        settings: &render::Settings,
        dt: f32,
    ) -> Vec<f32> {
        let (bass_mag, main_mag) = self.analyzer.spectrum_dual(samples);
        let bars = analysis::bin_spectrum(
            &bass_mag,
            &main_mag,
            self.num_bars,
            sample_rate,
            low_freq,
            high_freq,
        );
        Self::process_channel(
            &mut self.prev_bars,
            &bars,
            settings,
            dt,
            &mut self.autosens,
            &mut self.gravity,
        )
    }

    /// Process stereo spectrum and return render-ready (left, right) bars.
    #[allow(clippy::too_many_arguments)]
    fn process_stereo(
        &mut self,
        left_samples: &[f32],
        right_samples: &[f32],
        sample_rate: u32,
        low_freq: f32,
        high_freq: f32,
        settings: &render::Settings,
        dt: f32,
    ) -> (Vec<f32>, Vec<f32>) {
        let (left_bass, left_main) = self.analyzer.spectrum_dual(left_samples);
        let (right_bass, right_main) = self.analyzer.spectrum_dual(right_samples);
        let left_bars = analysis::bin_spectrum(
            &left_bass,
            &left_main,
            self.num_bars,
            sample_rate,
            low_freq,
            high_freq,
        );
        let right_bars = analysis::bin_spectrum(
            &right_bass,
            &right_main,
            self.num_bars,
            sample_rate,
            low_freq,
            high_freq,
        );

        let smooth_l = Self::process_channel(
            &mut self.prev_left,
            &left_bars,
            settings,
            dt,
            &mut self.autosens_l,
            &mut self.gravity_l,
        );
        let smooth_r = Self::process_channel(
            &mut self.prev_right,
            &right_bars,
            settings,
            dt,
            &mut self.autosens_r,
            &mut self.gravity_r,
        );
        (smooth_l, smooth_r)
    }
}

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
    cfg.mode = mode.as_str().to_string();

    cfg.gradient_by_position = settings.gradient_by_position;
    cfg.bar_width = settings.bar_width;
    cfg.bar_spacing = settings.bar_spacing;
    cfg.sensitivity = settings.sensitivity;

    let _ = config::save(cfg);
}

/// What the main loop should do after handling input.
enum LoopAction {
    Continue,
    Skip,
    Quit,
}

/// Handle input when the settings overlay is open.
fn handle_settings_input(
    settings_state: &mut Option<render::SettingsState>,
    settings: &mut render::Settings,
    themes: &[theme::Theme],
    theme_idx: &mut usize,
    cfg: &mut config::Config,
    mode: &Mode,
) -> Result<LoopAction> {
    if let Some(ref mut sstate) = settings_state {
        if let Some(key) = render::poll_key(Duration::ZERO)? {
            match sstate.handle_key(key, settings, themes.len()) {
                render::SettingsAction::Close => {
                    *theme_idx = settings.theme_idx;
                    save_state(cfg, settings, &themes[*theme_idx].name, mode);
                    *settings_state = None;
                }
                render::SettingsAction::Quit => return Ok(LoopAction::Quit),
                render::SettingsAction::None => {
                    *theme_idx = settings.theme_idx;
                }
            }
        }
    }
    Ok(LoopAction::Continue)
}

/// Handle input when no overlay is open. Returns LoopAction.
#[allow(clippy::too_many_arguments)]
fn handle_normal_input(
    vis: &mut VisualizerState,
    settings: &mut render::Settings,
    settings_state: &mut Option<render::SettingsState>,
    mode: &mut Mode,
    terminal: &mut render::Term,
    themes: &[theme::Theme],
    theme_idx: usize,
    cfg: &mut config::Config,
    audio: &mut AudioState,
) -> Result<LoopAction> {
    match render::poll_input(Duration::ZERO)? {
        render::Action::Quit => return Ok(LoopAction::Quit),
        render::Action::CycleMode => {
            *mode = mode.next();
            vis.reset_bars();
            save_state(cfg, settings, &themes[theme_idx].name, mode);
            return Ok(LoopAction::Skip);
        }
        render::Action::SelectDevice => {
            let devices = audio::list_devices()?;
            match render::device_menu(terminal, &devices, &themes[theme_idx])? {
                render::DeviceMenuResult::Selected(new_device) => {
                    drop_capture(&mut audio.capture);
                    let (sr, handle) = start_audio(
                        &audio.mono_buf,
                        &audio.stereo,
                        new_device.as_deref(),
                        &audio.last_write,
                    )?;
                    audio.sample_rate = sr;
                    audio.capture = handle;
                    audio.device_name =
                        new_device.unwrap_or_else(|| audio::SYSTEM_AUDIO_LABEL.to_string());
                    vis.reset_bars();
                    vis.reset_sensitivity();
                }
                render::DeviceMenuResult::Quit => return Ok(LoopAction::Quit),
                render::DeviceMenuResult::Cancelled => {}
            }
            return Ok(LoopAction::Skip);
        }
        render::Action::Settings => {
            *settings_state = Some(render::SettingsState::new());
        }
        render::Action::Help => {
            render::help(terminal, &themes[theme_idx])?;
            return Ok(LoopAction::Skip);
        }
        render::Action::SensUp => {
            settings.sensitivity = (settings.sensitivity + SENS_STEP).min(500);
            save_state(cfg, settings, &themes[theme_idx].name, mode);
            return Ok(LoopAction::Skip);
        }
        render::Action::SensDown => {
            settings.sensitivity = settings.sensitivity.saturating_sub(SENS_STEP).max(10);
            save_state(cfg, settings, &themes[theme_idx].name, mode);
            return Ok(LoopAction::Skip);
        }
        render::Action::MoreBars => {
            // Narrower bars = more bars on screen
            settings.bar_width = settings.bar_width.saturating_sub(1).max(1);
            save_state(cfg, settings, &themes[theme_idx].name, mode);
            return Ok(LoopAction::Skip);
        }
        render::Action::FewerBars => {
            // Wider bars = fewer bars on screen
            settings.bar_width = (settings.bar_width + 1).min(8);
            save_state(cfg, settings, &themes[theme_idx].name, mode);
            return Ok(LoopAction::Skip);
        }
        render::Action::None => {}
    }
    Ok(LoopAction::Continue)
}

/// Drop old capture handle safely by replacing with a dummy.
fn drop_capture(capture: &mut audio::CaptureHandle) {
    let old = std::mem::replace(
        capture,
        audio::CaptureHandle::Tap(
            std::process::Command::new("true")
                .spawn()
                .expect("failed to spawn dummy process"),
        ),
    );
    drop(old);
}

/// Detect stale audio and silence, resetting state as needed.
fn check_audio_health(vis: &mut VisualizerState, audio: &mut AudioState) {
    // Zero buffers if no new samples recently
    if audio.last_write.elapsed() > STALE_AUDIO_THRESHOLD {
        audio.mono_buf.lock().unwrap().fill(0.0);
        audio.stereo.0.lock().unwrap().fill(0.0);
        audio.stereo.1.lock().unwrap().fill(0.0);
    }

    // Auto-restart tap if it died
    if audio.capture.tap_exited() {
        let should_restart = audio
            .last_restart_attempt
            .map(|t| t.elapsed() >= RESTART_COOLDOWN)
            .unwrap_or(true);
        if should_restart {
            audio.last_restart_attempt = Some(Instant::now());
            drop_capture(&mut audio.capture);
            match start_audio(
                &audio.mono_buf,
                &audio.stereo,
                Some(&audio.device_name),
                &audio.last_write,
            ) {
                Ok((sr, handle)) => {
                    audio.sample_rate = sr;
                    audio.capture = handle;
                    vis.reset_sensitivity();
                }
                Err(_) => {
                    // Keep the dummy handle; we'll retry after cooldown
                }
            }
        }
    }

    // Detect silence: zero smoothing state so bars fall via gravity
    let buf = audio.mono_buf.lock().unwrap();
    let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
    if rms < 1e-6 {
        drop(buf);
        vis.prev_bars.fill(0.0);
        vis.prev_left.fill(0.0);
        vis.prev_right.fill(0.0);
        vis.reset_sensitivity();
    }
}

fn main() -> Result<()> {
    // Install signal handlers so the tap subprocess gets cleaned up on SIGINT/SIGTERM.
    // Without this, sleeping/waking or killing termwave leaves termwave-tap orphaned.
    unsafe {
        signal(SIGINT, shutdown_handler as usize);
        signal(SIGTERM, shutdown_handler as usize);
    }

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
    let now_playing = audio::new_now_playing();
    let mut audio = {
        let mono_buf = audio::new_buffer(analysis::FFT_SIZE);
        let stereo = audio::new_stereo_buffers(analysis::FFT_SIZE);
        let last_write = audio::LastWriteTime::new();
        let device_name = cli
            .device
            .clone()
            .unwrap_or_else(|| audio::SYSTEM_AUDIO_LABEL.to_string());
        let (sample_rate, capture) =
            start_audio(&mono_buf, &stereo, cli.device.as_deref(), &last_write)?;
        AudioState {
            mono_buf,
            stereo,
            last_write,
            device_name,
            sample_rate,
            capture,
            last_restart_attempt: None,
        }
    };

    // Poll now-playing in a background thread via osascript
    audio::start_now_playing_poller(now_playing.clone(), Duration::from_secs(3));

    // Init terminal
    let mut terminal = render::init()?;
    let fps = cfg.fps.max(1);
    let frame_duration = Duration::from_millis(1000 / fps);
    let low_freq = cfg.low_freq;
    let high_freq = cfg.high_freq;
    let themes = theme::load_themes();
    if themes.is_empty() {
        anyhow::bail!("No theme files found in ~/.config/termwave/themes/");
    }
    let mut theme_idx = themes
        .iter()
        .position(|t| t.name == cfg.theme)
        .unwrap_or(0);

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

    let mut vis = VisualizerState::new();

    // FPS tracking
    let mut frame_count: u32 = 0;
    let mut fps_timer = Instant::now();
    let mut actual_fps: Option<u32> = None;

    // Frame-rate independent timing
    let mut last_frame_time = Instant::now();

    // Settings overlay state (None = closed)
    let mut settings_state: Option<render::SettingsState> = None;

    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }

        let frame_start = Instant::now();

        // FPS counter: update once per second
        frame_count += 1;
        if fps_timer.elapsed() >= Duration::from_secs(1) {
            actual_fps = Some(frame_count);
            frame_count = 0;
            fps_timer = Instant::now();
        }

        // Compute bar count from terminal width.
        // Bar width and spacing are authoritative; count fills the terminal.
        let term_w = terminal.size()?.width.saturating_sub(2) as usize;
        let stride = settings.bar_width + settings.bar_spacing;
        let num_bars = if stride > 0 {
            (term_w + settings.bar_spacing) / stride
        } else {
            term_w
        }.max(MIN_BARS);
        vis.resize_bars(num_bars);

        // Input handling
        if settings_state.is_some() {
            match handle_settings_input(
                &mut settings_state,
                &mut settings,
                &themes,
                &mut theme_idx,
                &mut cfg,
                &mode,
            )? {
                LoopAction::Quit => break,
                LoopAction::Skip => continue,
                LoopAction::Continue => {}
            }
        } else {
            match handle_normal_input(
                &mut vis,
                &mut settings,
                &mut settings_state,
                &mut mode,
                &mut terminal,
                &themes,
                theme_idx,
                &mut cfg,
                &mut audio,
            )? {
                LoopAction::Quit => break,
                LoopAction::Skip => continue,
                LoopAction::Continue => {}
            }
        }

        let dt = last_frame_time.elapsed().as_secs_f32().clamp(0.001, 0.1);
        last_frame_time = Instant::now();

        // Audio health checks (stale data, tap crashes, silence)
        check_audio_health(&mut vis, &mut audio);

        // Build status line
        let status = match now_playing.lock().unwrap().as_deref() {
            Some(track) => format!("{} | {}", audio.device_name, track),
            None => audio.device_name.clone(),
        };

        let current_theme = &themes[theme_idx];

        // Prepare render data
        #[allow(clippy::enum_variant_names)]
        enum RenderData {
            Spectrum(Vec<f32>),
            Stereo(Vec<f32>, Vec<f32>),
            Wave(Vec<f32>),
            Scope(Vec<f32>),
        }

        let render_data = match mode {
            Mode::Spectrum => {
                let samples = audio.mono_buf.lock().unwrap().clone();
                let bars = vis.process_spectrum(
                    &samples, audio.sample_rate, low_freq, high_freq, &settings, dt,
                );
                RenderData::Spectrum(bars)
            }
            Mode::Stereo => {
                let left_samples = audio.stereo.0.lock().unwrap().clone();
                let right_samples = audio.stereo.1.lock().unwrap().clone();
                let (left, right) = vis.process_stereo(
                    &left_samples, &right_samples, audio.sample_rate, low_freq, high_freq, &settings, dt,
                );
                RenderData::Stereo(left, right)
            }
            Mode::Wave => {
                let samples = audio.mono_buf.lock().unwrap().clone();
                RenderData::Wave(samples)
            }
            Mode::Scope => {
                let samples = audio.mono_buf.lock().unwrap().clone();
                RenderData::Scope(samples)
            }
        };

        // Single terminal.draw call: visualizer + optional settings overlay
        let ctx = render::RenderContext {
            theme: current_theme,
            device: &status,
            gradient_by_position: settings.gradient_by_position,
            actual_fps,
            bar_width: settings.bar_width,
            bar_spacing: settings.bar_spacing,
            sensitivity: settings.sensitivity,
        };
        let settings_ref = &settings;
        let sstate_ref = &settings_state;
        terminal.draw(|frame| {
            match &render_data {
                RenderData::Spectrum(smoothed) => {
                    render::render_spectrum(frame, smoothed, &ctx);
                }
                RenderData::Stereo(smooth_l, smooth_r) => {
                    render::render_stereo(frame, smooth_l, smooth_r, &ctx);
                }
                RenderData::Wave(samples) => {
                    render::render_wave(frame, samples, current_theme, &status, actual_fps);
                }
                RenderData::Scope(samples) => {
                    render::render_scope(frame, samples, current_theme, &status, actual_fps);
                }
            }

            // Settings overlay on top
            if let Some(ref sstate) = sstate_ref {
                render::render_settings(frame, settings_ref, &themes, sstate);
            }
        })?;

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    render::cleanup(&mut terminal)?;
    Ok(())
}
