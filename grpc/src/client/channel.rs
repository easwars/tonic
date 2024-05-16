use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::vec;

use tokio::sync::watch;
use tonic::async_trait;
use url::Url; // NOTE: http::Uri requires non-empty authority portion of URI

use crate::attributes::Attributes;
use crate::credentials::Credentials;
use crate::rt;
use crate::service::{Request, Response, Service};

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
    inner: Arc<PersistentChannel>,
}

struct PersistentChannel {
    target: Url,
    active_channel: tokio::sync::Mutex<Option<Arc<ActiveChannel>>>,
}

// A channel that is not idle (connecting, ready, or erroring).
struct ActiveChannel {
    cur_state: tokio::sync::Mutex<ConnectivityState>,
    lb: Arc<LbWrapper>,
}

struct SubchannelPool {
    picker: Watcher<Box<load_balancing::Picker>>,
}

impl SubchannelPool {
    fn new() -> Self {
        Self {
            picker: Watcher::new(),
        }
    }
}

impl load_balancing::SubchannelPool for SubchannelPool {
    fn update_state(&self, update: load_balancing::Update) {
        if let Ok(s) = update {
            self.picker.update(s.picker);
        }
    }
    fn new_subchannel(
        &self,
        address: Arc<name_resolution::Address>,
    ) -> Arc<dyn load_balancing::Subchannel> {
        let t = transport::GLOBAL_REGISTRY
            .get_transport(&address.address_type)
            .unwrap();
        Arc::new(Subchannel::new(t, address.address.clone()))
    }
}

struct LbWrapper {
    policy: tokio::sync::Mutex<Option<Arc<Box<dyn load_balancing::Policy>>>>,
    subchannel_pool: Arc<SubchannelPool>,
}

impl LbWrapper {
    fn new() -> Self {
        Self {
            policy: tokio::sync::Mutex::default(),
            subchannel_pool: Arc::new(SubchannelPool::new()),
        }
    }
}

#[async_trait]
impl name_resolution::Balancer for LbWrapper {
    fn parse_config(&self) {}
    async fn update(
        self: &Self,
        state: name_resolution::ResolverUpdate,
    ) -> Result<(), Box<dyn Error>> {
        let policy_name = pick_first::POLICY_NAME;
        let name_resolution::ResolverUpdate::Data(update) = state else {
            todo!("unhandled update type");
        };
        if update.service_config != "" {
            return Err("can't do service configs yet".into());
        }
        // TODO: figure out lb policy name and config

        let mut p = self.policy.lock().await;
        match &*p {
            None => {
                let newpol = load_balancing::GLOBAL_REGISTRY
                    .get_policy(policy_name)
                    .unwrap()
                    .build(self.subchannel_pool.clone(), load_balancing::TODO);
                *p = Some(Arc::new(newpol));
            }
            _ => { /* TODO */ }
        };

        p.clone()
            .unwrap()
            .update(PolicyUpdate {
                update: name_resolution::ResolverUpdate::Data(update),
                config: super::load_balancing::TODO,
            })
            .await
        // TODO: close old LB policy gracefully vs. drop?
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
    async fn wait_for_resolver_update(&self) {
        return; // TODO
    }

    pub async fn call(&self, request: Request) -> Response {
        let ac = self.get_active_channel().await;
        self.wait_for_resolver_update().await;
        // pre-pick tasks (e.g. interceptor, retry)
        // start attempt
        // pick subchannel
        // perform attempt on transport
        let mut i = ac.lb.subchannel_pool.picker.iter();
        loop {
            if let Some(p) = i.next().await {
                let sc = p(&request).unwrap();
                let sc = sc.subchannel.as_any().downcast_ref::<Subchannel>().unwrap();
                return sc.call(request).await;
            }
        }
    }
}

impl ActiveChannel {
    async fn new(target: Url) -> Self {
        let new_ac = Self {
            cur_state: tokio::sync::Mutex::new(ConnectivityState::Connecting),
            lb: Arc::new(LbWrapper::new()),
        };

        let rb = name_resolution::GLOBAL_REGISTRY.get_scheme(target.scheme());
        let resolver = rb
            .unwrap()
            .build(
                target.clone(),
                new_ac.lb.clone(),
                name_resolution::ResolverOptions::default(),
            )
            .await;
        // TODO: save resolver in field.
        new_ac
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
            active_channel: tokio::sync::Mutex::default(),
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
