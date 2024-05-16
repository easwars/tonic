use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use crate::{
    attributes::Attributes,
    client::{
        name_resolution::{self, Resolver, ResolverBuilder, SharedResolverBuilder},
        transport,
    },
    server,
    service::{Request, Response, Service},
};
use once_cell::sync::Lazy;
use tokio::sync::{mpsc, oneshot, Mutex};
use tonic::async_trait;

pub struct Listener {
    id: String,
    s: Box<mpsc::Sender<Option<server::Call>>>,
    r: Arc<Mutex<mpsc::Receiver<Option<server::Call>>>>,
}

static ID: AtomicU32 = AtomicU32::new(0);

impl Listener {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::channel(1);
        let s = Arc::new(Self {
            id: format!("{}", ID.fetch_add(1, Ordering::Relaxed)),
            s: Box::new(tx),
            r: Arc::new(Mutex::new(rx)),
        });
        LISTENERS.lock().unwrap().insert(s.id.clone(), s.clone());
        s
    }

    pub fn target(&self) -> String {
        format!("inmemory:///{}", self.id)
    }

    pub async fn close(&self) {
        let _ = self.s.send(None).await;
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        LISTENERS.lock().unwrap().remove(&self.id);
    }
}

#[async_trait]
impl Service for Arc<Listener> {
    async fn call(&self, request: Request) -> Response {
        // 1. unblock accept, giving it a func back to me
        // 2. return what that func had
        let (s, r) = oneshot::channel();
        self.s.send(Some((request, s))).await.unwrap();
        r.await.unwrap()
    }
}

#[async_trait]
impl crate::server::Listener for Arc<Listener> {
    async fn accept(&self) -> Option<server::Call> {
        let mut recv = self.r.lock().await;
        let r = recv.recv().await;
        if r.is_none() {
            // Listener was closed.
            return None;
        }
        r.unwrap()
    }
}

static LISTENERS: Lazy<std::sync::Mutex<HashMap<String, Arc<Listener>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

struct ClientTransport {}

impl ClientTransport {
    fn new() -> Self {
        Self {}
    }
}

impl transport::Transport for ClientTransport {
    fn connect(&self, address: String) -> Result<Box<dyn Service>, String> {
        Ok(Box::new(
            LISTENERS
                .lock()
                .unwrap()
                .get(&address)
                .ok_or(format!("Could not find listener for address {address}"))?
                .clone(),
        ))
    }
}

static INMEMORY_ADDRESS_TYPE: &str = "inmemory";

pub fn reg() {
    dbg!("Registering inmemory::ClientTransport");
    transport::GLOBAL_REGISTRY
        .add_transport(INMEMORY_ADDRESS_TYPE.to_string(), ClientTransport::new());
    name_resolution::GLOBAL_REGISTRY
        .add_builder(SharedResolverBuilder::new(InMemoryResolverBuilder));
}

struct InMemoryResolverBuilder;

#[async_trait]
impl ResolverBuilder for InMemoryResolverBuilder {
    async fn build(
        &self,
        target: url::Url,
        balancer: Arc<dyn crate::client::name_resolution::Balancer>,
        options: crate::client::name_resolution::ResolverOptions,
    ) -> Box<dyn name_resolution::Resolver> {
        let id = target.path().strip_prefix("/").unwrap();
        let _ = balancer
            .update(name_resolution::ResolverUpdate::Data(
                name_resolution::ResolverData {
                    endpoints: vec![name_resolution::Endpoint {
                        addresses: vec![name_resolution::Address {
                            address_type: INMEMORY_ADDRESS_TYPE.to_string(),
                            address: id.to_string(),
                            attributes: Attributes::new(),
                        }],
                        attributes: Attributes::new(),
                    }],
                    service_config: String::from(""),
                    attributes: Attributes::new(),
                },
            ))
            .await;

        Box::new(NopResolver {})
    }

    fn scheme(&self) -> &'static str {
        "inmemory"
    }
}

struct NopResolver {}

impl Resolver for NopResolver {
    // Ignored as there is no way to re-resolve the in-memory resolver.
    fn resolve_now(&self) {}
}
