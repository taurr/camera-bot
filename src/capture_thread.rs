use std::thread;

use anyhow::Result;
use opencv::{prelude::*, videoio};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::{debug, info, instrument, trace, warn};

use crate::args::VideoParams;

pub enum Command {
    Snapshot(oneshot::Sender<Mat>),
}

pub async fn spawn(
    video_params: VideoParams,
    command_receiver: mpsc::Receiver<Command>,
    capture_event_sender: broadcast::Sender<Mat>,
    exit_receiver: broadcast::Receiver<bool>,
) -> Result<thread::JoinHandle<()>> {
    debug!("spawning capture thread");
    let (mut s, mut r) = mpsc::channel(1);
    let joinhandle = thread::spawn(move || {
        if frame_grabber(
            &mut s,
            video_params,
            command_receiver,
            capture_event_sender,
            exit_receiver,
        )
        .is_err()
        {
            s.blocking_send(false).ok();
        }
    });

    match r.recv().await.unwrap() {
        true => Ok(joinhandle),
        false => anyhow::bail!("failed to start capture-thread"),
    }
}

#[instrument(skip_all)]
fn frame_grabber(
    start_sender: &mut mpsc::Sender<bool>,
    video_params: VideoParams,
    mut command_receiver: mpsc::Receiver<Command>,
    frame_event_sender: broadcast::Sender<Mat>,
    mut exit_receiver: broadcast::Receiver<bool>,
) -> Result<()> {
    info!("capture thread started");

    debug!("opening camera");
    let mut camera = videoio::VideoCapture::new(video_params.device, videoio::CAP_GSTREAMER)?;

    //camera.set(videoio::CAP_PROP_FOURCC, f64::from(videoio::VideoWriter::fourcc(b'M' as i8, b'J' as i8, b'P' as i8, b'G' as i8).unwrap()))?;
    camera.set(
        videoio::CAP_PROP_FRAME_WIDTH,
        f64::from(video_params.video_width),
    )?;
    camera.set(
        videoio::CAP_PROP_XI_FRAMERATE,
        f64::from(video_params.frame_rate),
    )?;

    if !videoio::VideoCapture::is_opened(&camera)? {
        anyhow::bail!("Unable to open default camera!");
    }

    start_sender.blocking_send(true).ok();
    debug!("entering camera capture loop");

    let mut frame = Mat::default();
    loop {
        camera.read(&mut frame)?;
        if !frame.empty() {
            trace!(?frame, "image captured");
            if frame_event_sender.send(frame.clone()).is_err() {
                info!("all receivers has left");
                break;
            }
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

            camera.set(
                videoio::CAP_PROP_FRAME_WIDTH,
                f64::from(video_params.video_width),
            )?;
        }
        if exit_receiver.try_recv().is_ok() {
            info!("exit received");
            break;
        }
    }

    warn!("exiting");
    Ok(())
}
