mod analysis;
mod audio;
mod render;
mod theme;

use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "sonitus", about = "Terminal audio visualizer")]
struct Cli {
    /// Visualization mode
    #[arg(short, long, default_value = "spectrum")]
    mode: Mode,

    /// Audio input device (defaults to system default, use "system" for system audio)
    #[arg(short, long)]
    device: Option<String>,

    /// Color theme (classic, fire, ocean, purple, matrix, mono)
    #[arg(short, long, default_value = "classic")]
    theme: String,

    /// List available audio input devices
    #[arg(long)]
    list_devices: bool,
}

#[derive(Clone, clap::ValueEnum)]
enum Mode {
    /// Spectrum analyzer (frequency bars)
    Spectrum,
    /// Waveform (amplitude over time)
    Wave,
    /// Oscilloscope (triggered waveform)
    Scope,
}

const TARGET_FPS: u64 = 30;
const SMOOTHING_FACTOR: f32 = 0.7;
const NUM_BARS: usize = 64;
const DEFAULT_SAMPLE_RATE: u32 = 48000;

fn start_audio(
    buffer: &audio::SampleBuffer,
    device: Option<&str>,
) -> Result<(u32, audio::CaptureHandle)> {
    if device == Some(audio::SYSTEM_AUDIO_LABEL) || device == Some("system") {
        audio::start_tap(buffer.clone(), DEFAULT_SAMPLE_RATE)
    } else {
        audio::start_capture(buffer.clone(), device)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.list_devices {
        for name in audio::list_devices()? {
            println!("{}", name);
        }
        return Ok(());
    }

    // Start audio capture with default or specified device
    let buffer = audio::new_buffer(analysis::FFT_SIZE);
    let (mut sample_rate, mut capture) =
        start_audio(&buffer, cli.device.as_deref())?;

    // Init terminal
    let mut terminal = render::init()?;
    let frame_duration = Duration::from_millis(1000 / TARGET_FPS);
    let mut prev_bars: Vec<f32> = vec![0.0; NUM_BARS];

    let mut current_theme = theme::by_name(&cli.theme);
    let mut theme_idx = theme::THEMES
        .iter()
        .position(|t| t.name == current_theme.name)
        .unwrap_or(0);

    loop {
        let frame_start = Instant::now();

        // Poll input
        match render::poll_input(Duration::ZERO)? {
            render::Action::Quit => break,
            render::Action::SelectDevice => {
                let devices = audio::list_devices()?;
                match render::device_menu(&mut terminal, &devices)? {
                    render::DeviceMenuResult::Selected(new_device) => {
                        drop(capture);
                        let (sr, handle) =
                            start_audio(&buffer, new_device.as_deref())?;
                        sample_rate = sr;
                        capture = handle;
                        prev_bars = vec![0.0; NUM_BARS];
                    }
                    render::DeviceMenuResult::Quit => break,
                    render::DeviceMenuResult::Cancelled => {}
                }
                continue;
            }
            render::Action::SelectTheme => {
                match render::theme_menu(&mut terminal, theme::THEMES, theme_idx)? {
                    render::ThemeMenuResult::Selected(idx) => {
                        theme_idx = idx;
                        current_theme = &theme::THEMES[idx];
                    }
                    render::ThemeMenuResult::Quit => break,
                    render::ThemeMenuResult::Cancelled => {}
                }
                continue;
            }
            render::Action::Help => {
                render::help(&mut terminal)?;
                continue;
            }
            render::Action::None => {}
        }

        // Snapshot the current audio buffer
        let samples = {
            let buf = buffer.lock().unwrap();
            buf.clone()
        };

        match cli.mode {
            Mode::Spectrum => {
                let magnitudes = analysis::spectrum(&samples);
                let bars = analysis::bin_spectrum(&magnitudes, NUM_BARS, sample_rate);
                let smoothed = analysis::smooth(&prev_bars, &bars, SMOOTHING_FACTOR);
                render::draw_spectrum(&mut terminal, &smoothed, current_theme)?;
                prev_bars = smoothed;
            }
            Mode::Wave => {
                render::draw_wave(&mut terminal, &samples, current_theme)?;
            }
            Mode::Scope => {
                render::draw_scope(&mut terminal, &samples, current_theme)?;
            }
        }

        // Frame rate limiting
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    render::cleanup(&mut terminal)?;
    Ok(())
}
