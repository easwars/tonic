use tokio::sync::oneshot;
use tonic::async_trait;

use crate::service::{Request, Response, Service};

pub struct Server {
    handler: Option<Box<dyn Service>>,
}

pub type Call = (Request, oneshot::Sender<Response>);

#[async_trait]
pub trait Listener {
    async fn accept(&self) -> Option<Call>;
}

impl Server {
    pub fn new() -> Self {
        Self { handler: None }
    }

    pub fn set_handler(&mut self, f: Box<dyn Service>) {
        self.handler = Some(f)
    }

    pub async fn serve(&self, l: impl Listener) {
        while let Some((req, reply_on)) = l.accept().await {
            dbg!("got req:", &req);
            reply_on
                .send(self.handler.as_ref().unwrap().call(req).await)
                .ok(); // TODO: log error
        }
    }
}
