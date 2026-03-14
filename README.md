# sonitus

Terminal audio visualizer for macOS. Renders real-time spectrum bars, waveforms, and oscilloscopes from mic input or system audio.

## Install

Requires Rust and (optionally) Swift for system audio capture.

```
just
```

This builds and installs `sonitus` and `sonitus-tap` to `~/.cargo/bin`.

## Usage

```
sonitus                        # spectrum visualizer using default mic
sonitus --mode wave            # waveform mode
sonitus --mode scope           # oscilloscope mode
sonitus --device "system"      # capture system audio (requires sonitus-tap)
sonitus --theme fire           # use the fire color theme
sonitus --list-devices         # list available audio devices
```

## Keybindings

| Key | Action |
|-----|--------|
| `?` | Help |
| `d` | Select audio device |
| `t` | Select color theme |
| `q` / `Esc` | Quit |
| `Ctrl+C` | Quit |

## Themes

Six built-in color themes, selectable via `--theme` or `t` at runtime:

- **classic** — blue, cyan, green, yellow, red
- **fire** — dark red to bright yellow
- **ocean** — deep navy to bright aqua
- **purple** — dark violet to pink
- **matrix** — green monochrome
- **mono** — grayscale

## System audio capture

To visualize audio from Apple Music or other apps, sonitus uses a companion Swift binary (`sonitus-tap`) that captures system audio via ScreenCaptureKit.

**Requirements:**
- macOS 13+
- Screen Recording permission must be granted to your terminal app (System Settings > Privacy & Security > Screen Recording)

Select "System Audio (ScreenCaptureKit)" from the device menu (`d`), or pass `--device system`.

**Alternative:** Install [BlackHole](https://github.com/ExistentialAudio/BlackHole), create a Multi-Output Device in Audio MIDI Setup (speakers + BlackHole), set it as your output, then select "BlackHole 2ch" as the input device.

## Architecture

```
sonitus (Rust)
├── audio.rs      — cpal device capture + sonitus-tap subprocess management
├── analysis.rs   — Hann-windowed FFT, logarithmic frequency binning, smoothing
├── render.rs     — ratatui terminal UI (spectrum, waveform, oscilloscope, menus)
├── theme.rs      — color theme definitions
└── main.rs       — CLI, main loop (audio -> FFT -> render @ 30fps)

tap/ (Swift)
└── sonitus-tap   — ScreenCaptureKit system audio capture, outputs raw f32 to stdout
```
