use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Display,
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

#[derive(Debug, Clone)]
pub struct Address {
    id: u32,
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl transport::Address for Address {}

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
            id: ID.fetch_add(1, Ordering::Relaxed),
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
impl crate::server::Listener for Listener {
    async fn accept(&self) -> Option<server::Call> {
        let mut recv = self
            .r
            .lock()
            .unwrap()
            .take()
            .expect("multiple calls to accept");
        let res = recv.recv().await;
        dbg!("got request: {:?}", res.as_ref().unwrap());
        *self.r.lock().unwrap() = Some(recv);
        res
    }
}

struct ServerTransportMap(Arc<Mutex<HashMap<u32, Arc<Listener>>>>);

static LISTENERS: Lazy<ServerTransportMap> =
    Lazy::new(|| ServerTransportMap(Arc::new(Mutex::new(HashMap::new()))));

struct ClientTransport {}

impl crate::client::transport::Transport for ClientTransport {
    fn connect(
        &self,
        addr: &Box<dyn transport::Address>,
    ) -> Result<Box<(dyn Service + 'static)>, String> {
        let addr = addr.as_ref() as &dyn Any;
        Ok(Box::new(
            LISTENERS
                .0
                .lock()
                .unwrap()
                .get(&addr.downcast_ref::<&Address>().unwrap().id)
                .unwrap()
                .clone(),
        ) as Box<dyn Service>)
    }
}

pub fn reg() {
    dbg!("Registering inmemory::ClientTransport");
    dbg!(TypeId::of::<Address>());
    transport::GLOBAL_REGISTRY.add_transport::<Address, _>(ClientTransport {});
    name_resolution::GLOBAL_REGISTRY.add_builder(Builder {});
}

struct Builder {}

impl crate::client::name_resolution::Builder for Builder {
    fn build(
        &self,
        target: url::Url,
        resolve_now: mpsc::Receiver<crate::client::name_resolution::TODO>,
        options: crate::client::name_resolution::TODO,
    ) -> mpsc::Receiver<name_resolution::Update> {
        let (tx, rx) = mpsc::channel(1);
        let id = target.path().strip_prefix("/").unwrap();
        let addr = Address {
            id: id.parse().unwrap(),
        };
        tokio::spawn(async move {
            println!("sending addr to channel {:?}", addr);
            let _ = tx
                .send(Ok(name_resolution::State {
                    endpoints: vec![name_resolution::Endpoint {
                        addresses: vec![name_resolution::Address {
                            addr: Box::new(addr),
                            attributes: Attributes::new(),
                        }],
                        attributes: Attributes::new(),
                    }],
                    service_config: name_resolution::TODO,
                    attributes: Attributes::new(),
                }))
                .await;
        });
        rx
    }

    fn scheme(&self) -> &'static str {
        "inmemory"
    }
}
