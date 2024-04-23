use http::Uri;
use once_cell::sync::Lazy;
use std::{any::Any, collections::HashMap, net::IpAddr, sync::Arc};
use tokio::sync::mpsc;

use crate::attributes::Attributes;

pub struct TODO;

/// A registry to store and retrieve name resolvers.  Resolvers are indexed by
/// the URI scheme they are intended to handle.
pub struct Registry {
    m: HashMap<String, Box<dyn Builder>>,
}

impl Registry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self { m: HashMap::new() }
    }
    /// Add a name resolver into the registry.
    pub fn add_builder(&mut self, builder: Box<dyn Builder>) {
        self.m.insert(builder.scheme().to_string(), builder);
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_scheme(&self, name: &str) -> Option<&Box<dyn Builder>> {
        self.m.get(name)
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
        target: Uri,
        resolve_now: mpsc::Receiver<TODO>,
        options: TODO,
    ) -> mpsc::Receiver<Update>;
    /// Reports the URI scheme handled by this name resolver.
    fn scheme(&self) -> &'static str;
    fn authority(&self, target: Uri) -> String {
        String::from("")
    }
}

pub type Update = Result<State, String>;

/// Data provided by the name resolver
pub struct State {
    pub endpoints: Vec<Arc<Endpoint>>,
    pub service_config: TODO,
    // Contains optional data which can be used by the LB Policy or channel.
    pub attributes: Attributes,
}

pub struct Endpoint {
    pub addresses: Vec<Arc<Address>>,
    // Contains optional data which can be used by the LB Policy.
    pub attributes: Attributes,
}

pub struct Address {
    pub address: String,
    // Specifies the transport that should be used with this address.
    pub address_type: String,
    // Contains optional data which can be used by the Subchannel or transport.
    pub attributes: Attributes,

    // OR instead of address + address_type:
    pub address_alt_form: Box<dyn Any>,
}

// Example of an address:
pub struct TcpIpAddress {
    address: IpAddr, // or String?
}
// Then a registry of address types and which transport should handle that address type.

pub trait Resolver {
    fn resolve_now(&self);
}
