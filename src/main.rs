use anyhow::Result;
use clap::StructOpt;
use opencv::imgcodecs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::sleep;
use tracing::{debug, info};

use crate::alpha_image::AlphaImage;
use crate::snapshot_repo::SnapshotRepo;

mod alpha_image;
mod args;
mod auto_trigger;
mod capture_thread;
mod log;
mod snapshot_repo;
mod ui_thread;
mod web;

const KEY_ESCAPE: i32 = 27;
const KEY_ENTER: i32 = 13;

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::Args::parse();
    log::setup_tracing();
    info!("starting");

    let (countdown_blend_images, snapshot_blend_image) = read_overlay_images(
        &args.countdown.clone().unwrap_or_else(|| {
            ["assets/1.png", "assets/2.png", "assets/3.png"]
                .into_iter()
                .map(PathBuf::from)
                .collect()
        }),
        &args
            .mugshot
            .clone()
            .unwrap_or_else(|| PathBuf::from("assets/mugshot.png")),
    )?;

    let (exit_sender, exit_receiver) = broadcast::channel(1);
    let (capture_event_sender, capture_event_receiver) = broadcast::channel(1);
    let (capture_thread, capture_control_sender) = {
        let (sender, receiver) = mpsc::channel(1);
        (
            capture_thread::spawn(
                args.video,
                receiver,
                capture_event_sender,
                exit_sender.subscribe(),
            ).await?,
            sender,
        )
    };

    let (ui_event_sender, ui_event_receiver) = broadcast::channel(1);
    let (ui_thread, ui_control_sender) = ui_thread::spawn(
        if args.fullscreen {
            ui_thread::WindowMode::Fullscreen
        } else {
            ui_thread::WindowMode::Windowed
        },
        ui_event_sender,
        capture_event_receiver,
        exit_receiver,
    );

    let (trigger_event_sender, trigger_event_receiver) = broadcast::channel(1);
    let (trigger_thread, trigger_control_sender) = auto_trigger::spawn(
        args.trigger,
        trigger_event_sender.clone(),
        exit_sender.subscribe(),
        countdown_blend_images.len(),
    );

    let rest_service_thread = web::spawn(exit_sender.subscribe(), trigger_event_sender);

    let repo = SnapshotRepo::from_path_and_namepattern(args.output.clone(), &args.filename);

    coordinate_events(
        args,
        capture_control_sender,
        ui_event_receiver,
        &ui_control_sender,
        trigger_event_receiver,
        &trigger_control_sender,
        repo,
        &countdown_blend_images,
        snapshot_blend_image,
    )
    .await;

    info!("sending exit message");
    exit_sender.send(true)?;
    rest_service_thread.await??;
    trigger_thread.await??;
    capture_thread.join().expect("thread join failed");
    ui_thread.join().expect("thread join failed");

    info!("exited");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn coordinate_events(
    args: args::Args,
    capture_control_sender: mpsc::Sender<capture_thread::Command>,
    mut ui_event_receiver: broadcast::Receiver<ui_thread::EventMsg>,
    ui_control_sender: &mpsc::Sender<ui_thread::ControlMsg>,
    mut trigger_event_receiver: broadcast::Receiver<auto_trigger::EventMsg>,
    trigger_control_sender: &mpsc::Sender<auto_trigger::ControlMsg>,
    mut repo: SnapshotRepo,
    countdown_blend_images: &[AlphaImage],
    snapshot_blend_image: Option<AlphaImage>,
) {
    loop {
        tokio::select! {
            msg = ui_event_receiver.recv() => {
                debug!(?msg, "msg from ui thread");
                if let Ok(msg) = msg {
                    match msg {
                        ui_thread::EventMsg::KeyPressed(key) => match key {
                            KEY_ENTER => save_snapshot(
                                    args.freeze,
                                &capture_control_sender,
                                    trigger_control_sender,
                                    ui_control_sender,
                                    snapshot_blend_image.clone(),
                                    &mut repo,
                                ).await,
                            KEY_ESCAPE => return,
                            _ => {}
                        },
                        ui_thread::EventMsg::WindowClosed => return,
                    }
                }
            }
            msg = trigger_event_receiver.recv() => {
                debug!(?msg, "msg from trigger");
                if let Ok(msg) = msg {
                    match msg {
                        auto_trigger::EventMsg::Trigger => {
                            save_snapshot(
                                args.freeze,
                                &capture_control_sender,
                                trigger_control_sender,
                                ui_control_sender,
                                snapshot_blend_image.clone(),
                                &mut repo,
                            ).await;
                        },
                        auto_trigger::EventMsg::Countdown(n) => {
                            ui_control_sender.send(ui_thread::ControlMsg::Blend(countdown_blend_images.get(n-1).cloned())).await.ok();
                        },
                    }
                }
            }
        };
    }
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
    capture_control_sender: &mpsc::Sender<capture_thread::Command>,
    trigger_control_sender: &mpsc::Sender<auto_trigger::ControlMsg>,
    display_control_sender: &mpsc::Sender<ui_thread::ControlMsg>,
    snapshot_blend_image: Option<AlphaImage>,
    repo: &mut SnapshotRepo,
) {
    info!("Taking snapshot");
    let _ = trigger_control_sender
        .send(auto_trigger::ControlMsg::Stop)
        .await;

    let (s, r) = oneshot::channel();
    capture_control_sender
        .send(capture_thread::Command::Snapshot(s))
        .await
        .ok();
    let snapshot = r.await.unwrap();
    display_control_sender
        .send(ui_thread::ControlMsg::Blend(snapshot_blend_image))
        .await
        .ok();
    display_control_sender
        .send(ui_thread::ControlMsg::Freeze)
        .await
        .ok();
    repo.save_frame(&snapshot).expect("failed saving snapshot");

    sleep(freeze_duration).await;

    info!("restarting video");
    display_control_sender
        .send(ui_thread::ControlMsg::Blend(None))
        .await
        .ok();
    display_control_sender
        .send(ui_thread::ControlMsg::Live)
        .await
        .ok();
    let _ = trigger_control_sender
        .send(auto_trigger::ControlMsg::Run)
        .await;
    debug!("snapshot taken");
}
