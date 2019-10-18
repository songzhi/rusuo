use std::str::FromStr;

use async_std::net::SocketAddr;
use async_std::task;

use static_http_server::start_server;

fn main() {
    task::block_on(start_server(SocketAddr::from_str("127.0.0.1:8080").unwrap())).unwrap();
}
