// https://gist.githubusercontent.com/grasevski/a7238b5f3775605253b8e73ea2a8d5f2/raw/f083c5b265cd15cff049ee8a344a75c118e7f81e/multipart_stream.rs

//! Parser for async multipart streams.
use async_stream::try_stream;
use bytes::{Buf as _, Bytes, BytesMut};
use futures_util::{Stream, StreamExt as _};
use http::header::{HeaderMap, HeaderName, HeaderValue};
use memchr::memmem::Finder;
use std::convert::TryFrom;

/// Max number of headers to parse.
const MAX_HEADER: usize = 16;

/// A single part, including its headers and body.
#[derive(Debug, Eq, PartialEq)]
pub struct Part {
    /// The http headers for this part.
    pub headers: HeaderMap,

    /// The body of the part.
    pub body: Bytes,
}

impl TryFrom<Bytes> for Part {
    type Error = anyhow::Error;

    fn try_from(mut body: Bytes) -> Result<Self, Self::Error> {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADER];
        if let httparse::Status::Complete((pos, raw)) =
            httparse::parse_headers(&body, &mut headers)?
        {
            let mut headers = HeaderMap::with_capacity(raw.len());
            for h in raw {
                headers.append(
                    HeaderName::from_bytes(h.name.as_bytes())?,
                    HeaderValue::from_bytes(h.value)?,
                );
            }
            body.advance(pos);
            Ok(Self { headers, body })
        } else {
            Err(anyhow::anyhow!("Invalid part, {} bytes", body.len()))
        }
    }
}

/// Parses a multipart stream.
pub fn parse(
    input: impl Stream<Item = Result<Bytes, impl std::error::Error + Send + Sync + 'static>> + Unpin,
    boundary: &str,
) -> impl Stream<Item = anyhow::Result<Part>> + Unpin {
    let boundary = format!("--{}\r\n", boundary.strip_prefix("--").unwrap_or(boundary));
    Box::pin(parts(input, boundary).map(|x| x?.try_into()))
}

/// Splits a multipart stream into parts.
fn parts<E: std::error::Error + Send + Sync + 'static>(
    mut input: impl Stream<Item = Result<Bytes, E>> + Unpin,
    boundary: String,
) -> impl Stream<Item = Result<Bytes, E>> {
    let (mut started, mut buf) = (false, BytesMut::new());
    try_stream! {
        let finder = Finder::new(&boundary);
        while let Some(data) = input.next().await {
            let data = data?;
            buf.extend_from_slice(&data);
            let (k, mut pos) = (buf.len() - data.len(), 0);
            let offset = if k < boundary.len() {
                0
            } else {
                k - boundary.len()
            };
            for ix in finder.find_iter(&buf[offset..]).map(|x| x + offset) {
                if started {
                    yield Bytes::copy_from_slice(&buf[pos..ix]);
                } else {
                    started = true;
                }
                pos = ix + boundary.len();
            }
            buf.advance(pos);
        }
    }
}
