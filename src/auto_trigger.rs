use anyhow::Result;
use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tracing::{debug, info, instrument, warn};

use crate::args::TriggerParams;

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
    params: TriggerParams,
    event_sender: broadcast::Sender<EventMsg>,
    control_receiver: mpsc::Receiver<ControlMsg>,
    exit_receiver: broadcast::Receiver<bool>,
    countdown: usize,
) -> Result<()> {
    info!("auto_trigger started");

    let mut state = State::from(Waiting {
        data: CommonData {
            params,
            event_sender,
            control_receiver,
            exit_receiver,
            countdown,
        },
    });

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

#[async_trait]
#[enum_dispatch(State)]
trait StateBehavior {
    async fn next_state(self) -> Result<Option<State>>;
}

#[enum_dispatch]
#[derive(Debug)]
enum State {
    Waiting,
    Countdown,
    Trigger,
    Stopped,
}

#[derive(Debug)]
struct CommonData {
    params: TriggerParams,
    event_sender: broadcast::Sender<EventMsg>,
    control_receiver: mpsc::Receiver<ControlMsg>,
    exit_receiver: broadcast::Receiver<bool>,
    countdown: usize,
}

#[derive(Debug)]
struct Waiting {
    data: CommonData,
}

#[derive(Debug)]
struct Countdown {
    data: CommonData,
    count: usize,
}

#[derive(Debug)]
struct Trigger {
    data: CommonData,
}

#[derive(Debug)]
struct Stopped {
    data: CommonData,
}

#[async_trait]
impl StateBehavior for Waiting {
    #[instrument(skip(self))]
    async fn next_state(mut self) -> Result<Option<State>> {
        debug!("=> Waiting");
        let next_state = loop {
            select! {
                _ = self.data.exit_receiver.recv() => {
                    debug!("exit received");
                    break None
                },
                msg = self.data.control_receiver.recv() => {
                    debug!(?msg, "received control msg");
                    match msg {
                        Some(ControlMsg::Stop) => break Some(Stopped { data: self.data }.into()),
                        Some(ControlMsg::Run) | None => continue,
                    }
                },
                _ = sleep(self.data.params.timeout) => {
                    debug!("timeout");
                    break Some(Countdown {
                        count: self.data.countdown,
                        data: self.data,
                    }.into())
                },
            };
        };
        Ok(next_state)
    }
}

#[async_trait]
impl StateBehavior for Countdown {
    #[instrument(skip(self))]
    async fn next_state(mut self) -> Result<Option<State>> {
        debug!(index=?self.count, "=> Countdown");
        self.data
            .event_sender
            .send(EventMsg::Countdown(self.count))?;
        let next_state = loop {
            select! {
                _ = self.data.exit_receiver.recv() => {
                    debug!("exit received");
                    break None
                },
                msg = self.data.control_receiver.recv() => {
                    debug!(?msg, "received control msg");
                    match msg {
                        Some(ControlMsg::Stop) => break Some(Stopped{ data:self.data }.into()),
                        Some(ControlMsg::Run) | None => continue,
                    }
                },
                _ = sleep(self.data.params.timeout_between) => {
                    debug!("timeout");
                    self.count -= 1;
                    break Some(
                        if self.count > 0 {
                            self.into()
                        } else {
                            Trigger{data:self.data}.into()
                        }
                    )
                }
            }
        };
        Ok(next_state)
    }
}

#[async_trait]
impl StateBehavior for Trigger {
    #[instrument(skip(self))]
    async fn next_state(self) -> Result<Option<State>> {
        debug!("=> Triggering!!!");
        self.data.event_sender.send(EventMsg::Trigger)?;
        Ok(Some(Waiting { data: self.data }.into()))
    }
}

#[async_trait]
impl StateBehavior for Stopped {
    #[instrument(skip(self))]
    async fn next_state(mut self) -> Result<Option<State>> {
        debug!("=> Stopped");
        let next_state = loop {
            select! {
                _ = self.data.exit_receiver.recv() => {
                    debug!("exit received");
                    break None
                },
                msg = self.data.control_receiver.recv() => {
                    debug!(?msg, "received control msg");
                    match msg {
                        Some(ControlMsg::Run) => break Some(Waiting{ data:self.data }.into()),
                        Some(ControlMsg::Stop) | None => continue,
                    }
                },
            }
        };
        Ok(next_state)
    }
}
