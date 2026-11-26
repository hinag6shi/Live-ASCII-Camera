use crate::args::{Args, ColorStyle, Settings};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

pub fn path_for(args: &Args) -> PathBuf {
    args.config.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".config/ascii-virtual-camera/config")
    })
}

fn has_cli_option(name: &str) -> bool {
    std::env::args()
        .skip(1)
        .any(|arg| arg == name || arg.starts_with(&format!("{name}=")))
}

fn number<T>(values: &HashMap<String, String>, key: &str) -> Result<Option<T>>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    values
        .get(key)
        .map(|value| {
            value
                .parse()
                .map_err(|error| anyhow::anyhow!("invalid '{key}' in config: {error}"))
        })
        .transpose()
}

pub fn load(args: &mut Args) -> Result<()> {
    let path = path_for(args);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading config {}", path.display()))
        }
    };
    let mut values = HashMap::new();
    for (index, line) in contents.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .with_context(|| format!("invalid config line {}", index + 1))?;
        if values
            .insert(key.trim().to_owned(), value.to_owned())
            .is_some()
        {
            bail!(
                "duplicate key '{}' in config line {}",
                key.trim(),
                index + 1
            );
        }
    }

    if !has_cli_option("--input") && !has_cli_option("-i") {
        if let Some(value) = values.get("input") {
            args.input.clone_from(value);
        }
    }
    if !has_cli_option("--name") {
        if let Some(value) = values.get("name") {
            args.name.clone_from(value);
        }
    }
    if !has_cli_option("--width") {
        if let Some(value) = number(&values, "width")? {
            args.width = value;
        }
    }
    if !has_cli_option("--height") {
        if let Some(value) = number(&values, "height")? {
            args.height = value;
        }
    }
    if !has_cli_option("--fps") && !has_cli_option("-f") {
        if let Some(value) = number(&values, "fps")? {
            args.fps = value;
        }
    }
    if !has_cli_option("--cell") {
        if let Some(value) = number(&values, "cell")? {
            args.cell = value;
        }
    }
    if !has_cli_option("--brightness") {
        if let Some(value) = number(&values, "brightness")? {
            args.brightness = value;
        }
    }
    if !has_cli_option("--saturation") {
        if let Some(value) = number(&values, "saturation")? {
            args.saturation = value;
        }
    }
    if !has_cli_option("--mirror") && !args.no_mirror {
        if let Some(value) = values.get("mirror") {
            args.mirror = value
                .parse()
                .with_context(|| "invalid 'mirror' in config; use true or false")?;
        }
    }
    if !has_cli_option("--invert") && !args.no_invert {
        if let Some(value) = values.get("invert") {
            args.invert = value
                .parse()
                .with_context(|| "invalid 'invert' in config; use true or false")?;
        }
    }
    if !has_cli_option("--color") {
        if let Some(value) = values.get("color") {
            args.color = match value.as_str() {
                "camera" => ColorStyle::Camera,
                "green" => ColorStyle::Green,
                "amber" => ColorStyle::Amber,
                "blue" => ColorStyle::Blue,
                "gray" => ColorStyle::Gray,
                _ => bail!("invalid 'color' in config: {value}"),
            };
        }
    }
    if !has_cli_option("--ramp") && !has_cli_option("--ramp-preset") {
        if let Some(value) = values.get("ramp") {
            args.ramp = Some(value.clone());
        }
    }
    if args.no_mirror {
        args.mirror = false;
    }
    if args.no_invert {
        args.invert = false;
    }
    Ok(())
}

pub fn save(path: &Path, args: &Args, settings: &Settings, input: &str) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).context("creating config directory")?;
    }
    let color = match settings.color {
        ColorStyle::Camera => "camera",
        ColorStyle::Green => "green",
        ColorStyle::Amber => "amber",
        ColorStyle::Blue => "blue",
        ColorStyle::Gray => "gray",
    };
    let ramp = settings.ramp.iter().collect::<String>();
    let contents = format!(
        "input={input}\nname={}\nwidth={}\nheight={}\nfps={}\ncell={}\nbrightness={}\nsaturation={}\nmirror={}\ninvert={}\ncolor={color}\nramp={ramp}\n",
        args.name, args.width, args.height, args.fps, settings.cell,
        settings.brightness, settings.saturation, settings.mirror, settings.invert
    );
    fs::write(path, contents).with_context(|| format!("writing config {}", path.display()))
}
