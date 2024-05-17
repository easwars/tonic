use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec;

use tokio::sync::{watch, Mutex, Notify};
use tonic::async_trait;
use url::Url; // NOTE: http::Uri requires non-empty authority portion of URI

use crate::attributes::Attributes;
use crate::credentials::Credentials;
use crate::rt;
use crate::service::{Request, Response, Service};

use super::load_balancing::{
    pick_first, LbPolicy, LbPolicyOptions, LbPolicyRegistry, LbPolicyUpdate, LbUpdate, Picker,
    GLOBAL_LB_REGISTRY,
};
use super::name_resolution::{
    Address, ResolverBuilder, ResolverHandler, ResolverOptions, ResolverRegistry, ResolverUpdate,
    GLOBAL_RESOLVER_REGISTRY,
};
use super::subchannel::Subchannel;
use super::transport::TransportRegistry;
use super::{load_balancing, transport};

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
    pub transport_registry: Option<TransportRegistry>,
    pub name_resolver_registry: Option<LbPolicyRegistry>,
    pub lb_policy_registry: Option<ResolverRegistry>,

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
    inner: Arc<PersistentChannel>,
}

struct PersistentChannel {
    target: Url,
    active_channel: Mutex<Option<Arc<ActiveChannel>>>,
}

// A channel that is not idle (connecting, ready, or erroring).
struct ActiveChannel {
    cur_state: Mutex<ConnectivityState>,
    lb: Arc<LbWrapper>,
}

struct SubchannelPool {
    picker: Watcher<Box<dyn Picker>>,
}

impl SubchannelPool {
    fn new() -> Self {
        Self {
            picker: Watcher::new(),
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

struct LbWrapper {
    policy: Mutex<Option<Arc<Box<dyn LbPolicy>>>>,
    subchannel_pool: Arc<SubchannelPool>,
    first_update: Notify,
}

impl LbWrapper {
    fn new() -> Self {
        Self {
            policy: Mutex::default(),
            subchannel_pool: Arc::new(SubchannelPool::new()),
            first_update: Notify::new(),
        }
    }
    async fn wait_for_resolver_update(&self) {
        self.first_update.notified().await;
    }
    async fn handle_update(self: &Self, state: ResolverUpdate) -> Result<(), Box<dyn Error>> {
        let policy_name = pick_first::POLICY_NAME;
        let ResolverUpdate::Data(update) = state else {
            todo!("unhandled update type");
        };
        if update.service_config != "" {
            return Err("can't do service configs yet".into());
        }
        // TODO: figure out lb policy name and config

        let mut p = self.policy.lock().await;
        match &*p {
            None => {
                let newpol = GLOBAL_LB_REGISTRY
                    .get_policy(policy_name)
                    .unwrap()
                    .build(self.subchannel_pool.clone(), LbPolicyOptions {});
                *p = Some(Arc::new(newpol));
            }
            _ => { /* TODO */ }
        };

        p.clone()
            .unwrap()
            .update(LbPolicyUpdate {
                update: ResolverUpdate::Data(update),
                config: super::load_balancing::TODO,
            })
            .await
        // TODO: close old LB policy gracefully vs. drop?
    }
}

#[async_trait]
impl ResolverHandler for LbWrapper {
    fn parse_config(&self) {}
    async fn update(self: &Self, state: ResolverUpdate) -> Result<(), Box<dyn Error>> {
        let res = self.handle_update(state).await;
        self.first_update.notify_one();
        res
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ConnectivityState {
    Idle,
    Connecting,
    Ready,
    TransientFailure,
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
            inner: Arc::new(PersistentChannel::new(
                target,
                credentials,
                runtime,
                options,
            )),
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
        let ac = self.inner.active_channel.blocking_lock();
        if ac.is_none() {
            return ConnectivityState::Idle;
        }
        *ac.clone().unwrap().cur_state.blocking_lock()
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

    async fn get_active_channel(&self) -> Arc<ActiveChannel> {
        let mut s = self.inner.active_channel.lock().await;
        if s.is_none() {
            *s = Some(Arc::new(
                ActiveChannel::new(self.inner.target.clone()).await,
            ));
        }
        s.clone().unwrap()
    }

    pub async fn call(&self, request: Request) -> Response {
        let ac = self.get_active_channel().await;
        ac.call(request).await
    }
}

impl ActiveChannel {
    async fn new(target: Url) -> Self {
        let new_ac = Self {
            cur_state: Mutex::new(ConnectivityState::Connecting),
            lb: Arc::new(LbWrapper::new()),
        };

        let rb = GLOBAL_RESOLVER_REGISTRY.get_scheme(target.scheme());
        let resolver = rb
            .unwrap()
            .build(
                target.clone(),
                new_ac.lb.clone(),
                ResolverOptions::default(),
            )
            .await;
        // TODO: save resolver in field.
        new_ac
    }

    async fn call(&self, request: Request) -> Response {
        self.lb.wait_for_resolver_update().await;
        // pre-pick tasks (e.g. deadlines, interceptors, retry)
        // start attempt
        // pick subchannel
        // perform attempt on transport
        let mut i = self.lb.subchannel_pool.picker.iter();
        loop {
            if let Some(p) = i.next().await {
                let sc = p.pick(&request).unwrap();
                let sc = sc.subchannel.as_any().downcast_ref::<Subchannel>().unwrap();
                return sc.call(request).await;
            }
        }
    }
}

impl PersistentChannel {
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
            active_channel: Mutex::default(),
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
