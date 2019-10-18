use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use async_std::{
    fs::File,
    io::{BufWriter, copy},
    io::Result,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    prelude::*,
    task,
};
use http::{header, HeaderValue, Response, StatusCode};
use httparse::{Request, Status};
use log::{error, info};
use smallvec::SmallVec;

pub const ALLOWED_METHODS: [&str; 3] = ["GET", "HEAD", "OPTIONS"];
pub const BUF_INIT_SIZE: usize = 2048;
pub const BUF_MAX_SIZE: usize = 8192;

async fn send_file(file: &mut File, stream: &mut TcpStream) -> Result<u64> {
    copy(file, stream).await
}

#[inline]
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


pub async fn start_server(addr: SocketAddr) -> Result<()> {
    pretty_env_logger::init();
    let listener = TcpListener::bind(addr).await?;
    let mut incoming = listener.incoming();

    while let Some(Ok(stream)) = incoming.next().await {
        spawn_and_log_error(StaticServerConnection::new(stream).serve());
    };
    Ok(())
}


pub struct StaticServerConnection {
    static_dir: PathBuf,
    stream: TcpStream,
    peer_addr: SocketAddr,
}

impl StaticServerConnection {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            peer_addr: stream.peer_addr().unwrap_or_else(|_| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))),
            stream,
            static_dir: PathBuf::from(std::env::current_dir().unwrap().join("static")),
        }
    }
    pub fn with_dir(stream: TcpStream, static_dir: &str) -> Self {
        Self {
            peer_addr: stream.peer_addr().unwrap_or_else(|_| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))),
            stream,
            static_dir: PathBuf::from(std::env::current_dir().unwrap().join(static_dir)),
        }
    }
    pub async fn serve(mut self) -> Result<()> {
        let mut buf = SmallVec::<[u8; BUF_INIT_SIZE]>::from_elem(0, BUF_INIT_SIZE);
        'rcv: loop {
            let mut rcvd = self.stream.read(buf.as_mut_slice()).await?;
            if rcvd == 0 {
                continue;
            }
            let mut is_partial = false;
            loop {
                if is_partial {
                    if rcvd == buf.len() && rcvd < BUF_MAX_SIZE / 2 {
                        buf.resize(rcvd * 2, 0);
                        rcvd += self.stream.read(&mut buf[rcvd..]).await?;
                    } else {
                        break 'rcv;
                    }
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

    pub async fn handle_request<'headers, 'buf: 'headers>(&mut self, request: &Request<'headers, 'buf>) -> Result<usize> {
        self.log_request(request);
        if !request.method.map_or(false, |m| ALLOWED_METHODS.contains(&m)) {
            // 方法不允许
            self.send_response(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .header(http::header::SERVER, http::header::HeaderValue::from_static("rshttp"))
                .body(b"Method Not Allowed".as_ref())
                .unwrap()).await
        } else {
            let mut path = self.static_dir.join(&request.path.unwrap_or("/")[1..]);
            if path.exists().await {
                if path.is_dir().await {
                    path.push("index.html");
                }
                if !path.exists().await { return self.send_404_response().await; }
                let mut file = File::open(&path).await?;
                let content_type = mime_guess::from_path(&path).first_or_octet_stream();
                let mut count = 0;
                count += self.send_response(Response::builder()
                    .status(StatusCode::OK)
                    .header(header::SERVER, header::HeaderValue::from_static("rshttp"))
                    .header(header::CONTENT_LENGTH, header::HeaderValue::from(file.metadata().await?.len()))
                    .header(header::CONTENT_TYPE, header::HeaderValue::from_str(content_type.as_ref()).unwrap())
                    .body([0u8; 0].as_ref())
                    .unwrap()).await?;
                count += send_file(&mut file, &mut self.stream).await? as usize;
                Ok(count)
            } else {
                self.send_404_response().await
            }
        }
    }
    #[inline]
    pub async fn send_404_response(&mut self) -> Result<usize> {
        self.send_response(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(http::header::SERVER, http::header::HeaderValue::from_static("rshttp"))
            .header(http::header::CONTENT_TYPE, http::header::HeaderValue::from_static("text/html"))
            .body(b"<h1>File Not Found</h1>".as_ref())
            .unwrap()).await
    }

    pub async fn send_response(&mut self, response: Response<&[u8]>) -> Result<usize> {
        self.log_response(&response);
        let mut count = 0;
        let (mut parts, body) = response.into_parts();
        let mut stream = BufWriter::with_capacity(4096, &self.stream);
        count += stream.write(b"HTTP/1.1 ").await?;
        count += stream.write(parts.status.as_str().as_bytes()).await?;
        if let Some(reason) = parts.status.canonical_reason() {
            count += stream.write(b" ").await?;
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
    #[inline]
    pub fn log_request(&self, request: &Request) {
        info!("{:?} - \"{} {}\"", self.peer_addr, request.method.unwrap_or_default(), request.path.unwrap_or_default());
    }
    #[inline]
    pub fn log_response(&self, response: &Response<&[u8]>) {
        info!("{}", response.status());
    }
}



