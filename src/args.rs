use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Publish a live ASCII rendering as a Linux virtual camera"
)]
pub struct Args {
    /// Camera input device (for example /dev/video0)
    #[arg(short, long, default_value = "auto")]
    pub input: String,
    /// Name shown by applications in their camera list
    #[arg(long, default_value = "byhinag6shi")]
    pub name: String,
    /// V4L2 device path; selected automatically if omitted
    #[arg(short, long)]
    pub output: Option<String>,
    /// Output video width (must be even for YUYV422)
    #[arg(long, default_value_t = 1280)]
    pub width: usize,
    /// Output video height
    #[arg(long, default_value_t = 720)]
    pub height: usize,
    /// Camera capture frame rate
    #[arg(short, long, default_value_t = 30)]
    pub fps: u32,
    /// ASCII ramp, from dark to bright
    #[arg(long, value_enum, default_value_t = RampPreset::Classic)]
    pub ramp_preset: RampPreset,
    /// Custom printable ASCII ramp, from dark to bright; overrides --ramp-preset
    #[arg(long)]
    pub ramp: Option<String>,
    /// Color saturation multiplier (1.0 keeps camera colors, 3.5 is vivid)
    #[arg(long, default_value_t = 3.5)]
    pub saturation: f32,
    /// Color brightness multiplier
    #[arg(long, default_value_t = 1.12)]
    pub brightness: f32,
    /// Character cell size in pixels
    #[arg(long, default_value_t = 8)]
    pub cell: usize,
    /// Color treatment for rendered glyphs
    #[arg(long, value_enum, default_value_t = ColorStyle::Camera)]
    pub color: ColorStyle,
    /// Mirror the rendered image horizontally
    #[arg(long)]
    pub mirror: bool,
    /// Disable a mirrored setting loaded from the config
    #[arg(long, conflicts_with = "mirror")]
    pub no_mirror: bool,
    /// Invert brightness-to-character mapping
    #[arg(long)]
    pub invert: bool,
    /// Disable inversion loaded from the config
    #[arg(long, conflicts_with = "invert")]
    pub no_invert: bool,
    /// Config file path (defaults to ~/.config/ascii-virtual-camera/config)
    #[arg(long)]
    pub config: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ColorStyle {
    Camera,
    Green,
    Amber,
    Blue,
    Gray,
}

impl ColorStyle {
    pub fn next(self) -> Self {
        match self {
            Self::Camera => Self::Green,
            Self::Green => Self::Amber,
            Self::Amber => Self::Blue,
            Self::Blue => Self::Gray,
            Self::Gray => Self::Camera,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum RampPreset {
    Classic,
    Dense,
    Binary,
}

impl RampPreset {
    pub fn chars(self) -> &'static str {
        match self {
            Self::Classic => " .:-=+*#%@",
            Self::Dense => {
                " .'`^\",:;Il!i><~+_-?][}{1)(|\\/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"
            }
            Self::Binary => " 01",
        }
    }
}

pub struct Settings {
    pub cell: usize,
    pub max_cell: usize,
    pub saturation: f32,
    pub brightness: f32,
    pub mirror: bool,
    pub invert: bool,
    pub color: ColorStyle,
    pub ramp: Vec<char>,
}
