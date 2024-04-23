use std::{
    collections::HashMap,
    mem,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

use crate::{
    attributes::Attributes,
    client::name_resolution,
    server,
    service::{Request, Response, Service},
};
use once_cell::sync::Lazy;
use tokio::sync::{mpsc, oneshot};
use tonic::async_trait;

pub struct Address {
    id: u32,
}

pub struct Listener {
    id: u32,
    s: Box<mpsc::Sender<server::Call>>,
    r: Arc<Mutex<Option<mpsc::Receiver<server::Call>>>>,
}

static ID: AtomicU32 = AtomicU32::new(0);

impl Listener {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            id: ID.fetch_add(1, Ordering::AcqRel),
            s: Box::new(tx),
            r: Arc::new(Mutex::new(Some(rx))),
        }
    }

    pub fn target(&self) -> String {
        format!("inmemory:///{}", self.id)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        LISTENERS.0.lock().unwrap().remove(&self.id);
    }
}

#[async_trait]
impl Service for Listener {
    async fn call(&self, request: Request) -> Response {
        // 1. unblock accept, giving it a func back to me
        // 2. return what that func had
        let (s, r) = oneshot::channel();
        self.s.send((request, s)).await.unwrap();
        r.await.unwrap()
    }
}

#[async_trait]
impl crate::server::Listener for Listener {
    async fn accept(&self) -> Option<server::Call> {
        let mut recv = mem::replace(&mut *self.r.lock().expect("multiple calls to accept"), None);
        let res = recv.as_mut().unwrap().recv().await;
        *self.r.lock().unwrap() = recv;
        res
    }
}

struct ServerTransportMap(Arc<Mutex<HashMap<u32, Arc<Listener>>>>);

static LISTENERS: Lazy<ServerTransportMap> =
    Lazy::new(|| ServerTransportMap(Arc::new(Mutex::new(HashMap::new()))));

struct ClientTransport {}

impl crate::client::transport::Transport for ClientTransport {
    type Address = Address;
    fn connect(&self, addr: &Self::Address) -> Result<Arc<(dyn Service + 'static)>, String> {
        Err("".to_string())
    }
}

pub fn reg() {
    crate::client::transport::GLOBAL_REGISTRY.add_transport(Box::new(ClientTransport {}));
}

struct Builder {}

impl crate::client::name_resolution::Builder for Builder {
    fn build(
        &self,
        target: http::Uri,
        resolve_now: mpsc::Receiver<crate::client::name_resolution::TODO>,
        options: crate::client::name_resolution::TODO,
    ) -> mpsc::Receiver<name_resolution::Update> {
        let (tx, rx) = mpsc::channel(1);
        let id = target.path().strip_prefix("/").unwrap();
        let addr = Address {
            id: id.parse().unwrap(),
        };
        let _ = tx.blocking_send(Ok(name_resolution::State {
            endpoints: vec![Arc::new(name_resolution::Endpoint {
                addresses: vec![Arc::new(name_resolution::Address {
                    address: String::new(),
                    address_type: String::new(),
                    attributes: Attributes::new(),
                    address_alt_form: Box::new(addr),
                })],
                attributes: Attributes::new(),
            })],
            service_config: name_resolution::TODO,
            attributes: Attributes::new(),
        }));
        rx
    }

    fn scheme(&self) -> &'static str {
        todo!()
    }
}
