use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

use crate::{
    attributes::Attributes,
    client::{name_resolution, transport},
    server,
    service::{Request, Response, Service},
};
use once_cell::sync::Lazy;
use tokio::sync::{mpsc, oneshot};
use tonic::async_trait;

pub struct Listener {
    id: String,
    s: Box<mpsc::Sender<Option<server::Call>>>,
    r: Arc<Mutex<Option<mpsc::Receiver<Option<server::Call>>>>>,
}

static ID: AtomicU32 = AtomicU32::new(0);

impl Listener {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::channel(1);
        let s = Arc::new(Self {
            id: format!("{}", ID.fetch_add(1, Ordering::Relaxed)),
            s: Box::new(tx),
            r: Arc::new(Mutex::new(Some(rx))),
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
        let mut recv = self
            .r
            .lock()
            .unwrap()
            .take()
            .expect("multiple calls to accept");
        let r = recv.recv().await;
        if r.is_none() {
            // Listener was closed.
            return None;
        }
        *self.r.lock().unwrap() = Some(recv);
        return r.unwrap();
    }
}

static LISTENERS: Lazy<Mutex<HashMap<String, Arc<Listener>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

impl Drop for ClientTransport {
    fn drop(&mut self) {
        println!("CLIENT_TRANSPORT dropped")
    }
}

static INMEMORY_ADDRESS_TYPE: &str = "inmemory";

pub fn reg() {
    dbg!("Registering inmemory::ClientTransport");
    transport::GLOBAL_REGISTRY.add_transport(
        INMEMORY_ADDRESS_TYPE.to_string(),
        Box::new(ClientTransport::new()),
    );
    name_resolution::GLOBAL_REGISTRY.add_builder(ResolverAndBuilder {});
}

struct ResolverAndBuilder {}

impl Drop for ResolverAndBuilder {
    fn drop(&mut self) {
        println!("RAB dropped");
    }
}

impl crate::client::name_resolution::Maker for ResolverAndBuilder {
    fn make_resolver(
        &self,
        target: url::Url,
        channel: Box<dyn name_resolution::Channel>,
        options: crate::client::name_resolution::ResolverOptions,
    ) -> Box<dyn name_resolution::Resolver> {
        let id = target.path().strip_prefix("/").unwrap();
        // The inmemory resolver can't re-resolve, so ignore return value.
        // TODO: maybe log instead, but the channel should do that anyway.
        let _ = channel.update(Ok(name_resolution::State {
            endpoints: vec![name_resolution::Endpoint {
                addresses: vec![name_resolution::Address {
                    address_type: INMEMORY_ADDRESS_TYPE.to_string(),
                    address: id.to_string(),
                    attributes: Attributes::new(),
                }],
                attributes: Attributes::new(),
            }],
            service_config: name_resolution::TODO,
            attributes: Attributes::new(),
        }));
        Box::new(ResolverAndBuilder {})
    }

    fn scheme(&self) -> &'static str {
        "inmemory"
    }
}

impl name_resolution::Resolver for ResolverAndBuilder {
    fn resolve_now(&self) {} // ignored.
}
