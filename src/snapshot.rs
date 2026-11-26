use crate::signals;
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn requested_path(argument: Option<&str>) -> Result<PathBuf> {
    let mut path = if let Some(argument) = argument {
        let path = PathBuf::from(argument);
        if path.as_os_str().is_empty() {
            bail!("PNG path cannot be empty");
        }
        path
    } else {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?
            .as_millis();
        PathBuf::from(format!("ascii-camera-{timestamp}.png"))
    };

    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => {}
        Some(extension) => bail!("snapshot filename must use .png, got .{extension}"),
        None => {
            path.set_extension("png");
        }
    }
    if path.symlink_metadata().is_ok() {
        bail!("{} already exists; choose another filename", path.display());
    }
    Ok(path)
}

pub fn save_png(frame: &[u8], width: usize, height: usize, path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating screenshot directory {}", parent.display()))?;
    }
    let size = format!("{width}x{height}");
    let mut command = Command::new("ffmpeg");
    signals::ignore_in_child(&mut command);
    let mut encoder = command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-n",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
            &size,
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-c:v",
            "png",
            "-f",
            "image2",
        ])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .context("could not start ffmpeg to write PNG")?;

    let write_result = encoder
        .stdin
        .take()
        .context("ffmpeg stdin was not available")?
        .write_all(frame);
    if let Err(error) = write_result {
        let _ = encoder.kill();
        let _ = encoder.wait();
        bail!("writing PNG frame to ffmpeg: {error}");
    }
    let status = encoder.wait().context("waiting for PNG encoder")?;
    if !status.success() {
        bail!(
            "ffmpeg failed to write {} (exit status {status})",
            path.display()
        );
    }
    Ok(())
}
