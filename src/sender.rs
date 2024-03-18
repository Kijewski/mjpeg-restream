use std::fmt;
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use actix_web::{get, App, HttpResponse, HttpServer};
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::task::spawn_blocking;

use crate::image_holder;

pub async fn sender(addr: Args) -> Result<(), Error> {
    let server = HttpServer::new(|| App::new().service(index).service(send_image));
    let server = match (addr.tcp, addr.uds) {
        (Some(addr), None) => {
            let socket = spawn_blocking(|| TcpListener::bind(addr))
                .await
                .map_err(Error::JoinBlocking)?
                .map_err(Error::Tcp)?;
            server.listen(socket).map_err(Error::Tcp)?
        },
        (None, Some(addr)) => {
            let socket = spawn_blocking(|| UnixListener::bind(addr))
                .await
                .map_err(Error::JoinBlocking)?
                .map_err(Error::Uds)?;
            server.listen_uds(socket).map_err(Error::Uds)?
        },
        _ => unreachable!(),
    };
    server.run().await.map_err(Error::Run)
}

#[get("/")]
async fn index() -> HttpResponse {
    HttpResponse::TemporaryRedirect()
        .append_header((http::header::LOCATION, "/image.jpeg"))
        .content_type(mime::TEXT_PLAIN_UTF_8)
        .body("-> /image.jpeg\n")
}

#[get("/image.jpeg")]
async fn send_image() -> HttpResponse {
    HttpResponse::Ok()
        .append_header((
            http::header::CONTENT_TYPE,
            "multipart/x-mixed-replace; boundary=--frameboundary",
        ))
        .streaming(async_stream::stream! {
            let mut updates = std::pin::pin!(image_holder().stream_updates());
            while let Some(bytes) = updates.next().await {
                yield Ok::<Bytes, NoError>(bytes);
            }
        })
}

#[derive(Debug, Clone, Copy)]
enum NoError {}

impl std::error::Error for NoError {}

impl fmt::Display for NoError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

#[derive(clap::Args, Debug)]
#[group(id = "sender", required = true, multiple = false)]
pub struct Args {
    /// TCP socket address to listen on
    #[arg(long)]
    tcp: Option<String>,
    /// Unix domain socket path to bind to
    #[arg(long)]
    uds: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not start blocking thread")]
    JoinBlocking(#[source] tokio::task::JoinError),
    #[error("Could not start TCP listener")]
    Tcp(#[source] std::io::Error),
    #[error("Could not start UDS listener")]
    Uds(#[source] std::io::Error),
    #[error("Could not run server")]
    Run(#[source] std::io::Error),
}
