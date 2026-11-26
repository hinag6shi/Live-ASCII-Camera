use crate::{args::Args, signals};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub struct VirtualCamera {
    pub path: String,
}

impl Drop for VirtualCamera {
    fn drop(&mut self) {
        if !Path::new("/sys/module/v4l2loopback").exists() {
            return;
        }
        let is_root = Command::new("id")
            .arg("-u")
            .output()
            .map(|uid| String::from_utf8_lossy(&uid.stdout).trim() == "0")
            .unwrap_or(false);
        let mut last_error = String::new();
        for attempt in 0..6 {
            let result = if is_root {
                Command::new("v4l2loopback-ctl")
                    .args(["delete", &self.path])
                    .output()
            } else {
                Command::new("sudo")
                    .args(["-n", "v4l2loopback-ctl", "delete", &self.path])
                    .output()
            };
            match result {
                Ok(output) if output.status.success() => {
                    eprintln!("Removed virtual camera {}.", self.path);
                    return;
                }
                Ok(output) => {
                    last_error = format!(
                        "{} {}",
                        String::from_utf8_lossy(&output.stderr).trim(),
                        String::from_utf8_lossy(&output.stdout).trim()
                    )
                    .trim()
                    .to_owned();
                    if last_error.to_ascii_lowercase().contains("busy") && attempt < 5 {
                        std::thread::sleep(Duration::from_millis(500));
                    } else {
                        break;
                    }
                }
                Err(error) => {
                    last_error = error.to_string();
                    break;
                }
            }
        }
        eprintln!("Could not remove virtual camera {}: {}. Close apps using it, then run: sudo v4l2loopback-ctl delete {}", self.path, last_error, self.path);
    }
}

pub fn available_inputs() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/sys/class/video4linux") else {
        return Vec::new();
    };
    let mut devices = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let sys_path = fs::canonicalize(entry.path()).ok()?;
            if !name.starts_with("video") || sys_path.to_string_lossy().contains("/virtual/") {
                return None;
            }
            Some((name, sys_path))
        })
        .collect::<Vec<_>>();
    devices.sort_by_key(|(name, _)| {
        name.strip_prefix("video")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });
    devices
        .into_iter()
        .map(|(name, _)| format!("/dev/{name}"))
        .filter(|path| Path::new(path).exists() && is_capture_device(path))
        .collect()
}

fn is_capture_device(path: &str) -> bool {
    let result = Command::new("v4l2-ctl")
        .args(["--device", path, "--all"])
        .output();
    let result = match result {
        Ok(result) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    if !result.status.success() {
        return false;
    }
    let caps = String::from_utf8_lossy(&result.stdout);
    if !caps.contains("Device Caps") && !caps.contains("Capabilities") {
        return true;
    }
    caps.split("Device Caps")
        .last()
        .unwrap_or(&caps)
        .contains("Video Capture")
}

pub fn create_virtual(args: &Args) -> Result<String> {
    let output = match &args.output {
        Some(path) => path.clone(),
        None => (10..64)
            .map(|number| format!("/dev/video{number}"))
            .find(|path| !Path::new(path).exists())
            .context("no free /dev/video device number found (checked 10 through 63)")?,
    };
    let number = output
        .strip_prefix("/dev/video")
        .context("--output must be a device path like /dev/video10")?;
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("--output must be a device path like /dev/video10");
    }
    if Path::new(&output).exists() {
        bail!("{output} already exists; choose another device with --output");
    }

    let uid = Command::new("id")
        .arg("-u")
        .output()
        .context("could not check current user")?;
    let is_root = String::from_utf8_lossy(&uid.stdout).trim() == "0";
    let module_loaded = Path::new("/sys/module/v4l2loopback").exists();
    if Command::new("v4l2loopback-ctl")
        .arg("--version")
        .output()
        .is_err()
    {
        bail!("v4l2loopback-ctl is required; install v4l2loopback-utils");
    }
    let mut command: Vec<String> = if module_loaded {
        vec![
            "v4l2loopback-ctl".into(),
            "add".into(),
            "--name".into(),
            args.name.clone(),
            "--max-width".into(),
            args.width.to_string(),
            "--max-height".into(),
            args.height.to_string(),
            "--exclusive-caps".into(),
            "1".into(),
            output.clone(),
        ]
    } else {
        vec![
            "modprobe".into(),
            "v4l2loopback".into(),
            "devices=1".into(),
            format!("video_nr={number}"),
            format!("card_label={}", args.name),
            "exclusive_caps=1".into(),
            format!("max_width={}", args.width),
            format!("max_height={}", args.height),
        ]
    };
    let program = command.remove(0);
    eprintln!(
        "Creating virtual camera '{}' at {} (administrator permission may be requested)...",
        args.name, output
    );
    let status = if is_root {
        Command::new(&program).args(&command).status()
    } else {
        Command::new("sudo").arg(&program).args(&command).status()
    }
    .with_context(|| format!("could not run {program}; install v4l2loopback-utils"))?;
    if !status.success() {
        bail!("failed to create virtual camera; check that v4l2loopback is installed and sudo is available");
    }
    for _ in 0..20 {
        if Path::new(&output).exists() {
            return Ok(output);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!("creation command succeeded but {output} did not appear")
}

fn child(program: &str, args: &[&str]) -> Result<Child> {
    let mut command = Command::new(program);
    signals::ignore_in_child(&mut command);
    command
        .stdin(Stdio::null())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to start {program}; install ffmpeg"))
}

pub fn start_capture(input: &str, args: &Args) -> Result<Child> {
    let fps = args.fps.to_string();
    let scale = format!("scale={}:{}", args.width, args.height);
    child(
        "ffmpeg",
        &[
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-f",
            "v4l2",
            "-framerate",
            &fps,
            "-i",
            input,
            "-vf",
            &scale,
            "-an",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ],
    )
}

pub fn read_frames(
    stdout: ChildStdout,
    frame_len: usize,
) -> mpsc::Receiver<Result<Vec<u8>, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut stdout = stdout;
        loop {
            let mut frame = vec![0u8; frame_len];
            if let Err(error) = stdout.read_exact(&mut frame) {
                let _ = sender.send(Err(error.to_string()));
                break;
            }
            if sender.send(Ok(frame)).is_err() {
                break;
            }
        }
    });
    receiver
}
