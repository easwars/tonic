use core::fmt;

use std::{
    error::Error,
    fmt::{Display, Formatter},
    sync::Arc,
};
use tonic::async_trait;
use url::Url;

use crate::attributes::Attributes;

mod registry;
pub use registry::{ResolverRegistry, SharedResolverBuilder, GLOBAL_RESOLVER_REGISTRY};

#[derive(Debug, PartialEq)]
pub struct TODO;

/// A name resolver factory
#[async_trait]
pub trait ResolverBuilder: Send + Sync {
    /// Builds a name resolver instance, or returns an error.
    async fn build(
        &self,
        target: Url,
        handler: Arc<dyn ResolverHandler>,
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
pub trait ResolverHandler: Send + Sync {
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
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ResolverData {
    pub endpoints: Vec<Endpoint>,
    pub service_config: String,
    // Contains optional data which can be used by the LB Policy or channel.
    pub attributes: Attributes,
}

#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Endpoint {
    pub addresses: Vec<Address>,
    // Contains optional data which can be used by the LB Policy.
    pub attributes: Attributes,
}

#[derive(Debug, Default)] // TODO: define manually to get type of addr
#[non_exhaustive]
pub struct Address {
    // The address a string describing its type and a string.
    pub address_type: String, // TODO: &'static str?
    pub address: String,
    // Contains optional data which can be used by the Subchannel or transport.
    pub attributes: Attributes,
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("hi")
    }
}

pub static TCP_IP_ADDRESS_TYPE: &str = "tcp";

pub trait Resolver: Send + Sync {
    fn resolve_now(&self);
}
