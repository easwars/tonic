use tonic::async_trait;

use crate::service::{Request, Response, Service};

pub(crate) struct Subchannel {}

impl Subchannel {
    /// Creates a new subchannel in idle state.
    pub fn new() -> Self {
        Subchannel {}
    }
    /// Drain waits for any in-flight RPCs to terminate and then closes the
    /// connection and consumes the Subchannel.
    pub async fn drain(self) {}
    /// Begins connecting the subchannel asynchronously.  If now is set, does
    /// not wait for any pending connection backoff to complete.
    pub fn connect(&mut self, now: bool) {}
}

#[async_trait]
impl Service for Subchannel {
    async fn call(&self, request: Request) -> Response {
        todo!()
    }
}
