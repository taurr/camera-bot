use std::thread;

use anyhow::Result;
use opencv::{prelude::*, videoio};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, info, instrument, trace, warn};

use crate::args::VideoParams;

pub enum Command {
    Snapshot(oneshot::Sender<Mat>),
}

pub fn spawn(
    video_params: VideoParams,
    command_receiver: mpsc::Receiver<Command>,
    capture_event_sender: broadcast::Sender<Mat>,
    exit_receiver: broadcast::Receiver<bool>,
) -> thread::JoinHandle<()> {
    debug!("spawning capture thread");
    thread::spawn(move || {
        frame_grabber(
            video_params,
            command_receiver,
            capture_event_sender,
            exit_receiver,
        )
        .expect("capture thread failed");
    })
}

#[instrument(skip_all)]
fn frame_grabber(
    video_params: VideoParams,
    mut command_receiver: mpsc::Receiver<Command>,
    frame_event_sender: broadcast::Sender<Mat>,
    mut exit_receiver: broadcast::Receiver<bool>,
) -> Result<()> {
    info!("capture thread started");

    debug!("opening camera");
    let mut camera = videoio::VideoCapture::new(video_params.device, videoio::CAP_ANY)?;
    camera.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(video_params.video_width))?;
    camera.set(
        videoio::CAP_PROP_XI_FRAMERATE,
        f64::from(video_params.frame_rate),
    )?;
    if !videoio::VideoCapture::is_opened(&camera)? {
        anyhow::bail!("Unable to open default camera!");
    }

    debug!("entering camera capture loop");
    let mut frame = Mat::default();
    loop {
        camera.read(&mut frame)?;
        if !frame.empty() {
            trace!(?frame, "image captured");
            let _ = frame_event_sender.send(frame.clone());
        }
        if let Ok(command) = command_receiver.try_recv() {
            let Command::Snapshot(sender) = command;

            camera.set(
                videoio::CAP_PROP_FRAME_WIDTH,
                f64::from(video_params.snapshot_width),
            )?;

            let mut snapshot = Mat::default();
            camera.read(&mut snapshot)?;
            sender.send(snapshot).ok();

            camera.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(video_params.video_width))?;
        }
        if exit_receiver.try_recv().is_ok() {
            info!("exit received");
            break;
        }
    }

    warn!("exiting");
    Ok(())
}
