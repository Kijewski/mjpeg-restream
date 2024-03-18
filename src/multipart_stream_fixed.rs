// https://github.com/scottlamb/multipart-stream-rs/commit/0645358b094217867d306b995fc81e517631cb70
// + https://github.com/scottlamb/multipart-stream-rs/pull/3/commits/929cc8035bb484db2a65bc5c1fb16a966d085986
// + my changes

// Copyright (C) 2021 Scott Lamb <slamb@slamb.org>
// SPDX-License-Identifier: MIT OR Apache-2.0

// This implementation is gross (it's hard to read and copies when not
// necessary), I think due to a combination of the following:
//
// 1.  the current state of Rust async: in particular, that there are no coroutines.
// 2.  my inexperience with Rust async
// 3.  how quickly I threw this together.
//
// Fortunately the badness is hidden behind a decent interface, and there are decent tests
// of success cases with partial data. In the situations we're using it (small
// bits of metadata rather than video), the inefficient probably doesn't matter.
// TODO: add tests of bad inputs.

//! Parses a [`Bytes`] stream into a [`Part`] stream.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures_util::Stream;
use http::header::{self, HeaderMap, HeaderName, HeaderValue};
use multipart_stream::Part;
use pin_project::pin_project;

/// An error when reading from the underlying stream or parsing.
///
/// When the error comes from the underlying stream, it can be examined via
/// [`std::error::Error::source`].
#[derive(Debug)]
pub struct Error(ErrorInt);

#[derive(Debug)]
enum ErrorInt {
    ParseError(String),
    Underlying(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            ErrorInt::ParseError(ref s) => f.pad(s),
            ErrorInt::Underlying(ref e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0 {
            ErrorInt::Underlying(e) => Some(&**e),
            _ => None,
        }
    }
}

/// Creates a parse error with the specified format string and arguments.
macro_rules! parse_err {
    ($($arg:tt)*) => {
        Error(ErrorInt::ParseError(format!($($arg)*)))
    };
}

/// A parsing stream adapter, constructed via [`parse`] or [`ParserBuilder`].
#[pin_project]
pub struct Parser<S, E>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    #[pin]
    input: S,

    /// The boundary with `--` prefix and `\r\n` suffix.
    boundary: Vec<u8>,
    buf: BytesMut,
    state: State,
    max_header_bytes: usize,
    max_body_bytes: usize,
}

enum State {
    /// Consuming 0 or more `\r\n` pairs, advancing when encountering a byte that doesn't fit that pattern.
    Newlines,

    /// Waiting for the completion of a boundary.
    /// `pos` is the current offset within `boundary_buf`.
    Boundary { pos: usize },

    /// Waiting for a full set of headers.
    Headers,

    /// Waiting for a full body.
    Body {
        headers: HeaderMap,
        body_len: Option<usize>,
    },

    /// The stream is finished (has already returned an error).
    Done,
}

impl State {
    /// Processes the current buffer contents.
    ///
    /// This reverses the order of the return value so it can return error via `?` and `bail!`.
    /// The caller puts it back into the order expected by `Stream`.
    fn process(
        &mut self,
        boundary: &[u8],
        buf: &mut BytesMut,
        max_header_bytes: usize,
        max_body_bytes: usize,
    ) -> Result<Poll<Option<Part>>, Error> {
        'outer: loop {
            match self {
                State::Newlines => {
                    while buf.len() >= 2 {
                        if &buf[0..2] == b"\r\n" {
                            buf.advance(2);
                        } else {
                            *self = Self::Boundary { pos: 0 };
                            continue 'outer;
                        }
                    }
                    if buf.len() == 1 && buf[0] != b'\r' {
                        *self = Self::Boundary { pos: 0 };
                    } else {
                        return Ok(Poll::Pending);
                    }
                },
                State::Boundary { ref mut pos } => {
                    let len = std::cmp::min(boundary.len() - *pos, buf.len());
                    if buf[0..len] != boundary[*pos..*pos + len] {
                        return Err(parse_err!("bad boundary"));
                    }
                    buf.advance(len);
                    *pos += len;
                    if *pos < boundary.len() {
                        return Ok(Poll::Pending);
                    }
                    *self = State::Headers;
                },
                State::Headers => {
                    let mut raw = [httparse::EMPTY_HEADER; 16];
                    let headers = httparse::parse_headers(buf, &mut raw)
                        .map_err(|e| parse_err!("Part headers invalid: {}", e))?;
                    match headers {
                        httparse::Status::Complete((body_pos, raw)) => {
                            let mut headers = HeaderMap::with_capacity(raw.len());
                            for h in raw {
                                let _ = headers.append(
                                    HeaderName::from_bytes(h.name.as_bytes())
                                        .map_err(|_| parse_err!("bad header name"))?,
                                    HeaderValue::from_bytes(h.value)
                                        .map_err(|_| parse_err!("bad header value"))?,
                                );
                            }
                            buf.advance(body_pos);
                            let body_len = headers
                                .get(header::CONTENT_LENGTH)
                                .map(|v| v.to_str())
                                .transpose()
                                .map_err(|_| parse_err!("Part Content-Length is not valid string"))?
                                .map(|v| v.parse())
                                .transpose()
                                .map_err(|_| {
                                    parse_err!("Part Content-Length is not valid usize")
                                })?;
                            if let Some(body_len) = body_len {
                                if body_len > max_body_bytes {
                                    return Err(parse_err!(
                                        "body byte length {} exceeds maximum of {}",
                                        body_len,
                                        max_body_bytes
                                    ));
                                }
                            }
                            *self = State::Body { headers, body_len };
                        },
                        httparse::Status::Partial => {
                            if buf.len() >= max_header_bytes {
                                return Err(parse_err!(
                                    "incomplete {}-byte header, vs maximum of {} bytes",
                                    buf.len(),
                                    max_header_bytes
                                ));
                            }
                            return Ok(Poll::Pending);
                        },
                    }
                },
                State::Body {
                    headers,
                    ref mut body_len,
                } => {
                    if body_len.is_none() {
                        if let Some(n) = memchr::memmem::find(buf, boundary) {
                            *body_len = Some(n);
                        } else if buf.len() > max_body_bytes {
                            return Err(parse_err!(
                                "body byte length {} exceeds maximum of {}",
                                buf.len(),
                                max_body_bytes
                            ));
                        }
                    }
                    if let Some(body_len) = body_len {
                        if buf.len() >= *body_len && *body_len > 0 {
                            let body = buf.split_to(*body_len).freeze();
                            let headers = std::mem::replace(headers, HeaderMap::new());
                            *self = State::Newlines;
                            return Ok(Poll::Ready(Some(Part { headers, body })));
                        }
                    }
                    return Ok(Poll::Pending);
                },
                State::Done => return Ok(Poll::Ready(None)),
            }
        }
    }
}

/// Flexible builder for [`Parser`].
pub struct ParserBuilder {
    max_header_bytes: usize,
    max_body_bytes: usize,
}

impl ParserBuilder {
    pub fn new() -> Self {
        ParserBuilder {
            max_header_bytes: usize::MAX,
            max_body_bytes: usize::MAX,
        }
    }

    /// Parses a [`Bytes`] stream into a [`Part`] stream.
    ///
    /// `boundary` should be as in the `boundary` parameter of the `Content-Type` header.
    pub fn parse<S, E>(self, input: S, boundary: &str) -> impl Stream<Item = Result<Part, Error>>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let boundary = {
            let mut line = Vec::with_capacity(boundary.len() + 4);
            if !boundary.starts_with("--") {
                line.extend_from_slice(b"--");
            }
            line.extend_from_slice(boundary.as_bytes());
            line.extend_from_slice(b"\r\n");
            line
        };

        Parser {
            input,
            buf: BytesMut::new(),
            boundary,
            state: State::Newlines,
            max_header_bytes: self.max_header_bytes,
            max_body_bytes: self.max_body_bytes,
        }
    }
}

/// Parses a [`Bytes`] stream into a [`Part`] stream.
///
/// `boundary` should be as in the `boundary` parameter of the `Content-Type` header.
///
/// This doesn't allow customizing the parser; use [`ParserBuilder`] instead if desired.
pub fn parse<S, E>(input: S, boundary: &str) -> impl Stream<Item = Result<Part, Error>>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    ParserBuilder::new().parse(input, boundary)
}

impl<S, E> Stream for Parser<S, E>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Item = Result<Part, Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        loop {
            match this.state.process(
                this.boundary,
                this.buf,
                *this.max_header_bytes,
                *this.max_body_bytes,
            ) {
                Err(e) => {
                    *this.state = State::Done;
                    return Poll::Ready(Some(Err(e)));
                },
                Ok(Poll::Ready(Some(r))) => return Poll::Ready(Some(Ok(r))),
                Ok(Poll::Ready(None)) => return Poll::Ready(None),
                Ok(Poll::Pending) => {},
            }
            match this.input.as_mut().poll_next(ctx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    if !matches!(*this.state, State::Newlines) {
                        *this.state = State::Done;
                        return Poll::Ready(Some(Err(parse_err!("unexpected mid-part EOF"))));
                    }
                    return Poll::Ready(None);
                },
                Poll::Ready(Some(Err(e))) => {
                    *this.state = State::Done;
                    return Poll::Ready(Some(Err(Error(ErrorInt::Underlying(e.into())))));
                },
                Poll::Ready(Some(Ok(b))) => {
                    this.buf.extend_from_slice(&b);
                },
            };
        }
    }
}
