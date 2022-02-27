use anyhow::Result;
use opencv::{
    core::{CV_32F, CV_8U},
    highgui,
    prelude::*,
};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, instrument, trace, warn};

use crate::alpha_image::AlphaImage;

#[derive(Debug, Clone)]
pub enum EventMsg {
    KeyPressed(i32),
    WindowClosed,
}

#[derive(Debug)]
pub enum ControlMsg {
    Blend(Option<AlphaImage>),
    Freeze,
    Live,
}

enum VideoState {
    Live,
    Frozen,
}

#[instrument(skip_all)]
pub fn ui_event_loop(
    event_sender: broadcast::Sender<EventMsg>,
    mut control_receiver: mpsc::Receiver<ControlMsg>,
    mut frame_receiver: broadcast::Receiver<Mat>,
    mut exit_receiver: broadcast::Receiver<bool>,
) -> Result<()> {
    info!("ui thread started");

    let mut video_state = VideoState::Live;
    let mut blending_image = None;

    debug!("opening window");
    let window = "video capture";
    highgui::named_window(window, highgui::WINDOW_NORMAL | highgui::WINDOW_GUI_NORMAL)?;
    //highgui::set_window_property(window, highgui::WND_PROP_FULLSCREEN, 1.)?;

    debug!("entering ui event loop");
    let mut frame_i = Mat::default();
    let mut frame_f = Mat::default();
    let mut tmp_1_f = Mat::default();
    let mut tmp_2_f = Mat::default();
    loop {
        let key = highgui::wait_key(20)?;

        if exit_receiver.try_recv().is_ok() {
            info!("exit received");
            break;
        }

        if key > 0 {
            debug!(?key, "key event");
            event_sender.send(EventMsg::KeyPressed(key))?;
        }

        if highgui::get_window_property(window, highgui::WND_PROP_VISIBLE)? < 1.0 {
            debug!("window closed");
            event_sender.send(EventMsg::WindowClosed)?;
            break;
        }

        if let Ok(msg) = control_receiver.try_recv() {
            debug!(?msg, "received display msg");
            match msg {
                ControlMsg::Blend(img) => blending_image = img,
                ControlMsg::Freeze => video_state = VideoState::Frozen,
                ControlMsg::Live => video_state = VideoState::Live,
            }
        }

        match video_state {
            VideoState::Frozen => {}
            VideoState::Live => {
                if let Ok(frame) = frame_receiver.try_recv() {
                    trace!(?frame, "received image frame");
                    frame.assign_to(&mut tmp_1_f, CV_32F)?;

                    trace!("flip image");
                    opencv::core::flip(&tmp_1_f, &mut frame_f, 1)?;
                }
            }
        }

        if let Some(ref blending_image) = blending_image {
            trace!("blend image");
            opencv::core::multiply(&frame_f, blending_image.beta(), &mut tmp_1_f, 1., -1)?;
            opencv::core::add(
                &tmp_1_f,
                blending_image.rgb(),
                &mut tmp_2_f,
                &Mat::default(),
                -1,
            )?;

            tmp_2_f.assign_to(&mut frame_i, CV_8U)?;
        } else {
            frame_f.assign_to(&mut frame_i, CV_8U)?;
        }
        if !frame_f.empty() {
            trace!("display image");
            highgui::imshow(window, &frame_i)?;
        }
    }

    warn!("exiting");
    Ok(())
}
