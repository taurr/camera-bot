use std::{path::PathBuf, time::Duration};
use tracing::{debug, error};

#[derive(clap::Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(short = 'F', long)]
    pub fullscreen: bool,

    #[clap(flatten)]
    pub video: VideoParams,

    #[clap(flatten)]
    pub trigger: TriggerParams,

    /// Duration showing the frozen snapshot before restarting the trigger timer
    #[clap(long, parse(try_from_str = parse_duration), default_value="3s")]
    pub freeze: Duration,

    /// Output folder for snapshots
    #[clap(short, long, default_value = "captures")]
    pub output: PathBuf,

    /// Filename template for snapshots
    #[clap(short, long, default_value = "%Y-%m-%d_%H-%M-%S.jpg")]
    pub filename: String,

    #[clap(short, long)]
    pub countdown: Option<Vec<PathBuf>>,

    #[clap(short, long)]
    pub mugshot: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
pub struct TriggerParams {
    /// Duration until start of countdown
    #[clap(short, long, parse(try_from_str = parse_duration), default_value="9s")]
    pub timeout: Duration,

    /// Duration between each countdown
    #[clap(long, parse(try_from_str = parse_duration), default_value="1s")]
    pub timeout_between: Duration,
}

#[derive(clap::Args, Debug)]
pub struct VideoParams {
    /// Video capture device
    #[clap(short, long, default_value_t = 0)]
    pub device: i32,

    /// Frame image width
    #[clap(short, long, default_value_t = 640)]
    pub width: u32,

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
