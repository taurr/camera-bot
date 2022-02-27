use anyhow::Result;
use opencv::{imgcodecs, prelude::*};
use std::thread;
use std::time::Duration;
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tracing::{debug, info, trace};

use crate::alpha_image::AlphaImage;
use crate::snapshot_repo::SnapshotRepo;

mod alpha_image;
mod auto_trigger;
mod capture_thread;
mod log;
mod snapshot_repo;
mod ui_thread;

const CAM_WIDTH: f64 = 640.;
const CAM_FRAMERATE: f64 = 20.;

const KEY_ESCAPE: i32 = 27;
const KEY_ENTER: i32 = 13;

const DURATION_UNTIL_COUNTDOWN: Duration = Duration::from_secs(7);
const DURATION_BETWEEN_COUNTDOWN: Duration = Duration::from_secs(1);
const DURATION_SNAPSHOT_FREEZE: Duration = Duration::from_secs(3);

#[tokio::main]
async fn main() -> Result<()> {
    log::setup_tracing();
    info!("starting");

    let (exit_sender, exit_receiver) = broadcast::channel(1);
    let (capture_event_sender, mut capture_event_receiver) = broadcast::channel(1);
    let (trigger_event_sender, mut trigger_event_receiver) = broadcast::channel(1);
    let (ui_event_sender, mut ui_event_receiver) = broadcast::channel(1);

    let (countdown_blend_images, snapshot_blend_image) = read_overlay_images()?;

    let (ui_thread, display_control_sender) = spawn_ui_thread(
        ui_event_sender,
        capture_event_sender.subscribe(),
        exit_receiver,
    );

    let capture_thread = spawn_capture_thread(capture_event_sender, exit_sender.subscribe());

    let (trigger_thread, trigger_control_sender) = spawn_trigger(
        trigger_event_sender,
        exit_sender.subscribe(),
        countdown_blend_images.len(),
    );

    let mut repo = SnapshotRepo::from_path_and_namepattern("captures", "%Y-%m-%d_%H-%M-%S.jpg");
    let mut frame = Mat::default();
    loop {
        select! {
            msg = ui_event_receiver.recv() => {
                debug!(?msg, "msg from ui thread");
                if let Ok(msg) = msg {
                    match msg {
                        ui_thread::EventMsg::KeyPressed(key) => match key {
                            KEY_ENTER => save_snapshot(
                                    &trigger_control_sender,
                                    &display_control_sender,
                                    snapshot_blend_image.clone(),
                                    &mut repo,
                                    frame.clone(),
                                ).await,
                            KEY_ESCAPE => break,
                            _ => {}
                        },
                        ui_thread::EventMsg::WindowClosed => break,
                    }
                }
            }
            msg = capture_event_receiver.recv() => {
                trace!(?msg, "frame from capture thread");
                if let Ok(f) = msg { frame = f }
            },
            msg = trigger_event_receiver.recv() => {
                debug!(?msg, "msg from trigger");
                if let Ok(msg) = msg {
                    match msg {
                        auto_trigger::EventMsg::Trigger => save_snapshot(
                                &trigger_control_sender,
                                &display_control_sender,
                                snapshot_blend_image.clone(),
                                &mut repo,
                                frame.clone(),
                            ).await,
                        auto_trigger::EventMsg::Countdown(n) => {
                            display_control_sender.send(ui_thread::ControlMsg::Blend(countdown_blend_images.get(n-1).cloned())).await.ok();
                        },
                    }
                }
            }
        };
    }

    info!("sending exit message");
    exit_sender.send(true)?;
    trigger_thread.await??;
    capture_thread.join().expect("thread join failed");
    ui_thread.join().expect("thread join failed");

    info!("exited");
    Ok(())
}

fn spawn_trigger(
    trigger_event_sender: broadcast::Sender<auto_trigger::EventMsg>,
    exit_receiver: broadcast::Receiver<bool>,
    countdown_from: usize,
) -> (
    tokio::task::JoinHandle<Result<(), anyhow::Error>>,
    mpsc::Sender<auto_trigger::ControlMsg>,
) {
    debug!("spawning trigger");
    let (trigger_control_sender, control_receiver) = mpsc::channel(1);
    let trigger_thread = tokio::spawn(auto_trigger::auto_trigger(
        trigger_event_sender,
        control_receiver,
        exit_receiver,
        countdown_from,
    ));
    (trigger_thread, trigger_control_sender)
}

fn spawn_capture_thread(
    capture_event_sender: broadcast::Sender<Mat>,
    exit_receiver: broadcast::Receiver<bool>,
) -> thread::JoinHandle<()> {
    debug!("spawning capture thread");
    thread::spawn(move || {
        capture_thread::frame_grabber(capture_event_sender, exit_receiver)
            .expect("capture thread failed");
    })
}

fn spawn_ui_thread(
    ui_event_sender: broadcast::Sender<ui_thread::EventMsg>,
    capture_event_receiver: broadcast::Receiver<Mat>,
    exit_receiver: broadcast::Receiver<bool>,
) -> (thread::JoinHandle<()>, mpsc::Sender<ui_thread::ControlMsg>) {
    debug!("spawning ui thread");
    let (ui_thread, display_control_sender) = {
        let (display_control_sender, control_receiver) = mpsc::channel(1);
        let ui_thread = thread::spawn(move || {
            ui_thread::ui_event_loop(
                ui_event_sender,
                control_receiver,
                capture_event_receiver,
                exit_receiver,
            )
            .expect("ui thread failed");
        });
        (ui_thread, display_control_sender)
    };
    (ui_thread, display_control_sender)
}

fn read_overlay_images() -> Result<(Vec<AlphaImage>, Option<AlphaImage>)> {
    debug!("reading overlay images");
    let countdown_blend_images = ["assets/1.png", "assets/2.png", "assets/3.png"]
        .into_iter()
        .map(|path| AlphaImage::new(imgcodecs::imread(path, imgcodecs::IMREAD_UNCHANGED)?))
        .collect::<Result<Vec<AlphaImage>>>()?;
    let snapshot_blend_image = AlphaImage::new(imgcodecs::imread(
        "assets/snapshot.png",
        imgcodecs::IMREAD_UNCHANGED,
    )?)
    .ok();
    Ok((countdown_blend_images, snapshot_blend_image))
}

async fn save_snapshot(
    trigger_control_sender: &mpsc::Sender<auto_trigger::ControlMsg>,
    display_control_sender: &mpsc::Sender<ui_thread::ControlMsg>,
    snapshot_blend_image: Option<AlphaImage>,
    repo: &mut SnapshotRepo,
    frame: Mat,
) {
    info!("Taking snapshot");
    trigger_control_sender
        .send(auto_trigger::ControlMsg::Stop)
        .await
        .unwrap();
    display_control_sender
        .send(ui_thread::ControlMsg::Freeze)
        .await
        .unwrap();
    display_control_sender
        .send(ui_thread::ControlMsg::Blend(snapshot_blend_image))
        .await
        .unwrap();
    repo.save_frame(&frame).expect("failed saving snapshot");
    sleep(DURATION_SNAPSHOT_FREEZE).await;
    display_control_sender
        .send(ui_thread::ControlMsg::Blend(None))
        .await
        .unwrap();
    display_control_sender
        .send(ui_thread::ControlMsg::Live)
        .await
        .unwrap();
    trigger_control_sender
        .send(auto_trigger::ControlMsg::Run)
        .await
        .unwrap();
    debug!("snapshot taken");
}
