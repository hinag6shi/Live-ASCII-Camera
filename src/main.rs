use anyhow::{bail, Context, Result};
use clap::Parser;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Publish a live ASCII rendering as a Linux virtual camera"
)]
struct Args {
    /// Camera input device (for example /dev/video0)
    #[arg(short, long, default_value = "/dev/video0")]
    input: String,
    /// Name shown by applications in their camera list
    #[arg(long, default_value = "byhinag6shi")]
    name: String,
    /// V4L2 device path; selected automatically if omitted
    #[arg(short, long)]
    output: Option<String>,
    /// Output video width
    #[arg(long, default_value_t = 1280)]
    width: usize,
    /// Output video height
    #[arg(long, default_value_t = 720)]
    height: usize,
    /// Camera capture frame rate
    #[arg(short, long, default_value_t = 30)]
    fps: u32,
    /// ASCII ramp, from dark to bright
    #[arg(long, default_value = " .:-=+*#%@")]
    ramp: String,
    /// Color saturation multiplier (1.0 keeps camera colors, 3.5 is vivid)
    #[arg(long, default_value_t = 3.5)]
    saturation: f32,
    /// Color brightness multiplier
    #[arg(long, default_value_t = 1.12)]
    brightness: f32,
    /// Character cell size in pixels
    #[arg(long, default_value_t = 8)]
    cell: usize,
}

fn create_virtual_camera(args: &Args) -> Result<String> {
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
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
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
    if module_loaded
        && Command::new("v4l2loopback-ctl")
            .arg("--version")
            .output()
            .is_err()
    {
        bail!("v4l2loopback is loaded, but v4l2loopback-ctl is missing; on Arch/CachyOS install it with: sudo pacman -S v4l2loopback-utils");
    }
    let mut cmd: Vec<String> = if module_loaded {
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
    let program = cmd.remove(0);
    eprintln!(
        "Creating virtual camera '{}' at {} (administrator permission may be requested)...",
        args.name, output
    );
    let process = if is_root {
        Command::new(&program).args(&cmd).status()
    } else {
        Command::new("sudo").arg(&program).args(&cmd).status()
    }
    .with_context(|| {
        format!("could not run {program}; install v4l2loopback and v4l2loopback-utils")
    })?;
    if !process.success() {
        bail!("failed to create virtual camera; check that v4l2loopback is installed and sudo is available");
    }
    for _ in 0..20 {
        if Path::new(&output).exists() {
            return Ok(output);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    bail!("creation command succeeded but {output} did not appear")
}

fn child(program: &str, args: &[&str]) -> Result<Child> {
    Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to start {program}; install ffmpeg"))
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.width == 0 || args.height == 0 || args.fps == 0 || args.cell < 8 {
        bail!("width, height and fps must be positive; cell must be at least 8");
    }
    if args.ramp.is_empty() {
        bail!("ramp cannot be empty");
    }
    if !args.saturation.is_finite() || !(0.0..=5.0).contains(&args.saturation) {
        bail!("saturation must be between 0 and 5");
    }
    if !args.brightness.is_finite() || !(0.5..=2.0).contains(&args.brightness) {
        bail!("brightness must be between 0.5 and 2");
    }
    if args.name.trim().is_empty() || args.name.len() > 31 {
        bail!("camera name must contain 1 to 31 UTF-8 bytes");
    }
    let output = create_virtual_camera(&args)?;
    let mut capture = child(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "v4l2",
            "-framerate",
            &args.fps.to_string(),
            "-i",
            &args.input,
            "-vf",
            &format!("scale={}:{}", args.width, args.height),
            "-an",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ],
    )?;
    let mut stdout = capture.stdout.take().unwrap();
    let mut publish = Command::new("ffmpeg")
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
        .spawn()
        .context("failed to start ffmpeg output; install ffmpeg")?;
    let mut sink = publish.stdin.take().unwrap();
    let frame_len = args.width * args.height * 3;
    let mut input = vec![0u8; frame_len];
    let ramp: Vec<char> = args.ramp.chars().collect();
    let mut frame = vec![0u8; frame_len];
    eprintln!(
        "ASCII camera: {} -> {} ({}x{} @ {} fps)",
        args.input, output, args.width, args.height, args.fps
    );

    loop {
        if let Some(status) = publish.try_wait().context("checking ffmpeg output")? {
            let _ = capture.kill();
            bail!("ffmpeg could not open {} (exit status {status}); create the v4l2loopback device first", output);
        }
        if let Err(err) = stdout.read_exact(&mut input) {
            let _ = capture.kill();
            let _ = publish.kill();
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                bail!("camera stream ended");
            }
            return Err(err).context("reading camera frame");
        }
        render(
            &input,
            &mut frame,
            args.width,
            args.height,
            args.cell,
            args.saturation,
            args.brightness,
            &ramp,
        );
        if let Err(err) = sink.write_all(&frame) {
            let _ = capture.kill();
            let status = publish.try_wait().ok().flatten();
            if status.is_none() {
                let _ = publish.kill();
            }
            if let Some(status) = status {
                bail!("ffmpeg stopped with {status}; check that {} exists and accepts {}x{} YUYV frames", output, args.width, args.height);
            }
            return Err(err).context("writing virtual-camera frame");
        }
    }
}

fn render(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    cell: usize,
    saturation: f32,
    brightness: f32,
    ramp: &[char],
) {
    dst.fill(0);
    let cols = width / cell;
    let rows = height / cell;
    for cy in 0..rows {
        for cx in 0..cols {
            // Average one cell of the source image, then draw its luminance glyph.
            let mut rgb = [0u64; 3];
            let mut pixels = 0u64;
            for y in cy * cell..((cy + 1) * cell).min(height) {
                for x in cx * cell..((cx + 1) * cell).min(width) {
                    let p = (y * width + x) * 3;
                    for channel in 0..3 {
                        rgb[channel] += src[p + channel] as u64;
                    }
                    pixels += 1;
                }
            }
            for channel in &mut rgb {
                *channel /= pixels.max(1);
            }
            let lum = ((rgb[0] * 2126 + rgb[1] * 7152 + rgb[2] * 722) / 10000) as usize;
            let ch = ramp[lum * (ramp.len() - 1) / 255];
            if let Some(glyph) = BASIC_FONTS.get(ch) {
                for gy in 0..8 {
                    for gx in 0..8 {
                        if glyph[gy] & (1 << gx) != 0 {
                            let x = cx * cell + gx * cell / 8;
                            let y = cy * cell + gy * cell / 8;
                            if x < width && y < height {
                                let p = (y * width + x) * 3;
                                let color = vivid_color(
                                    [rgb[0] as u8, rgb[1] as u8, rgb[2] as u8],
                                    saturation,
                                    brightness,
                                );
                                dst[p..p + 3].copy_from_slice(&color);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn vivid_color(rgb: [u8; 3], saturation_gain: f32, brightness: f32) -> [u8; 3] {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let value = (max * brightness).clamp(0.0, 1.0);
    if delta <= f32::EPSILON || max <= f32::EPSILON {
        let gray = (value * 255.0) as u8;
        return [gray; 3];
    }

    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;
    let sat = ((delta / max) * saturation_gain).clamp(0.0, 1.0);
    let chroma = value * sat;
    let secondary = chroma * (1.0 - ((hue * 6.0).rem_euclid(2.0) - 1.0).abs());
    let offset = value - chroma;
    let sector = hue * 6.0;
    let (rr, gg, bb) = if sector < 1.0 {
        (chroma, secondary, 0.0)
    } else if sector < 2.0 {
        (secondary, chroma, 0.0)
    } else if sector < 3.0 {
        (0.0, chroma, secondary)
    } else if sector < 4.0 {
        (0.0, secondary, chroma)
    } else if sector < 5.0 {
        (secondary, 0.0, chroma)
    } else {
        (chroma, 0.0, secondary)
    };
    [rr, gg, bb].map(|component| ((component + offset) * 255.0).clamp(0.0, 255.0) as u8)
}
