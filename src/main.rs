mod multipart_stream_fixed;

use std::sync::Arc;
use std::time::Duration;

use actix_web::{get, App, HttpServer, Responder};
use anyhow::anyhow;
use async_event::Event;
use bytes::Bytes;
use futures_util::stream::unfold;
use futures_util::stream::StreamExt;
use futures_util::{try_join, Stream};
use mime::Mime;
use tokio::sync::RwLock;
use tokio::time::sleep;

use self::multipart_stream_fixed::parse;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let bytes = Arc::new(RwLock::const_new(None));
    try_join!(listener())?;
    Ok(())
}

async fn listener() -> anyhow::Result<()> {
    loop {
        // TODO: msg
        let _ = listener_inner().await;
        sleep(Duration::from_secs(1)).await;
    }
}

async fn listener_inner() -> anyhow::Result<()> {
    let resp = reqwest::get("https://horst.vetmed.fu-berlin.de/")
        .await?
        .error_for_status()?;
    let content_type: Mime = resp
        .headers()
        .get(http::header::CONTENT_TYPE)
        .ok_or_else(|| anyhow!("No content-type"))?
        .to_str()?
        .parse()?;
    if content_type.type_() != "multipart" || content_type.subtype() != "x-mixed-replace" {
        anyhow::bail!(r#"{content_type:?} not "multipart/x-mixed-replace""#);
    }
    let boundary = content_type
        .get_param(mime::BOUNDARY)
        .ok_or_else(|| anyhow!("No boundary"))?
        .as_str();
    let mut stream = parse(resp.bytes_stream(), boundary);
    while let Some(part) = stream.next().await {
        let part = part?;
        dbg!(part.body.len());
    }
    Ok(())
}

async fn sender() -> anyhow::Result<()> {
    HttpServer::new(|| App::new().service(send_image))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await?;
    Ok(())
}

#[get("/image.jpeg")]
async fn send_image() -> impl Responder {
    format!("Hello {name}!")
}

#[derive(Default)]
struct BytesEvent(Arc<BytesEventInner>);

#[derive(Default)]
struct BytesEventInner {
    data: RwLock<(u64, Bytes)>,
    event: Event,
}

impl BytesEvent {
    async fn listen(&self) -> impl Stream<Item = Bytes> {
        let inner = self.0.clone();
        unfold((inner, true), |(inner, first)| async move {
            inner.event.wait_until(move || )
                let guard = inner.data.read().await;
                if let Some(bytes) = &*guard {
                    let bytes = bytes.clone();
                    drop(guard);
                    return Some((bytes, (inner, false)));
                }
            }

            
            todo!()
        })
    }
}