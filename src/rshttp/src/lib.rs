use async_std::{
    io::copy,
    net::{TcpStream, TcpListener},
    fs::File,
    io::Result,
    prelude::*,
    task,
};
use httparse::{Request, Status};
use smallvec::SmallVec;
use http::{Response, response::Parts, StatusCode, HeaderValue, header};
use async_std::io::BufWriter;
use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};
use log::{error, info};
use std::path::{Path, PathBuf};

pub const ALLOWED_METHODS: [&str; 3] = ["GET", "HEAD", "OPTIONS"];
pub const BUF_INIT_SIZE: usize = 512;
pub const BUF_MAX_SIZE: usize = 8192;

async fn send_file(file: &mut File, stream: &mut TcpStream) -> Result<u64> {
    copy(file, stream).await
}

fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()>
    where
        F: Future<Output=Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = fut.await {
            error!("{}", e)
        }
    })
}


pub async fn start_server(static_dir: &str) -> Result<()> {
    pretty_env_logger::init();
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    let mut incoming = listener.incoming();

    while let Some(Ok(stream)) = incoming.next().await {
        spawn_and_log_error(StaticServerConnection::new(stream).serve());
    };
    Ok(())
}


pub struct StaticServerConnection {
    static_dir: PathBuf,
    stream: TcpStream,
}

impl StaticServerConnection {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            static_dir: std::env::current_dir().unwrap().join("static"),
        }
    }
    pub fn with_dir(stream: TcpStream, static_dir: &str) -> Self {
        Self {
            stream,
            static_dir: std::env::current_dir().unwrap().join(static_dir),
        }
    }
    pub async fn serve(mut self) -> Result<()> {
        let mut buf = SmallVec::<[u8; 1024]>::from_elem(0, 1024);
        'rcv: loop {
            let mut rcvd = self.stream.read(buf.as_mut_slice()).await?;
            if rcvd == 0 {
                continue;
            }
            let mut is_partial = false;
            loop {
                if is_partial && rcvd == buf.len() && rcvd < BUF_MAX_SIZE / 2 {
                    buf.resize(rcvd * 2, 0);
                    rcvd += self.stream.read(&mut buf[rcvd..]).await?;
                }
                let mut headers = [httparse::EMPTY_HEADER; 32];
                let mut request = Request::new(&mut headers);
                match request.parse(buf.as_slice()) {
                    Ok(Status::Complete(_)) => {
                        self.handle_request(&request).await?;
                        break;
                    }
                    Ok(Status::Partial) => {
                        is_partial = true;
                        continue;
                    }
                    Err(_) => {
                        break 'rcv;
                    }
                };
            };
        }

        Ok(())
    }

    pub async fn handle_request<'headers, 'buf: 'headers>(&mut self, request: &Request<'headers, 'buf>) -> Result<()> {
        let response = if !request.method.map_or(false, |m| ALLOWED_METHODS.contains(&m)) {
            // 方法不允许
            Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header(http::header::SERVER, http::header::HeaderValue::from_static("rshttp"))
                .body(b"Method Not Allowed".as_ref())
                .unwrap()
        } else {
            Response::builder()
                .status(StatusCode::OK)
                .header(http::header::SERVER, http::header::HeaderValue::from_static("rshttp"))
                .header(http::header::CONTENT_TYPE, http::header::HeaderValue::from_static("text/html"))
                .body(b"<h1>hello</h1>".as_ref())
                .unwrap()
        };
        self.log_handled(request, &response);
        self.send_response(response).await?;
        Ok(())
    }

    pub async fn send_response(&mut self, response: Response<&[u8]>) -> Result<usize> {
        let mut count = 0;
        let (mut parts, body) = response.into_parts();
        let mut stream = BufWriter::with_capacity(4096, &self.stream);
        count += stream.write(b"HTTP/1.1 ").await?;
        count += stream.write(parts.status.as_str().as_bytes()).await?;
        if let Some(reason) = parts.status.canonical_reason() {
            count += stream.write(reason.as_bytes()).await?;
        }
        count += stream.write(b"\r\n").await?;
        if parts.headers.get(header::CONTENT_LENGTH).is_none() {
            assert_ne!(body.len(), 0);
            parts.headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
        }
        for (name, val) in parts.headers {
            if let Some(name) = name {
                count += stream.write(name.as_ref()).await?;
                count += stream.write(b": ").await?;
            }
            count += stream.write(val.as_bytes()).await?;
            count += stream.write(b"\r\n").await?;
        };
        count += stream.write(b"\r\n").await?;
        count += stream.write(body).await?;
        stream.flush().await?;
        Ok(count)
    }

    pub fn log_handled(&self, request: &Request, response: &Response<&[u8]>) {
        let addr = self.stream.peer_addr().unwrap_or_else(|_| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)));
        info!("{:?} - \"{} {}\" {}", addr, request.method.unwrap_or_default(), request.path.unwrap_or_default(), response.status());
    }
}



