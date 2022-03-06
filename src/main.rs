use anyhow::Result;
use clap::StructOpt;
use opencv::{imgcodecs, prelude::*};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tracing::{debug, info, trace};

use crate::alpha_image::AlphaImage;
use crate::auto_trigger::spawn_trigger;
use crate::capture_thread::spawn_capture_thread;
use crate::snapshot_repo::SnapshotRepo;
use crate::ui_thread::spawn_ui_thread;

mod alpha_image;
mod args;
mod auto_trigger;
mod capture_thread;
mod log;
mod snapshot_repo;
mod ui_thread;

const KEY_ESCAPE: i32 = 27;
const KEY_ENTER: i32 = 13;

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    log::setup_tracing();
    info!("starting");

    let (exit_sender, exit_receiver) = broadcast::channel(1);
    let (capture_event_sender, mut capture_event_receiver) = broadcast::channel(1);
    let (trigger_event_sender, mut trigger_event_receiver) = broadcast::channel(1);
    let (ui_event_sender, mut ui_event_receiver) = broadcast::channel(1);

    let (countdown_blend_images, snapshot_blend_image) = read_overlay_images(
        &args.countdown.unwrap_or_else(|| {
            ["assets/1.png", "assets/2.png", "assets/3.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect()
        }),
        &args
            .mugshot
            .unwrap_or_else(|| PathBuf::from("assets/mugshot.png")),
    )?;

    let (ui_thread, ui_control_sender) = spawn_ui_thread(
        if args.fullscreen {
            ui_thread::WindowMode::Fullscreen
        } else {
            ui_thread::WindowMode::Windowed
        },
        ui_event_sender,
        capture_event_sender.subscribe(),
        exit_receiver,
    );

    let capture_thread =
        spawn_capture_thread(args.video, capture_event_sender, exit_sender.subscribe());

    let (trigger_thread, trigger_control_sender) = spawn_trigger(
        args.trigger,
        trigger_event_sender,
        exit_sender.subscribe(),
        countdown_blend_images.len(),
    );

    let mut repo = SnapshotRepo::from_path_and_namepattern(args.output, args.filename);
    let mut frame = Mat::default();
    loop {
        select! {
            msg = ui_event_receiver.recv() => {
                debug!(?msg, "msg from ui thread");
                if let Ok(msg) = msg {
                    match msg {
                        ui_thread::EventMsg::KeyPressed(key) => match key {
                            KEY_ENTER => save_snapshot(
                                    args.freeze,
                                    &trigger_control_sender,
                                    &ui_control_sender,
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
                                args.freeze,
                                &trigger_control_sender,
                                &ui_control_sender,
                                snapshot_blend_image.clone(),
                                &mut repo,
                                frame.clone(),
                            ).await,
                        auto_trigger::EventMsg::Countdown(n) => {
                            ui_control_sender.send(ui_thread::ControlMsg::Blend(countdown_blend_images.get(n-1).cloned())).await.ok();
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

fn read_overlay_images(
    countdown_images: &[PathBuf],
    mugshot_image: &Path,
) -> Result<(Vec<AlphaImage>, Option<AlphaImage>)> {
    debug!("reading overlay images");
    let countdown_blend_images = countdown_images
        .iter()
        .map(|path| {
            AlphaImage::new(imgcodecs::imread(
                &path.display().to_string(),
                imgcodecs::IMREAD_UNCHANGED,
            )?)
        })
        .collect::<Result<Vec<AlphaImage>>>()?;
    let snapshot_blend_image = AlphaImage::new(imgcodecs::imread(
        &mugshot_image.display().to_string(),
        imgcodecs::IMREAD_UNCHANGED,
    )?)
    .ok();
    Ok((countdown_blend_images, snapshot_blend_image))
}

async fn save_snapshot(
    freeze_duration: Duration,
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
    sleep(freeze_duration).await;
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
