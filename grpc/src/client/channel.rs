use std::error::Error;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::vec;

use crate::attributes::Attributes;
use crate::credentials::Credentials;
use crate::rt;
use crate::service::{Request, Response, Service};

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
    pub name_resolver_registry: Option<super::load_balancing::Registry<'a>>,
    pub lb_policy_registry: Option<super::name_resolution::Registry<'a>>,

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
pub struct Channel {
    cur_state: Mutex<ConnectivityState>,
}

#[derive(Copy, Clone)]
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
        credentials: impl Credentials,
        runtime: impl rt::Runtime,
        options: ChannelOptions,
    ) -> Self {
        Self {
            cur_state: Mutex::new(ConnectivityState::Idle),
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
}

impl Service for Channel {
    async fn call(&self, request: Request) -> Response {
        Response::new()
    }
}
