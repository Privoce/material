use material_rs::router;
use salvo::{Listener, Router, Server, conn::TcpListener};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    // Bind server to port 5800
    let acceptor = TcpListener::new("127.0.0.1:5800").bind().await;

    // Start serving requests
    Server::new(acceptor).serve(router::build()).await;
}


