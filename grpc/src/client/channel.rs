use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use std::vec;

use tokio::sync::watch;
use tonic::async_trait;
use url::Url; // NOTE: http::Uri requires non-empty authority portion of URI

use crate::attributes::Attributes;
use crate::credentials::Credentials;
use crate::rt;
use crate::service::{Message, MessageService, Request, Response, Service};

use super::load_balancing::{pick_first, PolicyUpdate};
use super::name_resolution::ResolverBuilder;
use super::subchannel::Subchannel;
use super::{load_balancing, name_resolution, transport};

#[non_exhaustive]
pub struct ChannelOptions {
    pub transport_options: Attributes, // ?
    pub override_authority: Option<String>,
    pub connection_backoff: Option<TODO>,
    pub default_service_config: Option<String>,
    pub disable_proxy: bool,
    pub disable_service_config_lookup: bool,
    pub disable_health_checks: bool,
    pub max_retry_memory: u32, // ?
    pub idle_timeout: Duration,
    pub transport_registry: Option<super::transport::Registry>,
    pub name_resolver_registry: Option<super::load_balancing::Registry>,
    pub lb_policy_registry: Option<super::name_resolution::ResolverRegistry>,

    // Typically we allow settings at the channel level that impact all RPCs,
    // but can also be set per-RPC.  E.g.s:
    //
    // - interceptors
    // - user-agent string override
    // - max message sizes
    // - max retry/hedged attempts
    // - disable retry
    //
    // In gRPC-Go, we can express CallOptions as DialOptions, which is a nice
    // pattern: https://pkg.go.dev/google.golang.org/grpc#WithDefaultCallOptions
    //
    // To do this in rust, all optional behavior for a request would need to be
    // expressed through a trait that applies a mutation to a request.  We'd
    // apply all those mutations before the user's options so the user's options
    // would override the defaults, or so the defaults would occur first.
    pub default_request_extensions: Vec<Box<TODO>>, // ??
}

impl Default for ChannelOptions {
    fn default() -> Self {
        Self {
            transport_options: Attributes::new(),
            override_authority: None,
            connection_backoff: None,
            default_service_config: None,
            disable_proxy: false,
            disable_service_config_lookup: false,
            disable_health_checks: false,
            max_retry_memory: 8 * 1024 * 1024, // 8MB -- ???
            idle_timeout: Duration::from_secs(30 * 60),
            name_resolver_registry: None,
            lb_policy_registry: None,
            default_request_extensions: vec![],
            transport_registry: None,
        }
    }
}

impl ChannelOptions {
    pub fn transport_options(self, transport_options: TODO) -> Self {
        todo!(); // add to existing options.
    }
    pub fn override_authority(self, authority: String) -> Self {
        Self {
            override_authority: Some(authority),
            ..self
        }
    }
    // etc
}

// All of Channel needs to be thread-safe.  Arc<inner>?  Or give out
// Arc<Channel> from constructor?
#[derive(Clone)]
pub struct Channel {
    inner: Arc<Inner>,
}

struct Inner {
    target: Url,
    cur_state: tokio::sync::Mutex<ConnectivityState>,
    lb: tokio::sync::Mutex<Option<Box<dyn load_balancing::Policy>>>,
    picker: Watcher<Box<load_balancing::Picker>>,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ConnectivityState {
    Idle,
    Connecting,
    Ready,
    TransientFailure,
    Shutdown,
}
pub struct TODO;

impl Channel {
    pub fn new(
        target: &str,
        credentials: Option<Box<dyn Credentials>>,
        runtime: Option<Box<dyn rt::Runtime>>,
        options: ChannelOptions,
    ) -> Self {
        Self {
            inner: Arc::new(Inner::new(target, credentials, runtime, options)),
        }
    }
    // Waits until all outstanding RPCs are completed, then stops the client
    // (via "drop"?).  Note that there probably needs to be a way to add a
    // timeout here or for the application to do a hard failure while waiting.
    // Or maybe this feature isn't necessary - Go doesn't have it.  Users can
    // determine on their own if they have outstanding calls.  Some users have
    // requested this feature for Go, nonetheless.
    pub async fn graceful_stop(self) {}

    // Causes the channel to enter idle mode immediately.  Any pending RPCs
    // will continue on existing connections.
    // TODO: do we want this?  Go does not have this.
    //pub async fn enter_idle(&self) {}

    /// Returns the current state of the channel.
    pub fn state(&mut self, connect: bool) -> ConnectivityState {
        *self.inner.cur_state.blocking_lock()
    }

    /// Waits for the state of the channel to change from source.  Times out and returns an error after the deadline.
    // TODO: do we want this or another API based on streaming?  Probably
    // something like the Watcher<T> would be nice.
    pub async fn wait_for_state_change(
        &self,
        source: ConnectivityState,
        deadline: Instant,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn wake_if_idle(&self) {
        let mut s = self.inner.cur_state.lock().await;
        if *s == ConnectivityState::Idle {
            *s = ConnectivityState::Connecting;
            let rb = name_resolution::GLOBAL_REGISTRY.get_scheme(self.inner.target.scheme());
            let resolver = rb.unwrap().build(
                self.inner.target.clone(),
                name_resolution::ResolverOptions::default(),
            );
            let inner = self.inner.clone();
            tokio::task::spawn(async move {
                // TODO: terminate this task when we go idle.
                loop {
                    let update = resolver.update().await;
                    if let name_resolution::ResolverUpdate::Data((state, tx)) = update {
                        // TODO: intercept tx?
                        inner
                            .update_lb(name_resolution::ResolverUpdate::Data((state, tx)))
                            .await;
                        continue;
                    }
                }
            });
            // TODO: save resolver in field.
        }
        return; // TODO
    }
    async fn wait_for_resolver_update(&self) {
        return; // TODO
    }
}

impl Inner {
    // Channels begin idle so new is a simple constructor.  Required parameters
    // are not in ChannelOptions.
    fn new(
        target: &str,
        credentials: Option<Box<dyn Credentials>>,
        runtime: Option<Box<dyn rt::Runtime>>,
        options: ChannelOptions,
    ) -> Self {
        Self {
            target: Url::from_str(target).unwrap(), // TODO handle err
            cur_state: tokio::sync::Mutex::new(ConnectivityState::Idle),
            lb: tokio::sync::Mutex::new(None),
            picker: Watcher::new(),
        }
    }

    async fn update_lb(self: &Arc<Self>, state: name_resolution::ResolverUpdate) {
        let policy_name = pick_first::POLICY_NAME;
        let name_resolution::ResolverUpdate::Data((update, tx)) = state else {
            todo!("unhandled update type");
        };
        if update.service_config != "" {
            let _ = tx.send(Err("can't do service configs yet".into()));
            return;
        }
        // TODO: figure out lb policy name and config

        let mut p = self.lb.lock().await;
        match &*p {
            None => {
                let newpol = load_balancing::GLOBAL_REGISTRY
                    .get_policy(policy_name)
                    .unwrap()
                    .build(Box::new(Arc::downgrade(self)), load_balancing::TODO);
                *p = Some(newpol);
            }
            _ => { /* TODO */ }
        };
        p.as_deref_mut()
            .unwrap()
            .update(PolicyUpdate {
                update: name_resolution::ResolverUpdate::Data((update, tx)),
                config: super::load_balancing::TODO,
            })
            .await;
        // TODO: close old LB policy gracefully vs. drop?
    }
}

impl load_balancing::Channel for Weak<Inner> {
    fn new_subchannel(
        &self,
        address: Arc<name_resolution::Address>,
    ) -> Arc<dyn load_balancing::Subchannel> {
        let t = transport::GLOBAL_REGISTRY
            .get_transport(&address.address_type)
            .unwrap();
        Arc::new(Subchannel::new(t, address.address.clone()))
    }

    fn update_state(&self, update: load_balancing::Update) {
        let u = update.unwrap();
        self.upgrade().unwrap().picker.update(u.picker);
    }
}

#[async_trait]
impl MessageService for Channel {
    async fn call(&self, request: Request<Into<Message>>) -> Response<From<Message>> {
        self.wake_if_idle().await;
        self.wait_for_resolver_update().await;
        // pre-pick tasks (e.g. interceptor, retry)
        // start attempt
        // pick subchannel
        // perform attempt on transport
        let mut i = self.inner.picker.iter();
        loop {
            if let Some(p) = i.next().await {
                let sc = p(&request).unwrap();
                let sc = sc.subchannel.as_any().downcast_ref::<Subchannel>().unwrap();
                return sc.call(request).await;
            }
        }
    }
}

// Enables multiple receivers to view data output from a single producer.
// Producer calls update.  Consumers call iter() and call next() until they find
// a good value or encounter None.
struct Watcher<T> {
    tx: watch::Sender<Option<Arc<T>>>,
    rx: watch::Receiver<Option<Arc<T>>>,
}

struct WatcherIter<T> {
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
