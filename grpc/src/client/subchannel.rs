use std::any::Any;
use std::sync::{Arc, Mutex};

use tonic::async_trait;

use super::load_balancing;
use crate::client::transport;
use crate::service::{Request, Response, Service};

pub(crate) struct Subchannel {
    t: Arc<Box<dyn transport::Transport>>,
    ct: Arc<Mutex<Option<Arc<Box<dyn transport::ConnectedTransport>>>>>,
    address: String,
}

impl Subchannel {
    /// Creates a new subchannel in idle state.
    pub fn new(t: Arc<Box<dyn transport::Transport>>, address: String) -> Self {
        Subchannel {
            t,
            ct: Arc::new(Mutex::new(None)),
            address,
        }
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
        let mut ct = self.ct.lock().unwrap();
        if ct.is_none() {
            *ct = Some(Arc::new(self.t.connect(self.address.clone()).unwrap()));
        }
    }

    fn listen(
        &self,
        updates: Box<dyn Fn(super::ConnectivityState) + Send + Sync>, // TODO: stream/asynciter/channel probably
    ) {
        updates(super::ConnectivityState::Ready);
        // TODO
    }

    fn shutdown(&self) {
        // Drop the connected transport, if there is one.
        self.ct.lock().unwrap().take();
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[async_trait]
impl Service for Subchannel {
    async fn call(&self, request: Request) -> Response {
        let ct = self.ct.lock().unwrap().as_ref().unwrap().clone();
        let svc: Box<dyn Service> = ct.get_service().unwrap();
        svc.call(request).await
    }
}
