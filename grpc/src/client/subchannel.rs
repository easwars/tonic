use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tonic::async_trait;

use super::load_balancing;
use crate::client::transport;
use crate::service::{Request, Response, Service};

pub(crate) struct Subchannel {
    t: Arc<dyn transport::Transport>,
    state: Mutex<State>,
    address: String,
}

type SharedService = Arc<dyn Service>;

enum State {
    Idle,
    Connecting,
    Ready(SharedService),
    TransientFailure(Instant),
}

impl State {
    fn connected_transport(&self) -> Option<SharedService> {
        match self {
            Self::Ready(t) => Some(t.clone()),
            _ => None,
        }
    }
}

impl Subchannel {
    /// Creates a new subchannel in idle state.
    pub fn new(t: Arc<dyn transport::Transport>, address: String) -> Self {
        Subchannel {
            t,
            state: Mutex::new(State::Idle),
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

// TODO: this should be a wrapper type that allows sharing the real subchannel
// between LB policies.
impl load_balancing::Subchannel for Subchannel {
    fn connect(&self) {
        let mut state = self.state.lock().unwrap();
        match &*state {
            State::Idle => {
                *state = State::Ready(
                    self.t
                        .connect(self.address.clone())
                        .expect("todo: handle error")
                        .into(),
                )
            }
            State::TransientFailure(_) => {} // TODO: remember connect request and skip Idle when expires
            _ => {}
        }
    }

    fn listen(
        &self,
        updates: Box<dyn Fn(super::ConnectivityState) + Send + Sync>, // TODO: stream/asynciter/channel probably
    ) {
        // TODO: don't go immediately ready.
        updates(super::ConnectivityState::Ready);
    }

    fn shutdown(&self) {
        // Transition to idle.
        *self.state.lock().unwrap() = State::Idle;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[async_trait]
impl Service for Subchannel {
    async fn call(&self, request: Request) -> Response {
        let svc = self
            .state
            .lock()
            .unwrap()
            .connected_transport()
            .expect("todo: handle !ready")
            .clone();
        svc.call(request).await
    }
}
