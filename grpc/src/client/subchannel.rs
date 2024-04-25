use std::sync::Arc;

use tonic::async_trait;

use crate::service::{Request, Response, Service};

use super::load_balancing;

pub(crate) struct Subchannel {
    t: Arc<Box<dyn Service>>,
}

impl Subchannel {
    /// Creates a new subchannel in idle state.
    pub fn new(t: Box<dyn Service>) -> Self {
        Subchannel { t: Arc::new(t) }
    }
    /// Drain waits for any in-flight RPCs to terminate and then closes the
    /// connection and consumes the Subchannel.
    pub async fn drain(self) {}
    /// Begins connecting the subchannel asynchronously.  If now is set, does
    /// not wait for any pending connection backoff to complete.
    pub fn connect(&mut self, now: bool) {
        /*let mut s = self.svc.lock().unwrap();
        let svc = self.t.connect(self.addr).unwrap();
        *s = Some(svc);*/
        todo!()
    }
}

impl load_balancing::Subchannel for Subchannel {
    fn connect(&self) {
        todo!()
    }

    fn listen(
        &self,
        updates: Box<dyn Fn(super::ConnectivityState)>, // TODO: stream/asynciter/channel probably
    ) {
        todo!()
    }

    fn shutdown(&self) {
        todo!()
    }
}

#[async_trait]
impl Service for Subchannel {
    async fn call(&self, request: Request) -> Response {
        todo!()
    }
}
