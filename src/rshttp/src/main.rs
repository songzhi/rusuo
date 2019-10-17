use static_http_server::start_server;
use async_std::task;

fn main() {
    task::block_on(start_server("")).unwrap();
}
