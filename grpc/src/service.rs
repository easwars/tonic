use std::{any::Any, time::Instant};

use tokio::sync::mpsc::{self, Receiver, Sender};
use tonic::async_trait;

#[derive(Debug)]
struct TODO;

// TODO: very similar to tower, obviously.  It's probably fine to always output
// a response, though, and the response will always contain the error as a grpc
// status & message.  Errors are delivered asynchronously in the case of a real
// RPC.
//
// Or can/should we use tonic's types (Service/Req/Res) directly?
#[async_trait]
pub trait Service<ReqMsg>: Send + Sync {
    type ResMsg;
    async fn call(&self, request: Request<ReqMsg>) -> Response<Self::ResMsg>;
}

#[async_trait]
pub trait MessageService: Send + Sync {
    async fn call(&self, request: Request<Box<dyn Message>>) -> Response<Box<dyn Message>>;
}

/*
#[async_trait]
impl MessageService for dyn Service<Box<dyn Message>, ResMsg = Box<dyn Message>> {
    async fn call(&self, request: Request<Box<dyn Message>>) -> Response<Box<dyn Message>> {
        todo!()
    }
}
*/

#[async_trait]
impl<Req, Res> MessageService for dyn Service<Req, ResMsg = Res> {
    async fn call(&self, request: Request<Box<dyn Message>>) -> Response<Box<dyn Message>> {
        // wrap Request so that next() calls
        let (x, y) = Response::new();
        x
    }
}

// TODO: are requests and responses different on client and server?  or do they
// just have different extensions available? E.g. peer would be an extension
// added to the request on the server but the client wouldn't see it.  The
// stream/messages are different: the client can write request messages where
// the server can only read them, and vice-versa.
#[derive(Debug)]
pub struct Request<Req> {
    pub method: String,
    rx: Receiver<Req>,
    // Should all of the below optional things be "extensions"?
    /*metadata: MetadataMap,
    deadline: Option<Instant>,
    compressor: Option<String>,
    wait_for_ready: bool,*/
}

pub trait ParentRequest {
    fn deadline(&self) -> Instant;
}

impl<Req: Message> Request<Req> {
    pub fn new(method: String, parent: Option<&dyn ParentRequest>) -> (Self, Sender<Req>) {
        let (tx, rx) = mpsc::channel(1);
        (Self { method, rx }, tx)
    }
    pub async fn next(&mut self) -> Option<Req> {
        self.rx.recv().await
    }
    fn headers(&mut self) -> Headers {
        Headers {}
    }
}

impl Request<Box<dyn Message>> {}

#[derive(Debug)]
pub struct Response<Res> {
    rx: Receiver<Res>, // TODO: include headers & trailers in one stream?
}

// Client -> static msg type -> grpc (as dyn Message) -> Transport (as Box<dyn Message>) -> wire -> Transport (as Box<dyn Message>) -> grpc (as Box<dyn Message>) -> Server (as static Message type)
// Are client-side and server-side view of requests and responses the same?
// client can set things; server can view.  But that can be handled by the builder, and the request should be a const.
// Server can also read things from it that the client can't, like the client's address.

pub trait Message: Send {
    fn as_any(&self) -> &dyn Any;
    // TODO: to_bytes() or whatever
    //fn into(self) -> Self;
}
pub struct Headers {}
pub struct Trailers {}

struct MessageBytes {}

impl<Res: Message> Response<Res> {
    pub fn new() -> (Self, Sender<Res>) {
        let (tx, rx) = mpsc::channel(1);
        (Self { rx }, tx)
    }
    pub async fn next(&mut self) -> Option<Res> {
        self.rx.recv().await
    }
    pub async fn headers(&mut self) -> Headers {
        Headers {}
    }
    pub async fn trailers(&mut self) -> Trailers {
        Trailers {}
    }
}

impl Into<Box<dyn Any>> for Box<dyn Message> {
    fn into(self) -> Box<dyn Any> {
        todo!()
    }
}

impl Message for Box<dyn Message> {
    fn as_any(&self) -> &dyn Any {
        self.as_ref().as_any()
    }
}
