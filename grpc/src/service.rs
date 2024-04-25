use std::time::{Duration, Instant};

use tonic::{async_trait, metadata::MetadataMap};

#[derive(Debug)]
struct TODO;

// TODO: very similar to tower, obviously.  It's probably fine to always output
// a response, though, and the response will always contain the error as a grpc
// status & message.  Errors are delivered asynchronously in the case of a real
// RPC.
//
// Or can/should we use tonic's types (Service/Req/Res) directly?
#[async_trait]
pub trait Service: Send + Sync {
    async fn call(&self, request: Request) -> Response;
}

// TODO: are requests and responses different on client and server?  or do they
// just have different extensions available? E.g. peer would be an extension
// added to the request on the server but the client wouldn't see it.  The
// stream/messages are different: the client can write request messages where
// the server can only read them, and vice-versa.
#[derive(Debug)]
pub struct Request {
    method: String,
    stream: TODO, // A way to send/receive request messages.

    // Should all of the below optional things be "extensions"?
    metadata: MetadataMap,
    deadline: Option<Instant>,
    compressor: Option<String>,
    wait_for_ready: bool,
}

impl Request {
    pub fn new(method: String, parent: Option<Request>) -> Self {
        Self {
            method,
            stream: TODO,
            metadata: MetadataMap::new(),
            deadline: parent.and_then(|p| p.deadline),
            compressor: None,
            wait_for_ready: false,
        }
    }

    pub fn set_timeout(self, timeout: Duration) -> Self {
        Self {
            deadline: Some(Instant::now() + timeout),
            ..self
        }
    }
}

#[derive(Debug)]
pub struct Response {
    stream: TODO, // A way to stream headers, messages, and status/trailers.
}

impl Response {
    pub fn new() -> Self {
        Response { stream: TODO }
    }
}
