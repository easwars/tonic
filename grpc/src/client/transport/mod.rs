use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use once_cell::sync::Lazy;

use crate::service::MessageService;

pub trait Transport: Send + Sync {
    fn connect(&self, address: String) -> Result<Box<dyn MessageService>, String>;
}

/// A registry to store and retrieve transports.  Transports are indexed by
/// the address type they are intended to handle.
pub struct Registry {
    m: Arc<Mutex<HashMap<String, Arc<dyn Transport>>>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = self.m.lock().unwrap();
        for (key, _) in &*m {
            write!(f, "k: {:?}", key)?
        }
        Ok(())
    }
}

impl Registry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a name resolver into the registry.
    pub fn add_transport(&self, address_type: String, transport: impl Transport + 'static) {
        //let a: Arc<dyn Any> = transport;
        //let a: Arc<dyn Transport<Addr = dyn Any>> = transport;
        self.m
            .lock()
            .unwrap()
            .insert(address_type, Arc::new(transport));
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_transport(&self, address_type: &String) -> Result<Arc<dyn Transport>, String> {
        self.m
            .lock()
            .unwrap()
            .get(address_type)
            .ok_or(format!(
                "no transport found for address type {address_type}"
            ))
            .map(|t| t.clone())
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());
