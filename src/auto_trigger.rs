use anyhow::Result;
use enum_dispatch::enum_dispatch;
use std::future::Future;
use std::pin::Pin;
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tracing::{debug, info, instrument, warn};

use crate::{DURATION_BETWEEN_COUNTDOWN, DURATION_UNTIL_COUNTDOWN};

#[derive(Debug, Clone)]
pub enum EventMsg {
    Trigger,
    Countdown(usize),
}

#[derive(Debug)]
pub enum ControlMsg {
    Run,
    Stop,
}

#[instrument(skip_all)]
pub async fn auto_trigger(
    event_sender: broadcast::Sender<EventMsg>,
    control_receiver: mpsc::Receiver<ControlMsg>,
    exit_receiver: broadcast::Receiver<bool>,
    countdown: usize,
) -> Result<()> {
    info!("auto_trigger started");

    let mut state: State = Waiting(CommonData {
        event_sender,
        control_receiver,
        exit_receiver,
        countdown,
    })
    .into();

    let status = loop {
        match state.next_state().await {
            Ok(Some(s)) => state = s,
            Ok(None) => break Ok(()),
            Err(e) => break Err(e),
        }
    };

    warn!(?status, "exit auto_trigger");
    status
}

#[enum_dispatch(State)]
trait StateBehavior {
    fn next_state(self) -> TraitFuture<Result<Option<State>>>;
}

#[enum_dispatch]
#[derive(Debug)]
enum State {
    Waiting,
    Countdown,
    Trigger,
    Stopped,
}

type TraitFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug)]
struct CommonData {
    event_sender: broadcast::Sender<EventMsg>,
    control_receiver: mpsc::Receiver<ControlMsg>,
    exit_receiver: broadcast::Receiver<bool>,
    countdown: usize,
}

#[derive(Debug)]
struct Waiting(CommonData);

#[derive(Debug)]
struct Countdown {
    data: CommonData,
    count: usize,
}

#[derive(Debug)]
struct Trigger(CommonData);

#[derive(Debug)]
struct Stopped(CommonData);

impl StateBehavior for Waiting {
    #[instrument(skip(self))]
    fn next_state(mut self) -> TraitFuture<Result<Option<State>>> {
        Box::pin(async move {
            debug!("=> Waiting");
            let next_state = loop {
                select! {
                    _ = self.0.exit_receiver.recv() => break None,
                    msg = self.0.control_receiver.recv() => {
                        match msg {
                            Some(ControlMsg::Stop) => break Some(Stopped(self.0).into()),
                            Some(ControlMsg::Run) => continue,
                            None => continue,
                        }
                    },
                    _ = sleep(DURATION_UNTIL_COUNTDOWN) => break Some(Countdown {
                        count: self.0.countdown,
                        data: self.0,
                    }.into()),
                };
            };
            Ok(next_state)
        })
    }
}

impl StateBehavior for Countdown {
    #[instrument(skip(self))]
    fn next_state(mut self) -> TraitFuture<Result<Option<State>>> {
        Box::pin(async move {
            debug!(index=?self.count, "=> Countdown");
            self.data
                .event_sender
                .send(EventMsg::Countdown(self.count))?;
            let next_state = loop {
                select! {
                    _ = self.data.exit_receiver.recv() => break None,
                    msg = self.data.control_receiver.recv() => {
                        match msg {
                            Some(ControlMsg::Stop) => break Some(Stopped(self.data).into()),
                            Some(ControlMsg::Run) => continue,
                            None => continue,
                        }
                    },
                    _ = sleep(DURATION_BETWEEN_COUNTDOWN) => {
                        self.count -= 1;
                        if self.count > 0 {
                            break Some(self.into())
                        } else {
                            break Some(Trigger(self.data).into())
                        }
                    }
                }
            };
            Ok(next_state)
        })
    }
}

impl StateBehavior for Trigger {
    #[instrument(skip(self))]
    fn next_state(self) -> TraitFuture<Result<Option<State>>> {
        Box::pin(async move {
            debug!("=> Triggering!!!");
            self.0.event_sender.send(EventMsg::Trigger)?;
            Ok(Some(Waiting(self.0).into()))
        })
    }
}

impl StateBehavior for Stopped {
    #[instrument(skip(self))]
    fn next_state(mut self) -> TraitFuture<Result<Option<State>>> {
        Box::pin(async move {
            debug!("=> Stopped");
            let next_state = loop {
                select! {
                    _ = self.0.exit_receiver.recv() => break None,
                    msg = self.0.control_receiver.recv() => {
                        match msg {
                            Some(ControlMsg::Stop) => continue,
                            Some(ControlMsg::Run) => break Some(Waiting(self.0).into()),
                            None => continue,
                        }
                    },
                }
            };
            Ok(next_state)
        })
    }
}
