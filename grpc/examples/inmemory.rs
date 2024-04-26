use grpc::client::load_balancing::pick_first;
use grpc::service::{Request, Response, Service};
use grpc::{client::ChannelOptions, inmemory};
use tonic::async_trait;

struct Handler {}

#[async_trait]
impl Service for Handler {
    async fn call(&self, request: Request) -> Response {
        Response::new_with_str(request.method)
    }
}

#[tokio::main]
async fn main() {
    inmemory::reg();
    pick_first::reg();
    let lis = inmemory::Listener::new();
    let mut srv = grpc::server::Server::new();
    srv.set_handler(Box::new(Handler {}));
    let target = lis.target();
    tokio::spawn(async move {
        srv.serve(lis).await;
    });
    println!("{target}");
    let chan = grpc::client::Channel::new(target.as_str(), None, None, ChannelOptions::default());
    let req = Request::new("hi".to_string(), None);
    let res = chan.call(req).await;
    println!("CALL RESPONSE: {:?}", res);
}
