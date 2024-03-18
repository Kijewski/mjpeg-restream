use std::time::Duration;

use anyhow::anyhow;
use futures_util::StreamExt;
use mime::Mime;
use reqwest::Url;
use tokio::time::sleep;

use crate::image_holder;
use crate::multipart_stream_fixed::parse;

pub async fn listener(args: Args) -> Result<(), Error> {
    loop {
        let err = listener_inner(&args).await;
        // TODO: msg
        let _ = dbg!(err);
        sleep(Duration::from_secs(5)).await;
    }
}

async fn listener_inner(args: &Args) -> anyhow::Result<()> {
    let resp = reqwest::get(args.url.clone()).await?.error_for_status()?;
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
        let body = part?.body;

        let head = format!(
            "\
            Content-Length: {}\r\n\
            Content-Type: image/jpeg\r\n\
            \r\n",
            body.len(),
        );
        let trailer = "--frameboundary\r\n";
        let mut data = Vec::<u8>::with_capacity(head.len() + body.len() + trailer.len());
        data.extend(head.as_bytes());
        data.extend(body);
        data.extend(trailer.as_bytes());

        image_holder().update(data.into()).await;
    }
    Ok(())
}

#[derive(clap::Args, Debug)]
#[command(id = "listener")]
pub struct Args {
    /// URL to restream
    #[arg(long)]
    url: Url,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {}
