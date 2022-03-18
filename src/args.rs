use std::{path::PathBuf, time::Duration};
use tracing::{debug, error};

#[derive(clap::Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Show the UI in fullscreen mode
    #[clap(short = 'F', long)]
    pub fullscreen: bool,

    #[clap(flatten)]
    pub video: VideoParams,

    #[clap(flatten)]
    pub trigger: TriggerParams,

    /// Output folder for mugshots
    #[clap(short, long, default_value = "captures")]
    pub output: PathBuf,

    /// Filename template for mugshots
    #[clap(short, long, default_value = "%Y-%m-%d_%H-%M-%S.jpg")]
    pub filename: String,

    /// 1 or more images to use as countdown overlays
    #[clap(short, long)]
    pub countdown: Option<Vec<PathBuf>>,

    /// Image to overlay the mugshot with while frozen
    #[clap(short, long)]
    pub mugshot: Option<PathBuf>,

    /// Duration showing the frozen mugshot before restarting the trigger timer
    #[clap(long, parse(try_from_str = parse_duration), default_value="3s")]
    pub freeze: Duration,
}

#[derive(clap::Args, Debug, Clone, Copy)]
pub struct TriggerParams {
    /// Duration until start of countdown
    #[clap(short, long, parse(try_from_str = parse_duration))]
    pub timeout: Option<Duration>,

    /// Duration between each countdown
    #[clap(long, parse(try_from_str = parse_duration), default_value="1s")]
    pub timeout_between: Duration,
}

#[derive(clap::Args, Debug, Clone, Copy)]
pub struct VideoParams {
    /// Video capture device
    #[clap(short, long, default_value_t = 0)]
    pub device: i32,

    /// Frame image width
    #[clap(long, default_value_t = 320)]
    pub video_width: u32,

    /// Frame image width
    #[clap(long, default_value_t = 1920)]
    pub snapshot_width: u32,

    /// Frame image width
    #[clap(long = "fps", default_value_t = 30)]
    pub frame_rate: u32,
}

fn parse_duration(s: &str) -> Result<Duration, &'static str> {
    match parse_duration::parse(s) {
        Ok(d) => {
            if d > Duration::ZERO {
                debug!(duration=?d);
                Ok(d)
            } else {
                error!(duration=?d, "fail parsing duration");
                Err("Must be > 0")
            }
        }
        Err(err) => {
            error!(?err, "Fail parsing duration");
            Err("Failed parsing duration")
        }
    }
}
