use once_cell::sync::Lazy;
use std::{any::Any, collections::HashMap, error::Error, net::IpAddr};
use url::Url;

use crate::attributes::Attributes;

pub struct TODO;

/// A registry to store and retrieve name resolvers.  Resolvers are indexed by
/// the URI scheme they are intended to handle.
pub struct Registry<'a> {
    m: HashMap<String, &'a (dyn Builder)>,
}

impl<'a> Registry<'a> {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self { m: HashMap::new() }
    }
    /// Add a name resolver into the registry.
    pub fn add_builder(&mut self, builder: &'a impl Builder) {
        self.m.insert(builder.scheme().to_string(), builder);
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_scheme(&self, name: &str) -> Option<&(dyn Builder)> {
        self.m.get(name).and_then(|&f| Some(f))
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());

/// This channel is a set of features the name resolver may use from the channel.
pub trait Channel: Send + Sync {
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
        channel: Box<dyn Channel>,
        options: TODO,
    ) -> Result<Box<dyn Resolver>, Box<dyn Error>>;
    /// Reports the URI scheme handled by this name resolver.
    fn scheme(&self) -> &'static str;
}

pub type Update = Result<State, Box<dyn Error>>;

/// Data provided by the name resolver
pub struct State {
    endpoints: Vec<Endpoint>,
    service_config: TODO,
    // Contains optional data which can be used by the LB Policy.
    attributes: Attributes,
}

pub struct Endpoint {
    addresses: Vec<Address>,
    // Contains optional data which can be used by the LB Policy.
    attributes: Attributes,
}

pub struct Address {
    address: String,
    // Specifies the transport that should be used with this address.
    address_type: String,
    // Contains optional data which can be used by the Subchannel or transport.
    attributes: Attributes,

    // OR instead of address + address_type:
    address_alt_form: Box<dyn Any>,
}

// Example of an address:
pub struct TcpIpAddress {
    address: IpAddr, // or String?
}
// Then a registry of address types and which transport should handle that address type.

pub trait Resolver {
    fn resolve_now(&self);
}
