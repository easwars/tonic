use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use once_cell::sync::Lazy;

use crate::service::Service;

/// A registry to store and retrieve transports.  Transports are indexed by
/// the address type they are intended to handle.
pub struct Registry {
    m: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl Registry {
    /// Construct an empty name resolver registry.
    pub fn new() -> Self {
        Self {
            m: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// Add a name resolver into the registry.
    pub fn add_transport<A, T>(&self, transport: Box<T>)
    where
        A: 'static,
        T: 'static + Transport<Address = A>,
    {
        self.m.lock().unwrap().insert(TypeId::of::<A>(), transport);
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_transport<A, T>(&self, addr: A) -> Result<Arc<dyn Service>, String>
    where
        A: 'static + Display,
        T: 'static + Transport<Address = A>,
    {
        self.m
            .lock()
            .unwrap()
            .get(&TypeId::of::<A>())
            .ok_or(format!("no transport found for address {:}", &addr))?
            .downcast_ref::<T>()
            .unwrap()
            .connect(&addr)
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());

pub trait Transport: Send + Sync {
    type Address;

    fn connect(&self, addr: &Self::Address) -> Result<Arc<dyn Service>, String>;
}
