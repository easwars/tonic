use std::any::Any;

use grpc::client::load_balancing::pick_first;
use grpc::service::{Message, MessageService, Request, Response};
use grpc::{client::ChannelOptions, inmemory};
use tonic::async_trait;

struct Handler {}

#[derive(Debug)]
struct MyReqMessage(String);
#[derive(Debug)]
struct MyResMessage(String);

impl Message for MyReqMessage {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl Message for MyResMessage {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[async_trait]
impl MessageService for Handler {
    async fn call(&self, mut request: Request<Box<dyn Message>>) -> Response<Box<dyn Message>> {
        let (res, tx) = Response::<Box<dyn Message>>::new();
        let req_msg = request
            .next()
            .await
            .unwrap()
            .as_any()
            .downcast_ref::<MyReqMessage>()
            .unwrap()
            .0
            .clone();
        tokio::task::spawn(async move {
            tx.send(Box::new(MyResMessage(format!(
                "responding to: {}; msg: {}",
                request.method, req_msg
            ))) as Box<dyn Message>)
                .await?;
            tx.send(
                Box::new(MyResMessage(format!("responding to: {}", request.method)))
                    as Box<dyn Message>,
            )
            .await
            /*tx.send(
                Box::new(MyResMessage(format!("request was {}", request.next())))
                    as Box<dyn Message>,
            )
            .await;*/
        });
        res
    }
}

#[tokio::main]
async fn main() {
    inmemory::reg();
    pick_first::reg();
    let lis = inmemory::Listener::new();
    let mut srv = grpc::server::Server::new();
    srv.set_handler(Handler {});
    let target = lis.target();
    let lis2 = lis.clone();
    tokio::task::spawn(async move {
        srv.serve(&lis2).await;
        println!("serve returned!");
    });
    println!("{target}");
    let chan = grpc::client::Channel::new(target.as_str(), None, None, ChannelOptions::default());
    let (req, tx) = Request::<Box<dyn Message>>::new("hi".to_string(), None);
    let mut res = chan.call(req).await;
    let _ = tx
        .send(Box::new(MyReqMessage("My Request".to_string())) as Box<dyn Message>)
        .await;
    while let Some(resp) = res.next().await {
        let resp_msg = resp.as_any().downcast_ref::<MyResMessage>().unwrap();
        println!("CALL RESPONSE: {:?}", resp_msg.0);
    }
    lis.close().await;
}
