use std::{
    any::Any,
    collections::HashMap,
    error::Error,
    hash::Hash,
    sync::{
        atomic::{AtomicU32, Ordering::Relaxed},
        Arc,
    },
};
use tokio::sync::Notify;
use tonic::metadata::MetadataMap;

use crate::service::{Request, Response};

use super::{
    name_resolution::{Address, ResolverUpdate},
    ConnectivityState,
};

pub mod pick_first;

mod registry;
pub use registry::{LbPolicyRegistry, GLOBAL_LB_REGISTRY};

pub struct LbPolicyOptions {
    pub work_scheduler: Box<dyn WorkScheduler>,
}

/// An LB policy factory
pub trait LbPolicyBuilder: Send + Sync {
    /// Builds an LB policy instance, or returns an error.
    fn build(&self, options: LbPolicyOptions) -> Box<dyn LbPolicy>;
    /// Reports the name of the LB Policy.
    fn name(&self) -> &'static str;
    fn parse_config(&self, config: &str) -> Option<Box<dyn LbConfig>> {
        None
    }
}

pub trait WorkScheduler: Send + Sync {
    fn schedule_work(&self, data: Box<dyn Any + Send + Sync>);
}

pub trait LbPolicy: Send + Sync {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<Box<dyn LbConfig>>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
    fn subchannel_update(
        &mut self,
        update: &SubchannelUpdate,
        channel_controller: &mut dyn ChannelController,
    );
    fn work(
        &mut self,
        channel_controller: &mut dyn ChannelController,
        data: Box<dyn Any + Send + Sync>,
    );
}

pub struct SubchannelUpdate {
    pub(crate) states: HashMap<u32, SubchannelState>,
}

#[derive(Clone)]
pub struct SubchannelState {
    /// The connectivity state of the subchannel.  See SubChannel for a
    /// description of the various states and their valid transitions.
    pub connectivity_state: ConnectivityState,
    // Set if connectivity state is TransientFailure to describe the failure.
    pub last_connection_error: Option<Arc<dyn Error + Send + Sync>>,
}

impl SubchannelUpdate {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }
    pub fn from(u: &SubchannelUpdate) -> Self {
        Self {
            states: u.states.clone().into_iter().collect(),
        }
    }
    pub fn get(&self, subchannel: &Subchannel) -> Option<&SubchannelState> {
        self.states.get(&subchannel.id)
    }
    pub fn set(&mut self, subchannel: &Subchannel, state: SubchannelState) {
        self.states.insert(subchannel.id, state);
    }
}

impl Default for SubchannelUpdate {
    fn default() -> Self {
        Self {
            states: Default::default(),
        }
    }
}

pub trait LbConfig: Send {
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

pub trait Picker: Send + Sync {
    fn pick(&self, request: &Request) -> Result<Pick, Box<dyn Error>>;
}

/// Data provided by the LB policy.
pub struct LbState {
    pub connectivity_state: super::ConnectivityState,
    pub picker: Box<dyn Picker>,
}

pub struct Pick {
    pub subchannel: Subchannel,
    pub on_complete: Option<Box<dyn Fn(&Response) + Send + Sync>>,
    pub metadata: Option<MetadataMap>, // to be added to existing outgoing metadata
}

/// Controls channel behaviors.
pub trait ChannelController: Send + Sync {
    /// Creates a new subchannel in IDLE state.
    fn new_subchannel(&mut self, address: &Address) -> Subchannel;
    fn update_picker(&mut self, update: LbState);
    fn request_resolution(&mut self);
}

/// A Subchannel represents a method of communicating with an address which may
/// be connected or disconnected many times across its lifetime.
///
/// - Subchannels start IDLE and transition to CONNECTING when connect is
///   called.
///
/// - CONNECTING leads to either READY on success or TRANSIENT_FAILURE on error.
///
/// - READY transitions to IDLE when the connection is lost.
///
/// - TRANSIENT_FAILURE transitions to CONNECTING when the reconnect backoff
///   timer has expired.  This timer scales exponentially and is reset when the
///   subchannel becomes READY.
///
/// When a Subchannel is dropped, it is disconnected automatically, and no
/// subsequent state updates will be provided for it to the LB policy.
#[derive(Clone)]
pub struct Subchannel {
    id: u32,
    notify: Arc<Notify>,
}

static NEXT_SUBCHANNEL_ID: AtomicU32 = AtomicU32::new(0);

impl Subchannel {
    /// Creates a new Subchannel that doesn't do anything besides forward
    /// connect calls to notify.
    pub fn new(notify: Arc<Notify>) -> Self {
        Self {
            id: NEXT_SUBCHANNEL_ID.fetch_add(1, Relaxed),
            notify,
        }
    }
    /// Notifies the Subchannel to connect.
    pub fn connect(&self) {
        self.notify.notify_one();
    }
}

impl Hash for Subchannel {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for Subchannel {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Subchannel {}

pub trait LbPolicyBuilderCallbacks: Send + Sync {
    /// Builds an LB policy instance, or returns an error.
    fn build(&self, options: LbPolicyOptions) -> Box<dyn LbPolicyCallbacks>;
    /// Reports the name of the LB Policy.
    fn name(&self) -> &'static str;
    fn parse_config(&self, config: &str) -> Option<Box<dyn LbConfig>> {
        None
    }
}

pub trait LbPolicyCallbacks: Send + Sync {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<Box<dyn LbConfig>>,
        channel_controller: &mut dyn ChannelControllerCallbacks,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
}

pub trait ChannelControllerCallbacks: Send + Sync {
    /// Creates a new subchannel in IDLE state.
    fn new_subchannel(
        &mut self,
        address: &Address,
        updates: Box<dyn Fn(Subchannel, SubchannelState, &mut dyn ChannelControllerCallbacks)>,
    ) -> Subchannel;
    fn update_picker(&mut self, update: LbState);
    fn request_resolution(&mut self);
}
