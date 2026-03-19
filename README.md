# termwave

[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Swift](https://img.shields.io/badge/swift-6.2+-F05138?logo=swift&logoColor=white)](https://swift.org/)
[![macOS](https://img.shields.io/badge/macOS-13%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)

Terminal audio visualizer for macOS. Renders real-time spectrum bars, waveforms, oscilloscopes, and stereo visualizations from mic input or system audio.

## Install

Requires Rust and (optionally) Swift for system audio capture.

```
just
```

This builds and installs `termwave` and `termwave-tap` to `~/.cargo/bin`.

## Usage

```
termwave                        # spectrum visualizer (system audio by default)
termwave --mode wave            # waveform mode
termwave --mode scope           # oscilloscope mode
termwave --mode stereo          # stereo L/R visualization
termwave --device "system"      # capture system audio (requires termwave-tap)
termwave --theme fire           # use the fire color theme
termwave --bars 128             # set number of spectrum bars
termwave --list-devices         # list available audio devices
```

## Keybindings

| Key | Action |
|-----|--------|
| `?` | Help |
| `m` | Cycle visualization mode |
| `d` | Select audio device |
| `t` | Select color theme |
| `s` | Settings menu |
| `Up` / `+` | More bars |
| `Down` / `-` | Fewer bars |
| `q` / `Esc` | Quit |

## Modes

- **spectrum** — frequency bars with color gradient and gravity fall-off
- **wave** — real-time waveform amplitude plot
- **scope** — oscilloscope with zero-crossing trigger
- **stereo** — left channel bars up, right channel bars down from center

| | |
|---|---|
| ![spectrum](spectrum.png) | ![waveform](waveform.png) |
| ![oscilloscope](oscilloscope.png) | ![stereo](stereo.png) |

## Themes

Seven built-in color themes, selectable via `--theme` or `t` at runtime:

- **classic** — blue, cyan, green, yellow, red
- **fire** — dark red to bright yellow
- **ocean** — deep navy to bright aqua
- **purple** — dark violet to pink
- **matrix** — green monochrome
- **synthwave** — indigo, violet, magenta, pink, orange
- **mono** — grayscale

## Settings

Press `s` to open the settings menu. Adjust with arrow keys or vim bindings:

- **Smoothing** — temporal smoothing between frames (0.0–0.99)
- **Monstercat** — smooth envelope connecting bar tops
- **Noise floor** — threshold to zero out quiet bars
- **Gradient mode** — color by amplitude or by bar position

All settings persist to `~/.config/termwave/config.toml`.

## System audio capture

To visualize audio from Apple Music or other apps, termwave uses a companion Swift binary (`termwave-tap`) that captures system audio via ScreenCaptureKit.

**Requirements:**
- macOS 13+
- Screen Recording permission must be granted to your terminal app (System Settings > Privacy & Security > Screen Recording)

Select "System Audio (ScreenCaptureKit)" from the device menu (`d`), or pass `--device system`.

**Alternative:** Install [BlackHole](https://github.com/ExistentialAudio/BlackHole), create a Multi-Output Device in Audio MIDI Setup (speakers + BlackHole), set it as your output, then select "BlackHole 2ch" as the input device.
