use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    error::Error,
    fmt::Display,
    sync::{Arc, Mutex},
};
use tonic::async_trait;
use url::Url;

use crate::attributes::Attributes;

#[derive(Debug, PartialEq)]
pub struct TODO;

#[derive(Clone)]
pub struct SharedResolverBuilder {
    rb: Arc<dyn ResolverBuilder>,
}

impl SharedResolverBuilder {
    pub fn new(rb: impl ResolverBuilder + 'static) -> Self {
        Self { rb: Arc::new(rb) }
    }
}

#[async_trait]
impl ResolverBuilder for SharedResolverBuilder {
    async fn build(
        &self,
        target: Url,
        balancer: Arc<dyn Balancer>,
        options: ResolverOptions,
    ) -> Box<dyn Resolver> {
        self.rb.build(target, balancer, options).await
    }

    fn scheme(&self) -> &'static str {
        self.rb.scheme()
    }
}

/// A registry to store and retrieve name resolvers.  Resolvers are indexed by
/// the URI scheme they are intended to handle.
pub struct ResolverRegistry {
    m: Arc<Mutex<HashMap<String, SharedResolverBuilder>>>,
}

impl ResolverRegistry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a name resolver into the registry.
    pub fn add_builder(&self, builder: SharedResolverBuilder) {
        self.m
            .lock()
            .unwrap()
            .insert(builder.scheme().to_string(), builder);
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_scheme(&self, name: &str) -> Option<SharedResolverBuilder> {
        self.m.lock().unwrap().get(name).map(|f| f.clone())
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<ResolverRegistry> = Lazy::new(|| ResolverRegistry::new());

/// A name resolver factory
#[async_trait]
pub trait ResolverBuilder: Send + Sync {
    /// Builds a name resolver instance, or returns an error.
    async fn build(
        &self,
        target: Url,
        balancer: Arc<dyn Balancer>,
        options: ResolverOptions,
    ) -> Box<dyn Resolver>;
    /// Reports the URI scheme handled by this name resolver.
    fn scheme(&self) -> &'static str;
    /// Returns the default authority for a channel using this name resolver and
    /// target.
    fn authority(&self, target: &Url) -> String {
        let path = target.path();
        path.strip_prefix("/").unwrap_or(path).to_string()
    }
}

#[async_trait]
pub trait Balancer: Send + Sync {
    fn parse_config(&self); // TODO
    async fn update(&self, update: ResolverUpdate) -> Result<(), Box<dyn Error>>;
}

pub enum ResolverUpdate {
    Err(Box<dyn Error + Send + Sync>), // The name resolver encountered an error.
    Data(ResolverData),                // The name resolver produced a result.
}

#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ResolverOptions {
    authority: String,
}

/// Data provided by the name resolver
#[derive(Debug)]
#[non_exhaustive]
pub struct ResolverData {
    pub endpoints: Vec<Endpoint>,
    pub service_config: String,
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

pub trait Resolver: Send + Sync {
    fn resolve_now(&self);
}
