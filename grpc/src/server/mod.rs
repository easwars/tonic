use std::sync::Arc;

use tokio::sync::oneshot;
use tonic::async_trait;

use crate::service::{Message, MessageService, Request, Response};

pub struct Server {
    handler: Option<Arc<dyn MessageService>>,
}

pub type Call = (
    Request<Box<dyn Message>>,
    oneshot::Sender<Response<Box<dyn Message>>>,
);

#[async_trait]
pub trait Listener {
    async fn accept(&self) -> Option<Call>;
}

impl Server {
    pub fn new() -> Self {
        Self { handler: None }
    }

    pub fn set_handler(&mut self, f: impl MessageService + 'static) {
        self.handler = Some(Arc::new(f))
    }

    pub async fn serve(&self, l: &impl Listener) {
        while let Some((req, reply_on)) = l.accept().await {
            reply_on
                .send(self.handler.as_ref().unwrap().call(req).await)
                .ok(); // TODO: log error
        }
    }
}
