use std::thread;

use anyhow::Result;
use opencv::{prelude::*, videoio};
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, trace, warn};

use crate::args::VideoParams;

pub fn spawn_capture_thread(
    video_params: VideoParams,
    capture_event_sender: broadcast::Sender<Mat>,
    exit_receiver: broadcast::Receiver<bool>,
) -> thread::JoinHandle<()> {
    debug!("spawning capture thread");
    thread::spawn(move || {
        frame_grabber(video_params, capture_event_sender, exit_receiver)
            .expect("capture thread failed");
    })
}

#[instrument(skip_all)]
fn frame_grabber(
    video_params: VideoParams,
    frame_event_sender: broadcast::Sender<Mat>,
    mut exit_receiver: broadcast::Receiver<bool>,
) -> Result<()> {
    info!("capture thread started");

    debug!("opening camera");
    let mut camera = videoio::VideoCapture::new(video_params.device, videoio::CAP_ANY)?;
    camera.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(video_params.width))?;
    camera.set(videoio::CAP_PROP_XI_FRAMERATE, f64::from(video_params.frame_rate))?;
    if !videoio::VideoCapture::is_opened(&camera)? {
        anyhow::bail!("Unable to open default camera!");
    }

    debug!("entering camera capture loop");
    let mut frame = Mat::default();
    loop {
        camera.read(&mut frame)?;
        if !frame.empty() {
            trace!(?frame, "image captured");
            frame_event_sender
                .send(frame.clone())
                .map_err(|_| anyhow::anyhow!("failed sending captured frame"))?;
        }
        if exit_receiver.try_recv().is_ok() {
            info!("exit received");
            break;
        }
    }

    warn!("exiting");
    Ok(())
}
