use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    net::IpAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use url::Url;

use crate::attributes::Attributes;

use super::transport;

#[derive(Debug, PartialEq)]
pub struct TODO;

/// A registry to store and retrieve name resolvers.  Resolvers are indexed by
/// the URI scheme they are intended to handle.
pub struct Registry {
    m: Arc<Mutex<HashMap<String, Arc<dyn Builder>>>>,
}

impl Registry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a name resolver into the registry.
    pub fn add_builder(&self, builder: impl Builder + 'static) {
        self.m
            .lock()
            .unwrap()
            .insert(builder.scheme().to_string(), Arc::new(builder));
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_scheme(&self, name: &str) -> Option<Arc<dyn Builder>> {
        self.m.lock().unwrap().get(name).map(|f| f.clone())
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());

/// This channel is a set of features the name resolver may use from the channel.
pub trait Channel {
    fn parse_service_config(&self, config: String) -> TODO;
    /// Consumes an update from the name resolver.
    fn update(&self, update: Update);
}

/// A name resolver factory
pub trait Builder: Send + Sync {
    /// Builds a name resolver instance, or returns an error.
    fn build(
        &self,
        target: Url,
        resolve_now: mpsc::Receiver<TODO>,
        options: TODO,
    ) -> mpsc::Receiver<Update>;
    /// Reports the URI scheme handled by this name resolver.
    fn scheme(&self) -> &'static str;
    fn authority<'a>(&self, target: &'a Url) -> &'a str {
        let path = target.path();
        path.strip_prefix("/").unwrap_or(path)
    }
}

pub type Update = Result<State, String>;

/// Data provided by the name resolver
#[derive(Debug)]
#[non_exhaustive]
pub struct State {
    pub endpoints: Vec<Endpoint>,
    pub service_config: TODO,
    // Contains optional data which can be used by the LB Policy or channel.
    pub attributes: Attributes,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct Endpoint {
    pub addresses: Vec<Address>,
    // Contains optional data which can be used by the LB Policy.
    pub attributes: Attributes,
}

#[derive(Debug)] // TODO: define manually to get type of addr
#[non_exhaustive]
pub struct Address {
    // The address represented as a transport address
    pub addr: Box<dyn transport::Address>,
    // Contains optional data which can be used by the Subchannel or transport.
    pub attributes: Attributes,
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("hi")
    }
}

// Example of an address:
#[derive(Debug)]
pub struct TcpIpAddress {
    address: IpAddr,
}
// Then a registry of address types and which transport should handle that address type.

pub trait Resolver {
    fn resolve_now(&self);
}

#[non_exhaustive]
pub struct BuildOptions {
    // For calling into the parent channel/wrapper:
    pub parse_service_config: Box<dyn Fn(&str) -> TODO>,
    pub update: Box<dyn Fn(Update) -> Result<(), String>>,
}
