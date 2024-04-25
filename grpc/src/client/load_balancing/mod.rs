use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, Mutex},
};
use tonic::metadata::MetadataMap;

use crate::service::{Request, Response};

use super::{name_resolution::Address, ConnectivityState};

pub mod pick_first;

pub struct TODO;

/// A registry to store and retrieve LB policies.  LB policies are indexed by
/// their names.
pub struct Registry<'a> {
    m: Arc<Mutex<HashMap<String, &'a (dyn Builder)>>>,
}

impl<'a> Registry<'a> {
    /// Construct an empty LB policy registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a LB policy into the registry.
    pub fn add_builder(&self, builder: &'a impl Builder) {
        self.m
            .lock()
            .unwrap()
            .insert(builder.name().to_string(), builder);
    }
    /// Retrieve a LB policy from the registry, or None if not found.
    pub fn get_policy(&self, name: &str) -> Option<&(dyn Builder)> {
        self.m.lock().unwrap().get(name).map(|&f| f)
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());

pub trait Subchannel: Send + Sync {
    /// Begins connecting the subchannel.
    fn connect(&self);
    // Attaches a listener to the subchannel.  Must be called before connect and
    // not after connect.
    fn listen(
        &self,
        updates: Box<dyn Fn(ConnectivityState)>, // TODO: stream/asynciter/channel probably
    );
    fn shutdown(&self);
}

/// This channel is a set of features the LB policy may use from the channel.
pub trait Channel: Send + Sync {
    /// Creates a new subchannel in idle state.
    fn new_subchannel(&self, address: Arc<Address>) -> Arc<dyn Subchannel>;
    /// Consumes an update from the LB Policy.
    fn update_state(&self, update: Update);
}

/// An LB policy factory
pub trait Builder: Send + Sync {
    /// Builds an LB policy instance, or returns an error.
    fn build(&self, channel: Arc<dyn Channel>, options: TODO) -> Box<dyn Policy>;
    /// Reports the name of the LB Policy.
    fn name(&self) -> &'static str;
}

pub type Update = Result<Arc<State>, Box<dyn Error>>;
pub type Picker = dyn Fn(Request) -> Result<Pick, Box<dyn Error>>;

/// Data provided by the LB policy.
pub struct State {
    connectivity_state: super::ConnectivityState,
    picker: Arc<Picker>,
}

impl State {
    pub fn new(connectivity_state: super::ConnectivityState, picker: Arc<Picker>) -> Self {
        Self {
            connectivity_state,
            picker,
        }
    }
}

pub struct Pick {
    subchannel: Arc<dyn Subchannel>,
    on_complete: Option<Box<dyn FnOnce(Response)>>,
    metadata: Option<MetadataMap>, // to be added to existing outgoing metadata
}

impl Pick {
    pub fn new(subchannel: Arc<dyn Subchannel>) -> Self {
        Self {
            subchannel,
            on_complete: None,
            metadata: None,
        }
    }
}

pub struct ResolverUpdate {
    pub update: super::name_resolution::Update,
    pub config: TODO, // LB policy's parsed config
}

pub trait Policy: Send + Sync {
    fn resolver_update(&mut self, update: ResolverUpdate);
}
