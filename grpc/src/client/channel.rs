use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::vec;

use tokio::sync::mpsc;
use tonic::async_trait;
use url::Url; // NOTE: http::Uri requires non-empty authority portion of URI

use crate::attributes::Attributes;
use crate::credentials::Credentials;
use crate::rt;
use crate::service::{Request, Response, Service};

use super::load_balancing::{pick_first, ResolverUpdate};
use super::subchannel::Subchannel;
use super::{load_balancing, name_resolution, transport};

#[non_exhaustive]
pub struct ChannelOptions<'a> {
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
    pub name_resolver_registry: Option<super::load_balancing::Registry<'a>>,
    pub lb_policy_registry: Option<super::name_resolution::Registry>,

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

impl<'a> Default for ChannelOptions<'a> {
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

impl<'a> ChannelOptions<'a> {
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
    target: Url,
    cur_state: Arc<Mutex<ConnectivityState>>,
    lb: Arc<Mutex<Option<Box<dyn load_balancing::Policy>>>>,
}

#[derive(Copy, Clone, PartialEq)]
pub enum ConnectivityState {
    Idle,
    Connecting,
    Ready,
    TransientFailure,
    Shutdown,
}
pub struct TODO;

impl Channel {
    // Channels begin idle so new is a simple constructor.  Required parameters
    // are not in ChannelOptions.
    pub fn new(
        target: &str,
        credentials: Option<Box<dyn Credentials>>,
        runtime: Option<Box<dyn rt::Runtime>>,
        options: ChannelOptions,
    ) -> Self {
        Self {
            target: Url::from_str(target).unwrap(), // TODO handle err
            cur_state: Arc::new(Mutex::new(ConnectivityState::Idle)),
            lb: Arc::new(Mutex::new(None)),
        }
    }
    // Waits until all outstanding RPCs are completed, then stops the client
    // (via "drop"?).
    pub async fn graceful_stop(self) {}

    // Causes the channel to enter idle mode immediately.  Any pending RPCs
    // will continue on existing connections.
    // TODO: do we want this?
    //pub async fn enter_idle(&self) {}

    /// Returns the current state of the channel.
    pub fn state(&mut self, connect: bool) -> ConnectivityState {
        *self.cur_state.lock().unwrap()
    }

    /// Waits for the state of the channel to change from source.  Times out and returns an error after the deadline.
    pub async fn wait_for_state_change(
        &self,
        source: ConnectivityState,
        deadline: Instant,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn wake_if_idle(&self) {
        let mut s = self.cur_state.lock().unwrap();
        if *s == ConnectivityState::Idle {
            *s = ConnectivityState::Connecting;
            println!("creating resolver for scheme: {}", self.target.scheme());
            let lb = name_resolution::GLOBAL_REGISTRY.get_scheme(self.target.scheme());
            let (tx, rx) = mpsc::channel(1);
            let mut lbrx = lb
                .unwrap()
                .build(self.target.clone(), rx, name_resolution::TODO);
            let slf = self.clone();
            tokio::spawn(async move {
                while let Some(Ok(state)) = lbrx.recv().await {
                    println!("got update: {:?}", state);
                    slf.update_lb(state).await;
                }
            });
        }
        return; // TODO
    }
    async fn wait_for_resolver_update(&self) {
        return; // TODO
    }

    async fn update_lb(&self, state: name_resolution::State) {
        let policy_name = pick_first::POLICY_NAME;
        if state.service_config != name_resolution::TODO {
            // TODO: figure out lb policy name and config
            todo!();
        }
        let mut p = self.lb.lock().unwrap();
        match &*p {
            None => {
                let newpol = load_balancing::GLOBAL_REGISTRY
                    .get_policy(policy_name)
                    .unwrap()
                    .build(Arc::new(self.clone()), load_balancing::TODO);
                *p = Some(newpol);
            }
            _ => {}
        };
        p.as_deref_mut().unwrap().resolver_update(ResolverUpdate {
            update: Ok(state),
            config: load_balancing::TODO,
        });
        // TODO: close old LB policy gracefully vs. drop?
    }
}

impl load_balancing::Channel for Channel {
    fn new_subchannel(
        &self,
        address: Arc<name_resolution::Address>,
    ) -> Arc<dyn load_balancing::Subchannel> {
        dbg!("Address: ", &address.addr);
        let t = transport::GLOBAL_REGISTRY
            .get_transport(&address.addr)
            .unwrap();
        Arc::new(Subchannel::new(t))
    }

    fn update_state(&self, update: load_balancing::Update) {
        todo!()
    }
}

#[async_trait]
impl Service for Channel {
    async fn call(&self, request: Request) -> Response {
        self.wake_if_idle().await;
        self.wait_for_resolver_update().await;
        // pre-pick tasks (e.g. interceptor)
        // start attempt
        // pick subchannel
        // perform attempt on transport
        println!("performing call?");
        Response::new()
    }
}
