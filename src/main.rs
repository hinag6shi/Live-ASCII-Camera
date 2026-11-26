mod args;
mod camera;
mod config;
mod render;
mod signals;
mod snapshot;

use anyhow::{bail, Context, Result};
use args::{Args, Settings};
use clap::Parser;
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Controls {
    running: bool,
    camera_change: bool,
    pending_snapshots: Vec<PathBuf>,
}

fn parse_snapshot_command(command: &str) -> Option<Option<&str>> {
    if command == "shot" {
        Some(None)
    } else {
        command.strip_prefix("shot ").map(|path| Some(path.trim()))
    }
}

fn queue_snapshot(command: &str, pending: &mut Vec<PathBuf>) {
    let Some(argument) = parse_snapshot_command(command) else {
        return;
    };
    let argument = argument.map(|path| {
        path.strip_prefix('"')
            .and_then(|path| path.strip_suffix('"'))
            .or_else(|| {
                path.strip_prefix('\'')
                    .and_then(|path| path.strip_suffix('\''))
            })
            .unwrap_or(path)
    });
    match snapshot::requested_path(argument) {
        Ok(path) => pending.push(path),
        Err(error) => eprintln!("Cannot take screenshot: {error:#}"),
    }
}

fn process_commands(
    receiver: &mpsc::Receiver<String>,
    settings: &mut Settings,
    controls: &mut Controls,
    args: &Args,
    config_path: &std::path::Path,
    input: &str,
) {
    while let Ok(line) = receiver.try_recv() {
        let command = line.trim();
        if parse_snapshot_command(command).is_some() {
            queue_snapshot(command, &mut controls.pending_snapshots);
            continue;
        }
        match command {
            "1" => settings.ramp = args::RampPreset::Classic.chars().chars().collect(),
            "2" => settings.ramp = args::RampPreset::Dense.chars().chars().collect(),
            "3" => settings.ramp = args::RampPreset::Binary.chars().chars().collect(),
            "+" | "=" => settings.cell = settings.cell.saturating_sub(1).max(8),
            "-" => settings.cell = settings.cell.saturating_add(1).min(settings.max_cell),
            "b" => settings.brightness = (settings.brightness + 0.1).min(2.0),
            "B" => settings.brightness = (settings.brightness - 0.1).max(0.5),
            "s" => settings.saturation = (settings.saturation + 0.25).min(5.0),
            "S" => settings.saturation = (settings.saturation - 0.25).max(0.0),
            "m" => settings.mirror = !settings.mirror,
            "i" => settings.invert = !settings.invert,
            "c" => settings.color = settings.color.next(),
            "v" => controls.camera_change = true,
            "save" => match config::save(config_path, args, settings, input) {
                Ok(()) => eprintln!("Settings saved to {}", config_path.display()),
                Err(error) => eprintln!("Could not save settings: {error:#}"),
            },
            "q" | "quit" => controls.running = false,
            "" => {}
            "help" => eprintln!("1/2/3 ramp, +/- cell size, b/B brightness, s/S saturation, m mirror, i invert, c color, v next camera, shot [file.png], save, q quit"),
            other => eprintln!("Unknown command '{other}'. Type help + Enter for controls."),
        }
    }
}

fn save_pending_snapshots(paths: &mut Vec<PathBuf>, frame: &[u8], width: usize, height: usize) {
    for path in paths.drain(..) {
        match snapshot::save_png(frame, width, height, &path) {
            Ok(()) => eprintln!("Saved PNG screenshot: {}", path.display()),
            Err(error) => eprintln!("Could not save {}: {error:#}", path.display()),
        }
    }
}

fn main() -> Result<()> {
    signals::install();
    let mut args = Args::parse();
    config::load(&mut args)?;

    if args.width == 0 || args.height == 0 || args.fps == 0 || args.cell < 8 {
        bail!("width, height and fps must be positive; cell must be at least 8");
    }
    if args.width % 2 != 0 {
        bail!("output width must be even because the virtual camera uses YUYV422");
    }
    let frame_len = args
        .width
        .checked_mul(args.height)
        .and_then(|pixels| pixels.checked_mul(3))
        .context("output dimensions are too large")?;
    if args.cell > args.width.min(args.height) {
        bail!(
            "cell must not exceed the smaller output dimension ({} pixels)",
            args.width.min(args.height)
        );
    }
    let ramp: Vec<char> = args
        .ramp
        .as_deref()
        .unwrap_or_else(|| args.ramp_preset.chars())
        .chars()
        .collect();
    if ramp.is_empty() {
        bail!("ramp cannot be empty");
    }
    if ramp
        .iter()
        .any(|ch| !ch.is_ascii() || (!ch.is_ascii_graphic() && *ch != ' '))
    {
        bail!("ramp must contain printable ASCII characters only");
    }
    if !args.saturation.is_finite() || !(0.0..=5.0).contains(&args.saturation) {
        bail!("saturation must be between 0 and 5");
    }
    if !args.brightness.is_finite() || !(0.5..=2.0).contains(&args.brightness) {
        bail!("brightness must be between 0.5 and 2");
    }
    if args.name.trim().is_empty()
        || args.name.len() > 31
        || args.name.chars().any(char::is_control)
    {
        bail!("camera name must contain 1 to 31 UTF-8 bytes");
    }
    if args.input.is_empty() || args.input.chars().any(char::is_control) {
        bail!("input device path cannot be empty or contain control characters");
    }
    if args.input != "auto" && !std::path::Path::new(&args.input).exists() {
        bail!("input device {} does not exist", args.input);
    }

    let settings_path = config::path_for(&args);
    let mut settings = Settings {
        cell: args.cell,
        max_cell: args.width.min(args.height),
        saturation: args.saturation,
        brightness: args.brightness,
        mirror: args.mirror,
        invert: args.invert,
        color: args.color,
        ramp,
    };
    let devices = camera::available_inputs();
    let mut input_path = if args.input == "auto" {
        devices
            .first()
            .cloned()
            .context("no physical V4L2 camera found; specify one with --input /dev/videoN")?
    } else {
        args.input.clone()
    };
    eprintln!(
        "Available cameras: {}",
        if devices.is_empty() {
            "none detected".to_owned()
        } else {
            devices.join(", ")
        }
    );

    let output = camera::create_virtual(&args)?;
    let _virtual_camera = camera::VirtualCamera {
        path: output.clone(),
    };
    let mut capture = camera::start_capture(&input_path, &args)?;
    let mut frames = camera::read_frames(capture.stdout.take().unwrap(), frame_len);

    let mut publisher = Command::new("ffmpeg");
    signals::ignore_in_child(&mut publisher);
    let publish_result = publisher
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-video_size",
        ])
        .arg(format!("{}x{}", args.width, args.height))
        .args(["-framerate", &args.fps.to_string(), "-i", "-"])
        .args(["-f", "v4l2", "-pix_fmt", "yuyv422", "-video_size"])
        .arg(format!("{}x{}", args.width, args.height))
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    let mut publisher = match publish_result {
        Ok(child) => child,
        Err(error) => {
            drop(frames);
            let _ = capture.kill();
            let _ = capture.wait();
            return Err(error).context("failed to start ffmpeg output; install ffmpeg");
        }
    };
    let mut sink = publisher
        .stdin
        .take()
        .context("ffmpeg output stdin was unavailable")?;
    let (command_tx, command_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if command_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut frame = vec![0u8; frame_len];
    let mut has_frame = false;
    let mut controls = Controls {
        running: true,
        camera_change: false,
        pending_snapshots: Vec::new(),
    };
    eprintln!(
        "ASCII camera: {} -> {} ({}x{} @ {} fps). Type help + Enter for controls.",
        input_path, output, args.width, args.height, args.fps
    );

    let run_result = loop {
        controls.camera_change = false;
        process_commands(
            &command_rx,
            &mut settings,
            &mut controls,
            &args,
            &settings_path,
            &input_path,
        );
        if has_frame && !controls.pending_snapshots.is_empty() {
            save_pending_snapshots(
                &mut controls.pending_snapshots,
                &frame,
                args.width,
                args.height,
            );
        }
        if !controls.running {
            break Ok(());
        }
        if controls.camera_change && !devices.is_empty() {
            let current = devices
                .iter()
                .position(|device| device == &input_path)
                .unwrap_or(usize::MAX);
            let next = devices[(current.wrapping_add(1)) % devices.len()].clone();
            if next != input_path {
                let next_capture = match camera::start_capture(&next, &args) {
                    Ok(child) => child,
                    Err(error) => break Err(error),
                };
                drop(frames);
                let _ = capture.kill();
                let _ = capture.wait();
                capture = next_capture;
                frames = camera::read_frames(capture.stdout.take().unwrap(), frame_len);
                input_path = next;
                eprintln!("Switched input camera to {input_path}");
            }
        }
        if signals::should_stop() {
            break Ok(());
        }
        match publisher.try_wait() {
            Ok(Some(status)) => {
                break Err(anyhow::anyhow!("ffmpeg could not open {} (exit status {status}); create the v4l2loopback device first", output));
            }
            Ok(None) => {}
            Err(error) => break Err(error).context("checking ffmpeg output"),
        }
        let input = match frames.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(frame)) => frame,
            Ok(Err(_error)) if signals::should_stop() => break Ok(()),
            Ok(Err(error)) => break Err(anyhow::anyhow!("camera stream ended: {error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) if signals::should_stop() => break Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(anyhow::anyhow!("camera frame reader stopped"))
            }
        };
        render::render(&input, &mut frame, args.width, args.height, &settings);
        has_frame = true;
        save_pending_snapshots(
            &mut controls.pending_snapshots,
            &frame,
            args.width,
            args.height,
        );
        if let Err(error) = sink.write_all(&frame) {
            if signals::should_stop() {
                break Ok(());
            }
            break Err(error).context("writing virtual-camera frame");
        }
    };

    drop(sink);
    drop(frames);
    let _ = capture.kill();
    let capture_result = capture.wait();
    let publish_result = publisher.wait();
    if !controls.pending_snapshots.is_empty() {
        eprintln!(
            "Skipped {} screenshot(s): no rendered frame was available.",
            controls.pending_snapshots.len()
        );
    }
    run_result?;
    let capture_status = capture_result.context("waiting for camera capture process")?;
    let publish_status = publish_result.context("waiting for virtual camera process")?;
    if !signals::should_stop()
        && controls.running
        && (!capture_status.success() || !publish_status.success())
    {
        bail!("an ffmpeg process exited unsuccessfully (capture: {capture_status}, output: {publish_status})");
    }
    if signals::should_stop() || !controls.running {
        eprintln!("Stopped.");
    }
    Ok(())
}
