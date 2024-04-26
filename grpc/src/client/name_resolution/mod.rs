use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};
use url::Url;

use crate::attributes::Attributes;

#[derive(Debug, PartialEq)]
pub struct TODO;

/// A registry to store and retrieve name resolvers.  Resolvers are indexed by
/// the URI scheme they are intended to handle.
pub struct Registry {
    m: Arc<Mutex<HashMap<String, Arc<dyn Maker>>>>,
}

impl Registry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a name resolver into the registry.
    pub fn add_builder(&self, builder: impl Maker + 'static) {
        self.m
            .lock()
            .unwrap()
            .insert(builder.scheme().to_string(), Arc::new(builder));
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_scheme(&self, name: &str) -> Option<Arc<dyn Maker>> {
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
pub trait Maker: Send + Sync {
    /// Builds a name resolver instance, or returns an error.
    fn make_resolver(
        &self,
        target: Url,
        channel: Box<dyn Channel>,
        options: ResolverOptions,
    ) -> Box<dyn Resolver>;
    /// Reports the URI scheme handled by this name resolver.
    fn scheme(&self) -> &'static str;
    fn authority<'a>(&self, target: &'a Url) -> &'a str {
        let path = target.path();
        path.strip_prefix("/").unwrap_or(path)
    }
}

pub type Update = Result<State, String>;

#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ResolverOptions {
    authority: String,
}

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
    // The address a string describing its type and a string.
    pub address_type: String, // TODO: &'static str?
    pub address: String,
    // Contains optional data which can be used by the Subchannel or transport.
    pub attributes: Attributes,
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("hi")
    }
}

pub static TCP_IP_ADDRESS_TYPE: &str = "tcp";

pub trait Resolver {
    fn resolve_now(&self);
}

#[non_exhaustive]
pub struct BuildOptions {
    // For calling into the parent channel/wrapper:
    pub parse_service_config: Box<dyn Fn(&str) -> TODO>,
    pub update: Box<dyn Fn(Update) -> Result<(), String>>,
}
