use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex},
};

use once_cell::sync::Lazy;

use crate::service::Service;
pub trait Address: Any + Display + std::fmt::Debug + Send + Sync {}

pub trait Transport: Send + Sync {
    fn connect(&self, addr: &Box<dyn Address>) -> Result<Box<dyn Service>, String>;
}

/// A registry to store and retrieve transports.  Transports are indexed by
/// the address type they are intended to handle.
pub struct Registry {
    m: Arc<Mutex<HashMap<TypeId, Arc<dyn Transport>>>>,
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
    pub fn add_transport<A, T>(&self, transport: T)
    where
        A: 'static,
        T: 'static + Transport,
    {
        //let a: Arc<dyn Any> = transport;
        //let a: Arc<dyn Transport<Addr = dyn Any>> = transport;
        self.m
            .lock()
            .unwrap()
            .insert(TypeId::of::<A>(), Arc::new(transport));
    }
    /// Retrieve a name resolver from the registry, or None if not found.
    pub fn get_transport(&self, addr: &Box<dyn Address>) -> Result<Box<dyn Service>, String> {
        self.m
            .lock()
            .unwrap()
            .get(&(**addr).type_id())
            .ok_or(format!("no transport found for address {addr}")) // TODO: print address
            .and_then(|t| t.connect(addr))
    }
}

/// The registry used if a local registry is not provided to a channel or if it
/// does not exist in the local registry.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(|| Registry::new());
