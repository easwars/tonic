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
        name_resolution::{self, ResolverBuilder, ResolverUpdate, SharedResolverBuilder},
        transport,
    },
    server,
    service::{Message, MessageService, Request, Response},
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
impl MessageService for Arc<Listener> {
    async fn call(&self, request: Request<Box<dyn Message>>) -> Response<Box<dyn Message>> {
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
    fn connect(&self, address: String) -> Result<Box<dyn MessageService>, String> {
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

impl ResolverBuilder for InMemoryResolverBuilder {
    fn build(
        &self,
        target: url::Url,
        options: crate::client::name_resolution::ResolverOptions,
    ) -> Box<dyn name_resolution::Resolver> {
        let id = target.path().strip_prefix("/").unwrap();
        Box::new(SingleUpdateResolver::new(name_resolution::ResolverData {
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
        }))
    }

    fn scheme(&self) -> &'static str {
        "inmemory"
    }
}

struct SingleUpdateResolver {
    //rxos: oneshot::Receiver<Result<(), Box<dyn Error + Send + Sync>>>,
    tx: mpsc::Sender<ResolverUpdate>,
    rx: Mutex<mpsc::Receiver<ResolverUpdate>>,
}

impl SingleUpdateResolver {
    fn new(update: name_resolution::ResolverData) -> Self {
        let (txos, rxos) = oneshot::channel();
        let (tx, rx) = mpsc::channel(1);
        let _ = tx.try_send(ResolverUpdate::Data((update, txos)));
        tokio::spawn(async move {
            rxos.await
                .ok()
                .and_then(|r| Some(r.expect("unexpected error with update")));
        });
        Self {
            tx,
            rx: Mutex::new(rx),
        }
    }
}

#[async_trait]
impl name_resolution::Resolver for SingleUpdateResolver {
    // Ignored as there is no way to re-resolve the in-memory resolver.
    fn resolve_now(&self) {}

    async fn update(&self) -> ResolverUpdate {
        self.rx.lock().await.recv().await.unwrap()
        //.unwrap_or(Err(String::from("resolver closed")))
    }
}
