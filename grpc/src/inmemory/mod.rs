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
    s: Box<mpsc::Sender<server::Call>>,
    r: Arc<Mutex<Option<mpsc::Receiver<server::Call>>>>,
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
        self.s.send((request, s)).await.unwrap();
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
        *self.r.lock().unwrap() = Some(recv);
        return r;
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
    fn connect(&self, address: String) -> Result<Box<dyn transport::ConnectedTransport>, String> {
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

impl transport::ConnectedTransport for Arc<Listener> {
    fn get_service(&self) -> Result<Box<dyn Service>, String> {
        Ok(Box::new(self.clone()))
    }
}

static INMEMORY_ADDRESS: &str = "inmemory";

pub fn reg() {
    dbg!("Registering inmemory::ClientTransport");
    transport::GLOBAL_REGISTRY.add_transport(
        INMEMORY_ADDRESS.to_string(),
        Box::new(ClientTransport::new()),
    );
    name_resolution::GLOBAL_REGISTRY.add_builder(Builder {});
}

struct Builder {}

impl crate::client::name_resolution::Maker for Builder {
    fn make_resolver(
        &self,
        target: url::Url,
        channel: Box<dyn name_resolution::Channel>,
        options: crate::client::name_resolution::ResolverOptions,
    ) -> Box<dyn name_resolution::Resolver> {
        let id = target.path().strip_prefix("/").unwrap();
        channel.update(Ok(name_resolution::State {
            endpoints: vec![name_resolution::Endpoint {
                addresses: vec![name_resolution::Address {
                    address_type: INMEMORY_ADDRESS.to_string(),
                    address: id.to_string(),
                    attributes: Attributes::new(),
                }],
                attributes: Attributes::new(),
            }],
            service_config: name_resolution::TODO,
            attributes: Attributes::new(),
        }));
        Box::new(Builder {})
    }

    fn scheme(&self) -> &'static str {
        "inmemory"
    }
}

impl name_resolution::Resolver for Builder {
    fn resolve_now(&self) {} // ignored.
}
