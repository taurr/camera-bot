use actix_web::{dev::Server, get, web::Data, App, HttpResponse, HttpServer, Responder};
use anyhow::Result;
use tokio::sync::broadcast;
use tracing::warn;

type TriggerType = crate::auto_trigger::EventMsg;

pub fn spawn(
    mut exit_receiver: broadcast::Receiver<bool>,
    trigger_event_sender: broadcast::Sender<TriggerType>,
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::spawn(async move {
        let server = web_server(trigger_event_sender);
        tokio::select! {
            err = server => {
                warn!(?err, "Rest service exited");
            }
            _ = exit_receiver.recv() => {
                warn!("exiting");
            }
        };
        Ok(())
    })
}

fn web_server(trigger_event_sender: broadcast::Sender<TriggerType>) -> Server {
    HttpServer::new(move || {
        let data: Data<broadcast::Sender<TriggerType>> = Data::new(trigger_event_sender.clone());
        App::new().app_data(data).service(trigger)
    })
    .bind(("0.0.0.0", 8080))
    .unwrap()
    .run()
}

#[get("/trigger")]
#[allow(clippy::unused_async)]
async fn trigger(sender: Data<broadcast::Sender<TriggerType>>) -> impl Responder {
    sender.send(TriggerType::Trigger).unwrap();
    HttpResponse::Ok().body("Camera triggered")
}
