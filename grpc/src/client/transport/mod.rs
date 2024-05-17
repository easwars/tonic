use crate::service::Service;

mod registry;

pub use registry::{TransportRegistry, GLOBAL_TRANSPORT_REGISTRY};

pub trait Transport: Send + Sync {
    fn connect(&self, address: String) -> Result<Box<dyn Service>, String>;
}
