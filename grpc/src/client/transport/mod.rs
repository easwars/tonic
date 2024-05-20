use crate::service::Service;

mod registry;

pub use registry::{TransportRegistry, GLOBAL_TRANSPORT_REGISTRY};
use tonic::async_trait;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self, address: String) -> Result<Box<dyn ConnectedTransport>, String>;
}

#[async_trait]
pub trait ConnectedTransport: Service {
    async fn disconnected(&self);
}
