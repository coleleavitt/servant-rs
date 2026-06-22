use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant::query::{DeepQueryEntry, DeepQueryParams, FromDeepQuery, Query, ToDeepQuery};
use servant::stream::StreamBodyError;
use servant_client::{ClientError, ClientRequest, ClientResponse, RunClient};
use servant_openapi::ToSchema;
use servant_server::response::full_body;
use servant_server::{
    AuthCheck,
    BasicAuthCheck,
    Context,
    RawRequest,
    ResourceProvider,
    RouterService,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct NewParity {
    pub(crate) label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub(crate) struct ParityItem {
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) observed: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) author: String,
    pub(crate) year: u16,
}

impl FromDeepQuery for Filter {
    fn from_deep_query(params: &DeepQueryParams) -> Result<Self, ParseError> {
        let author = params
            .first_value(&["author"])
            .ok_or_else(|| ParseError::new("missing author"))?
            .to_string();
        let year = params
            .first_value(&["year"])
            .ok_or_else(|| ParseError::new("missing year"))
            .and_then(u16::from_query_param)?;
        Ok(Filter { author, year })
    }
}

impl ToDeepQuery for Filter {
    fn to_deep_query(&self) -> DeepQueryParams {
        DeepQueryParams::new(vec![
            DeepQueryEntry::with_value(["author"], self.author.clone()),
            DeepQueryEntry::with_value(["year"], self.year.to_string()),
        ])
    }
}

#[derive(Clone)]
pub(crate) struct User;

pub(crate) async fn create_handler(
    id: u64,
    tail: Vec<String>,
    query: Query,
    filter: Filter,
    tags: Vec<String>,
    active: bool,
    limit: Option<u32>,
    note: Option<String>,
    body: NewParity,
) -> Result<ParityItem, ServerError> {
    Ok(ParityItem {
        id,
        label: body.label,
        observed: format!(
            "{}|{}|{}|{}|{}|{}|{}",
            tail.join("/"),
            query.raw().unwrap_or_default(),
            filter.author,
            filter.year,
            tags.join(","),
            active,
            note.unwrap_or_else(|| format!("limit={}", limit.unwrap_or_default())),
        ),
    })
}

pub(crate) async fn stream_handler(
    body: SourceStream<Result<u64, StreamBodyError>>,
) -> Result<String, ServerError> {
    let mut stream = body.into_inner();
    let mut sum = 0u64;
    while let Some(item) = stream.next().await {
        sum += item.map_err(|err| ServerError::err400().with_body(err.to_string()))?;
    }
    Ok(sum.to_string())
}

pub(crate) async fn headers_handler() -> Result<Headers<u32>, ServerError> {
    Ok(Headers::new(7u32).try_header("x-total-count", "42"))
}

pub(crate) async fn raw_handler(
    request: RawRequest,
) -> http::Response<servant_server::response::ResponseBody> {
    http::Response::builder()
        .status(http::StatusCode::ACCEPTED)
        .body(full_body(Bytes::from(format!(
            "raw:{}",
            request.tail().join("/")
        ))))
        .expect("raw response builds")
}

pub(crate) async fn raw_m_handler(
    request: RawRequest,
    context: Arc<Context>,
) -> Result<http::Response<servant_server::response::ResponseBody>, ServerError> {
    let prefix = context.get::<String>().map(String::as_str).unwrap_or("ctx");
    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(full_body(Bytes::from(format!(
            "{prefix}:{}",
            request.method()
        ))))
        .expect("rawm response builds"))
}

pub(crate) async fn vault_handler(ext: Arc<http::Extensions>) -> Result<String, ServerError> {
    Ok(format!("vault={}", ext.get::<&'static str>().is_some()))
}

pub(crate) async fn resource_handler(resource: u32) -> Result<u32, ServerError> {
    Ok(resource)
}

pub(crate) async fn info_handler(
    secure: bool,
    addr: Option<std::net::SocketAddr>,
    version: http::Version,
) -> Result<String, ServerError> {
    Ok(format!("{secure}/{addr:?}/{version:?}"))
}

pub(crate) async fn auth_handler(_user: User) -> Result<String, ServerError> {
    Ok("auth ok".to_string())
}

pub(crate) async fn basic_handler(_user: User) -> Result<String, ServerError> {
    Ok("basic ok".to_string())
}

pub(crate) async fn gone_handler() -> Result<NoContent, ServerError> {
    Ok(NoContent)
}

pub(crate) fn context() -> Context {
    Context::new()
        .with(String::from("rawm-context"))
        .with(ResourceProvider::new(|| 77u32))
        .with(AuthCheck::<User>::new(|headers| {
            if headers.contains_key("x-token") {
                Ok(User)
            } else {
                Err(ServerError::err403())
            }
        }))
        .with(BasicAuthCheck::new(|_data: &BasicAuthData| {
            BasicAuthResult::Authorized(User)
        }))
}

pub(crate) struct InProcess(pub(crate) RouterService);

impl RunClient for InProcess {
    fn supports_streaming_request_body(&self) -> bool {
        true
    }

    async fn run_request(&self, req: ClientRequest) -> Result<ClientResponse, ClientError> {
        let mut builder = http::Request::builder()
            .method(req.method.clone())
            .uri(req.target());
        if !req.accept.is_empty() {
            let accept = req
                .accept
                .iter()
                .map(|m| m.as_ref())
                .collect::<Vec<&str>>()
                .join(", ");
            builder = builder.header(http::header::ACCEPT, accept);
        }
        for (name, value) in req.headers.iter() {
            builder = builder.header(name, value);
        }
        let body = request_body(&mut builder, req).await?;
        let resp = self
            .0
            .handle(
                builder
                    .body(body)
                    .map_err(|err| ClientError::ConnectionError(Box::new(err)))?,
            )
            .await;
        Ok(ClientResponse {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: resp
                .into_body()
                .collect()
                .await
                .expect("response body")
                .to_bytes(),
        })
    }
}

async fn request_body(
    builder: &mut http::request::Builder,
    mut req: ClientRequest,
) -> Result<Full<Bytes>, ClientError> {
    if let Some(streaming) = req.streaming_body.take() {
        *builder = std::mem::take(builder)
            .header(http::header::CONTENT_TYPE, streaming.media_type().as_ref());
        let mut stream = streaming.take_stream()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk?);
        }
        return Ok(Full::new(Bytes::from(bytes)));
    }
    if let Some((bytes, media)) = req.body {
        *builder = std::mem::take(builder).header(http::header::CONTENT_TYPE, media.as_ref());
        Ok(Full::new(bytes))
    } else {
        Ok(Full::new(Bytes::new()))
    }
}

pub(crate) async fn text(
    svc: &RouterService,
    method: &str,
    uri: &str,
    header: Option<(&str, &str)>,
) -> (http::StatusCode, String) {
    let mut builder = http::Request::builder().method(method).uri(uri);
    if let Some((name, value)) = header {
        builder = builder.header(name, value);
    }
    let resp = svc
        .handle(
            builder
                .body(Full::new(Bytes::new()))
                .expect("request builds"),
        )
        .await;
    let status = resp.status();
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}
