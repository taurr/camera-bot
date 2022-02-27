use anyhow::Result;
use opencv::{prelude::*, videoio};
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, trace, warn};

#[instrument(skip_all)]
pub fn frame_grabber(
    frame_event_sender: broadcast::Sender<Mat>,
    mut exit_receiver: broadcast::Receiver<bool>,
) -> Result<()> {
    info!("capture thread started");

    debug!("opening camera");
    let mut camera = videoio::VideoCapture::new(0, videoio::CAP_ANY)?;
    camera.set(videoio::CAP_PROP_FRAME_WIDTH, crate::CAM_WIDTH)?;
    camera.set(videoio::CAP_PROP_XI_FRAMERATE, crate::CAM_FRAMERATE)?;
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
