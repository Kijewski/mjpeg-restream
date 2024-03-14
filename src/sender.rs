use std::fmt;

use actix_web::{get, App, HttpResponse, HttpServer};
use bytes::Bytes;
use futures_util::stream::unfold;
use futures_util::StreamExt;

use crate::image_holder;

pub(crate) async fn sender() -> anyhow::Result<()> {
    HttpServer::new(|| App::new().service(index).service(send_image))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await?;
    Ok(())
}

#[get("/")]
async fn index() -> HttpResponse {
    HttpResponse::TemporaryRedirect()
        .append_header((http::header::LOCATION, "/image.jpeg"))
        .body("/image.jpeg")
}

#[get("/image.jpeg")]
async fn send_image() -> HttpResponse {
    let updates = Box::pin(image_holder().stream_updates());
    HttpResponse::Ok()
        .append_header((
            http::header::CONTENT_TYPE,
            "multipart/x-mixed-replace; boundary=--frameboundary",
        ))
        .streaming(unfold(updates, |mut updates| async move {
            let bytes = updates.next().await?;
            Some((Ok::<Bytes, NoError>(bytes), updates))
        }))
}

#[derive(Debug, Clone, Copy)]
enum NoError {}

impl std::error::Error for NoError {}

impl fmt::Display for NoError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}
