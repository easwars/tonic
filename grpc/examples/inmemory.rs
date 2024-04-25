use grpc::client::load_balancing::pick_first;
use grpc::service::{Request, Service};
use grpc::{client::ChannelOptions, inmemory};

#[tokio::main]
async fn main() {
    inmemory::reg();
    pick_first::reg();
    let lis = inmemory::Listener::new();
    let srv = grpc::server::Server::new();
    let target = lis.target();
    tokio::spawn(async move {
        srv.serve(lis).await;
    });
    println!("{target}");
    let chan = grpc::client::Channel::new(target.as_str(), None, None, ChannelOptions::default());
    let req = Request::new("hi".to_string(), None);
    let res = chan.call(req).await;
    println!("{:?}", res);
}
