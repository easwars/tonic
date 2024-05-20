use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{watch, Notify};
use tokio::task::AbortHandle;
use tonic::async_trait;

use super::load_balancing::{self, LbUpdate, Picker};
use super::name_resolution::Address;
use super::transport::{self, ConnectedTransport, Transport};
use super::ConnectivityState;
use crate::service::{Request, Response, Service};

struct TODO;

struct Subchannel {
    t: Arc<dyn Transport>,
    state: Mutex<State>,
    address: String,
    backoff: TODO,
    listeners: Mutex<Vec<Box<dyn Fn(ConnectivityState) + Send + Sync>>>,
}

type SharedService = Arc<dyn ConnectedTransport>;

enum State {
    Idle,
    Connecting(AbortHandle),
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

impl Drop for State {
    fn drop(&mut self) {
        if let State::Connecting(ah) = self {
            ah.abort();
        }
    }
}

impl Subchannel {
    /// Creates a new subchannel in idle state.
    fn new(t: Arc<dyn Transport>, address: String) -> Self {
        Subchannel {
            t,
            state: Mutex::new(State::Idle),
            address,
            backoff: TODO,
            listeners: Mutex::default(),
        }
    }

    /// Wait for any in-flight RPCs to terminate and then close the connection
    /// and destroy the Subchannel.
    async fn drain(self) {}

    /// Begins connecting the subchannel asynchronously.  If now is set, does
    /// not wait for any pending connection backoff to complete.
    fn connect(&self, now: bool) {
        let mut state = self.state.lock().unwrap();
        match &*state {
            State::Idle => {
                let n = Arc::new(Notify::new());
                let n2 = n.clone();
                // TODO: safe alternative? This task is aborted in drop so self
                // can never outlive it.
                let s = unsafe { std::mem::transmute::<&Subchannel, &'static Subchannel>(self) };
                let fut = async move {
                    // Block until the Connecting state is set so we can't race
                    // and set Ready first.
                    n2.notified().await;
                    let svc =
                        s.t.connect(s.address.clone())
                            .await
                            .expect("todo: handle error (go TF w/backoff)");
                    let svc: Arc<dyn ConnectedTransport> = svc.into();
                    s.to_ready(svc.clone());
                    svc.disconnected().await;
                    s.to_idle();
                };
                let jh = tokio::task::spawn(fut);
                *state = State::Connecting(jh.abort_handle());
                n.notify_one();
            }
            State::TransientFailure(_) => {
                // TODO: remember connect request and skip Idle when expires
            }
            State::Ready(_) => {}      // Cannot connect while ready.
            State::Connecting(_) => {} // Already connecting.
        }
    }

    fn to_ready(&self, svc: Arc<dyn ConnectedTransport>) {
        *self.state.lock().unwrap() = State::Ready(svc);
        self.notify_listeners();
    }

    fn to_idle(&self) {
        *self.state.lock().unwrap() = State::Idle;
        self.notify_listeners();
    }

    fn notify_listeners(&self) {
        let state = match *self.state.lock().unwrap() {
            State::Idle => ConnectivityState::Idle,
            State::Connecting(_) => ConnectivityState::Connecting,
            State::Ready(_) => ConnectivityState::Ready,
            State::TransientFailure(_) => ConnectivityState::TransientFailure,
        };
        let listeners = self.listeners.lock().unwrap();
        for lis in &*listeners {
            lis(state);
        }
    }
}

// TODO: this should be a wrapper type that allows sharing the real subchannel
// between LB policies.
impl load_balancing::Subchannel for Subchannel {
    fn connect(&self) {
        Subchannel::connect(self, false);
    }

    fn listen(
        &self,
        updates: Box<dyn Fn(super::ConnectivityState) + Send + Sync>, // TODO: stream/asynciter/channel probably
    ) {
        self.listeners.lock().unwrap().push(updates);
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

pub(crate) struct SubchannelPool {
    picker: Watcher<Box<dyn Picker>>,
    // TODO: HashSet<Subchannel>
}

impl SubchannelPool {
    pub(crate) fn new() -> Self {
        Self {
            picker: Watcher::new(),
        }
    }
    pub(crate) async fn call(&self, request: Request) -> Response {
        let mut i = self.picker.iter();
        loop {
            if let Some(p) = i.next().await {
                let sc = p.pick(&request).unwrap();
                let sc = sc.subchannel.as_any().downcast_ref::<Subchannel>().unwrap();
                return sc.call(request).await;
            }
        }
    }
}

impl load_balancing::SubchannelPool for SubchannelPool {
    fn update_state(&self, update: LbUpdate) {
        if let Ok(s) = update {
            self.picker.update(s.picker);
        }
    }
    fn new_subchannel(&self, address: Arc<Address>) -> Arc<dyn load_balancing::Subchannel> {
        let t = transport::GLOBAL_TRANSPORT_REGISTRY
            .get_transport(&address.address_type)
            .unwrap();
        Arc::new(Subchannel::new(t, address.address.clone()))
    }
}

// Enables multiple receivers to view data output from a single producer.
// Producer calls update.  Consumers call iter() and call next() until they find
// a good value or encounter None.
struct Watcher<T> {
    tx: watch::Sender<Option<Arc<T>>>,
    rx: watch::Receiver<Option<Arc<T>>>,
}

impl<T> Watcher<T> {
    fn new() -> Self {
        let (tx, rx) = watch::channel(None);
        Self { tx, rx }
    }

    fn iter(&self) -> WatcherIter<T> {
        let mut rx = self.rx.clone();
        rx.mark_changed();
        WatcherIter { rx }
    }

    fn update(&self, item: T) {
        self.tx.send(Some(Arc::new(item))).unwrap();
    }
}

struct WatcherIter<T> {
    rx: watch::Receiver<Option<Arc<T>>>,
}

impl<T> WatcherIter<T> {
    // next returns None when the Watcher is dropped.
    async fn next(&mut self) -> Option<Arc<T>> {
        loop {
            self.rx.changed().await.ok()?;
            let x = self.rx.borrow_and_update();
            if x.is_some() {
                return x.clone();
            }
        }
    }
}
