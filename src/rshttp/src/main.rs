use async_std::task;

use static_http_server::start_server;

fn main() {
    task::block_on(start_server()).unwrap();
}
