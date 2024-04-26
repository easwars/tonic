use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::vec;

use tokio::sync::watch;
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
    inner: Arc<Inner>,
}

struct Inner {
    target: Url,
    cur_state: Mutex<ConnectivityState>,
    lb: Mutex<Option<Box<dyn load_balancing::Policy>>>,
    picker: PickerWrapper,
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
    // (via "drop"?).
    pub async fn graceful_stop(self) {}

    // Causes the channel to enter idle mode immediately.  Any pending RPCs
    // will continue on existing connections.
    // TODO: do we want this?
    //pub async fn enter_idle(&self) {}

    /// Returns the current state of the channel.
    pub fn state(&mut self, connect: bool) -> ConnectivityState {
        *self.inner.cur_state.lock().unwrap()
    }

    /// Waits for the state of the channel to change from source.  Times out and returns an error after the deadline.
    // TODO: do we want this or another API based on streaming?
    pub async fn wait_for_state_change(
        &self,
        source: ConnectivityState,
        deadline: Instant,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn wake_if_idle(&self) {
        let mut s = self.inner.cur_state.lock().unwrap();
        if *s == ConnectivityState::Idle {
            *s = ConnectivityState::Connecting;
            println!(
                "creating resolver for scheme: {}",
                self.inner.target.scheme()
            );
            let rb = name_resolution::GLOBAL_REGISTRY.get_scheme(self.inner.target.scheme());
            let resolver = rb.unwrap().make_resolver(
                self.inner.target.clone(),
                Box::new(self.inner.clone()),
                name_resolution::ResolverOptions::default(),
            );
            let slf = self.clone();
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
            cur_state: Mutex::new(ConnectivityState::Idle),
            lb: Mutex::new(None),
            picker: PickerWrapper::new(),
        }
    }

    fn update_lb(self: &Arc<Self>, state: name_resolution::State) {
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
                    .build(self.clone(), load_balancing::TODO);
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

impl name_resolution::Channel for Arc<Inner> {
    fn parse_service_config(&self, config: String) -> name_resolution::TODO {
        todo!()
    }

    fn update(&self, update: name_resolution::Update) {
        if let Ok(state) = update {
            println!("got update: {:?}", state);
            self.clone().update_lb(state);
        } // TODO: handle error
    }
}

impl load_balancing::Channel for Inner {
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
        self.picker.update(u.picker);
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
        let mut i = self.inner.picker.iter();
        loop {
            println!("pick ?");
            if let Some(p) = i.next().await {
                println!("got picker");
                let sc = p(&request).unwrap();
                let sc = sc.subchannel.as_any().downcast_ref::<Subchannel>().unwrap();
                return sc.call(request).await;
            }
        }
    }
}

struct PickerWrapper {
    tx: watch::Sender<Option<Arc<Box<load_balancing::Picker>>>>,
    rx: watch::Receiver<Option<Arc<Box<load_balancing::Picker>>>>,
}
struct PickerWrapperIter {
    rx: watch::Receiver<Option<Arc<Box<load_balancing::Picker>>>>,
}

impl PickerWrapper {
    fn new() -> Self {
        let (tx, rx) = watch::channel(None);
        Self { tx, rx }
    }

    fn iter(&self) -> PickerWrapperIter {
        let mut rx = self.rx.clone();
        rx.mark_changed();
        PickerWrapperIter {
            rx: self.rx.clone(),
        }
    }

    fn update(&self, picker: Box<load_balancing::Picker>) {
        println!("updating picker");
        self.tx.send(Some(Arc::new(picker))).unwrap();
    }
}

impl PickerWrapperIter {
    async fn next(&mut self) -> Option<Arc<Box<load_balancing::Picker>>> {
        self.rx.changed().await.ok()?;
        let x = self.rx.borrow_and_update();
        x.clone()
    }
}
