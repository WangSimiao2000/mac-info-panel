//! Security boundary for host-mediated HTTP requests.
//!
//! The client validates a request before resolving it, validates every resolved
//! address before connecting, pins the transport to those addresses, and repeats
//! the entire process for each redirect hop. Plugins never receive this client
//! directly.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http::header::{AUTHORIZATION, COOKIE, HOST, LOCATION, PROXY_AUTHORIZATION};
use http::{HeaderMap, Method, StatusCode};
use thiserror::Error;
use url::{Host, Url};

pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AddressClass {
    Loopback,
    LinkLocal,
    Private,
    Multicast,
    Unspecified,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkPermission {
    origin: Origin,
    allowed_address_classes: BTreeSet<AddressClass>,
}

impl NetworkPermission {
    pub fn parse(origin: &str) -> Result<Self, PermissionError> {
        Ok(Self {
            origin: Origin::parse(origin)?,
            allowed_address_classes: BTreeSet::new(),
        })
    }

    #[must_use]
    pub fn allow_address_class(mut self, class: AddressClass) -> Self {
        self.allowed_address_classes.insert(class);
        self
    }

    fn allows_url(&self, url: &Url) -> bool {
        Origin::from_url(url).is_some_and(|origin| self.origin == origin)
    }

    fn allows_address(&self, address: IpAddr) -> bool {
        classify_address(address).is_none_or(|class| self.allowed_address_classes.contains(&class))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl Origin {
    fn parse(value: &str) -> Result<Self, PermissionError> {
        let url = Url::parse(value).map_err(|_| PermissionError::MalformedOrigin)?;
        reject_credentials(&url).map_err(|_| PermissionError::CredentialsNotAllowed)?;
        validate_scheme_and_host(&url).map_err(PermissionError::UrlPolicy)?;
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(PermissionError::OriginMustNotContainResource);
        }
        Self::from_url(&url).ok_or(PermissionError::MalformedOrigin)
    }

    fn from_url(url: &Url) -> Option<Self> {
        Some(Self {
            scheme: url.scheme().to_owned(),
            host: normalized_host(url)?,
            port: url.port_or_known_default()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpTimeouts {
    pub connect: Duration,
    pub request: Duration,
    pub resource: Duration,
}

impl Default for HttpTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            request: Duration::from_secs(15),
            resource: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestLimits {
    pub max_response_bytes: usize,
    pub max_redirect_hops: usize,
    pub max_concurrent_requests_per_plugin: usize,
    pub max_requests_per_window: usize,
    pub request_rate_window: Duration,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            max_response_bytes: 1024 * 1024,
            max_redirect_hops: 5,
            max_concurrent_requests_per_plugin: 4,
            max_requests_per_window: 60,
            request_rate_window: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedirectPolicy {
    pub max_hops: usize,
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub redirects: Option<RedirectPolicy>,
}

impl HttpRequest {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            url: url.into(),
            headers: HeaderMap::new(),
            body: Bytes::new(),
            redirects: None,
        }
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

pub trait Resolver: Send + Sync {
    fn resolve(
        &self,
        host: &str,
        port: u16,
    ) -> impl Future<Output = Result<Vec<SocketAddr>, ResolveError>> + Send;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, ResolveError> {
        tokio::net::lookup_host((host, port))
            .await
            .map(|addresses| addresses.collect())
            .map_err(|_| ResolveError)
    }
}

pub trait Transport: Send + Sync {
    fn send(
        &self,
        request: TransportRequest,
    ) -> impl Future<Output = Result<TransportResponse, TransportError>> + Send;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReqwestTransport;

impl Transport for ReqwestTransport {
    async fn send(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(request.connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();

        if matches!(request.url.host(), Some(Host::Domain(_))) {
            let host = request.url.host_str().ok_or(TransportError::Request)?;
            builder = builder.resolve_to_addrs(host, &request.resolved_addresses);
        }

        let client = builder.build().map_err(|_| TransportError::Request)?;
        let response = client
            .request(request.method, request.url)
            .headers(request.headers)
            .body(request.body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|_| TransportError::Body));
        Ok(TransportResponse {
            status,
            headers,
            body: Box::pin(body),
        })
    }
}

fn map_reqwest_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() && error.is_connect() {
        TransportError::ConnectTimeout
    } else if error.is_connect() {
        TransportError::Connect
    } else if error.is_timeout() {
        TransportError::RequestTimeout
    } else {
        TransportError::Request
    }
}

pub struct TransportRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub resolved_addresses: Vec<SocketAddr>,
    pub connect_timeout: Duration,
}

pub struct TransportResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: BodyStream,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Duration;
}

#[derive(Clone, Debug)]
pub struct SystemClock {
    started_at: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.started_at.elapsed()
    }
}

pub struct HttpClient<R = SystemResolver, T = ReqwestTransport, C = SystemClock> {
    resolver: R,
    transport: T,
    clock: C,
    timeouts: HttpTimeouts,
    limits: RequestLimits,
    plugin_states: Arc<Mutex<HashMap<String, PluginState>>>,
}

impl HttpClient<SystemResolver, ReqwestTransport, SystemClock> {
    pub fn system(timeouts: HttpTimeouts, limits: RequestLimits) -> Result<Self, ConfigError> {
        Self::new(
            SystemResolver,
            ReqwestTransport,
            SystemClock::default(),
            timeouts,
            limits,
        )
    }
}

impl<R, T, C> HttpClient<R, T, C>
where
    R: Resolver,
    T: Transport,
    C: Clock,
{
    pub fn new(
        resolver: R,
        transport: T,
        clock: C,
        timeouts: HttpTimeouts,
        limits: RequestLimits,
    ) -> Result<Self, ConfigError> {
        validate_config(timeouts, limits)?;
        Ok(Self {
            resolver,
            transport,
            clock,
            timeouts,
            limits,
            plugin_states: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn execute(
        &self,
        plugin_id: &str,
        permissions: &[NetworkPermission],
        request: HttpRequest,
    ) -> Result<HttpResponse, HttpError> {
        let _permit = self.acquire_plugin_permit(plugin_id)?;
        tokio::time::timeout(
            self.timeouts.resource,
            self.execute_within_resource_deadline(plugin_id, permissions, request),
        )
        .await
        .map_err(|_| HttpError::Timeout(TimeoutPhase::Resource))?
    }

    async fn execute_within_resource_deadline(
        &self,
        plugin_id: &str,
        permissions: &[NetworkPermission],
        request: HttpRequest,
    ) -> Result<HttpResponse, HttpError> {
        reject_forbidden_headers(&request.headers)?;
        let mut url = parse_request_url(&request.url)?;
        let mut method = request.method;
        let mut headers = request.headers;
        let mut body = request.body;
        let redirect_hop_limit = request
            .redirects
            .map(|policy| policy.max_hops.min(self.limits.max_redirect_hops));
        let mut followed_hops = 0usize;

        loop {
            let permission = permission_for(permissions, &url)?;
            self.consume_plugin_request_rate(plugin_id)?;
            let response = tokio::time::timeout(self.timeouts.request, async {
                let addresses = self.resolve_and_validate(&url, permission).await?;
                let transport_request = TransportRequest {
                    method: method.clone(),
                    url: url.clone(),
                    headers: headers.clone(),
                    body: body.clone(),
                    resolved_addresses: addresses,
                    connect_timeout: self.timeouts.connect,
                };
                self.transport
                    .send(transport_request)
                    .await
                    .map_err(HttpError::from)
            })
            .await
            .map_err(|_| HttpError::Timeout(TimeoutPhase::Request))??;

            if !is_followable_redirect(response.status) {
                return self.finish_response(response).await;
            }
            let Some(max_hops) = redirect_hop_limit else {
                return self.finish_response(response).await;
            };
            if followed_hops >= max_hops {
                return Err(HttpError::TooManyRedirects);
            }
            let location = response
                .headers
                .get(LOCATION)
                .ok_or(HttpError::InvalidRedirect)?
                .to_str()
                .map_err(|_| HttpError::InvalidRedirect)?;
            let next = resolve_redirect(&url, location)?;
            let _ = permission_for(permissions, &next)?;
            redirect_method_and_body(response.status, &mut method, &mut body);
            headers.remove(AUTHORIZATION);
            headers.remove(PROXY_AUTHORIZATION);
            url = next;
            followed_hops += 1;
        }
    }

    async fn resolve_and_validate(
        &self,
        url: &Url,
        permission: &NetworkPermission,
    ) -> Result<Vec<SocketAddr>, HttpError> {
        let host = url.host().ok_or(HttpError::MissingHost)?;
        let port = url.port_or_known_default().ok_or(HttpError::MissingPort)?;
        let addresses = match host {
            Host::Ipv4(address) => vec![SocketAddr::new(IpAddr::V4(address), port)],
            Host::Ipv6(address) => vec![SocketAddr::new(IpAddr::V6(address), port)],
            Host::Domain(host) => self.resolver.resolve(host, port).await?,
        };
        if addresses.is_empty() {
            return Err(HttpError::ResolutionFailed);
        }
        if url.scheme() == "http"
            && addresses
                .iter()
                .any(|address| classify_address(address.ip()) != Some(AddressClass::Loopback))
        {
            return Err(HttpError::HttpDestinationNotLoopback);
        }
        let mut checked = Vec::with_capacity(addresses.len());
        for mut address in addresses {
            address.set_port(port);
            if !permission.allows_address(address.ip()) {
                return Err(HttpError::ForbiddenAddress(
                    classify_address(address.ip()).expect("denied addresses have a class"),
                ));
            }
            checked.push(address);
        }
        checked.sort_unstable();
        checked.dedup();
        Ok(checked)
    }

    async fn finish_response(
        &self,
        response: TransportResponse,
    ) -> Result<HttpResponse, HttpError> {
        let body = collect_limited(response.body, self.limits.max_response_bytes).await?;
        Ok(HttpResponse {
            status: response.status,
            headers: response.headers,
            body,
        })
    }

    fn acquire_plugin_permit(&self, plugin_id: &str) -> Result<PluginPermit, HttpError> {
        let mut states = self
            .plugin_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = states.entry(plugin_id.to_owned()).or_default();
        if state.in_flight >= self.limits.max_concurrent_requests_per_plugin {
            return Err(HttpError::ConcurrencyLimit);
        }
        state.in_flight += 1;
        Ok(PluginPermit {
            plugin_id: plugin_id.to_owned(),
            states: Arc::clone(&self.plugin_states),
        })
    }

    fn consume_plugin_request_rate(&self, plugin_id: &str) -> Result<(), HttpError> {
        let now = self.clock.now();
        let mut states = self
            .plugin_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = states.entry(plugin_id.to_owned()).or_default();
        while state
            .requests
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= self.limits.request_rate_window)
        {
            state.requests.pop_front();
        }
        if state.requests.len() >= self.limits.max_requests_per_window {
            let retry_after = state
                .requests
                .front()
                .map(|at| {
                    self.limits
                        .request_rate_window
                        .saturating_sub(now.saturating_sub(*at))
                })
                .unwrap_or_default();
            return Err(HttpError::RateLimit { retry_after });
        }
        state.requests.push_back(now);
        Ok(())
    }
}

#[derive(Default)]
struct PluginState {
    in_flight: usize,
    requests: VecDeque<Duration>,
}

struct PluginPermit {
    plugin_id: String,
    states: Arc<Mutex<HashMap<String, PluginState>>>,
}

impl Drop for PluginPermit {
    fn drop(&mut self) {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = states.get_mut(&self.plugin_id) {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
    }
}

async fn collect_limited(
    mut body: BodyStream,
    max_response_bytes: usize,
) -> Result<Bytes, HttpError> {
    let mut collected = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        if chunk.len() > max_response_bytes.saturating_sub(collected.len()) {
            return Err(HttpError::ResponseTooLarge {
                limit: max_response_bytes,
            });
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(collected))
}

fn parse_request_url(value: &str) -> Result<Url, HttpError> {
    let url = Url::parse(value).map_err(|_| HttpError::MalformedUrl)?;
    reject_credentials(&url)?;
    validate_scheme_and_host(&url)?;
    if url.fragment().is_some() {
        return Err(HttpError::FragmentNotAllowed);
    }
    Ok(url)
}

fn validate_scheme_and_host(url: &Url) -> Result<(), UrlPolicyError> {
    let host = url.host().ok_or(UrlPolicyError::MissingHost)?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if structurally_loopback(host) => Ok(()),
        "http" => Err(UrlPolicyError::PlaintextRemote),
        _ => Err(UrlPolicyError::UnsupportedScheme),
    }
}

fn structurally_loopback(host: Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    }
}

fn reject_credentials(url: &Url) -> Result<(), HttpError> {
    if url.username().is_empty() && url.password().is_none() {
        Ok(())
    } else {
        Err(HttpError::CredentialsNotAllowed)
    }
}

fn normalized_host(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(host) => Some(host.to_ascii_lowercase()),
        Host::Ipv4(address) => Some(address.to_string()),
        Host::Ipv6(address) => Some(address.to_string()),
    }
}

fn permission_for<'a>(
    permissions: &'a [NetworkPermission],
    url: &Url,
) -> Result<&'a NetworkPermission, HttpError> {
    permissions
        .iter()
        .find(|permission| permission.allows_url(url))
        .ok_or(HttpError::OriginNotAllowed)
}

fn reject_forbidden_headers(headers: &HeaderMap) -> Result<(), HttpError> {
    if headers.contains_key(COOKIE) {
        Err(HttpError::CookieHeaderNotAllowed)
    } else if headers.contains_key(HOST) {
        Err(HttpError::HostHeaderNotAllowed)
    } else {
        Ok(())
    }
}

fn is_followable_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn resolve_redirect(base: &Url, location: &str) -> Result<Url, HttpError> {
    let next = base
        .join(location)
        .map_err(|_| HttpError::InvalidRedirect)?;
    reject_credentials(&next)?;
    validate_scheme_and_host(&next)?;
    if next.fragment().is_some() {
        return Err(HttpError::FragmentNotAllowed);
    }
    Ok(next)
}

fn redirect_method_and_body(status: StatusCode, method: &mut Method, body: &mut Bytes) {
    let switch_to_get = status == StatusCode::SEE_OTHER
        || ((status == StatusCode::MOVED_PERMANENTLY || status == StatusCode::FOUND)
            && *method == Method::POST);
    if switch_to_get && *method != Method::HEAD {
        *method = Method::GET;
        *body = Bytes::new();
    }
}

#[must_use]
pub fn classify_address(address: IpAddr) -> Option<AddressClass> {
    match address {
        IpAddr::V4(address) => classify_ipv4(address),
        IpAddr::V6(address) => classify_ipv6(address),
    }
}

fn classify_ipv4(address: Ipv4Addr) -> Option<AddressClass> {
    if address.is_unspecified() {
        Some(AddressClass::Unspecified)
    } else if address.is_loopback() {
        Some(AddressClass::Loopback)
    } else if address.is_link_local() {
        Some(AddressClass::LinkLocal)
    } else if address.is_private() {
        Some(AddressClass::Private)
    } else if address.is_multicast() {
        Some(AddressClass::Multicast)
    } else if is_reserved_ipv4(address) {
        Some(AddressClass::Reserved)
    } else {
        None
    }
}

fn is_reserved_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    first == 0
        || first >= 240
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 192 && second == 0 && third == 2)
        || (first == 192 && second == 88 && third == 99)
        || (first == 198 && matches!(second, 18 | 19))
        || (first == 198 && second == 51 && third == 100)
        || (first == 203 && second == 0 && third == 113)
}

fn classify_ipv6(address: Ipv6Addr) -> Option<AddressClass> {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return classify_ipv4(mapped);
    }
    if address.is_unspecified() {
        Some(AddressClass::Unspecified)
    } else if address.is_loopback() {
        Some(AddressClass::Loopback)
    } else if address.is_unicast_link_local() {
        Some(AddressClass::LinkLocal)
    } else if address.is_unique_local() || is_site_local_ipv6(address) {
        Some(AddressClass::Private)
    } else if address.is_multicast() {
        Some(AddressClass::Multicast)
    } else if is_reserved_ipv6(address) {
        Some(AddressClass::Reserved)
    } else {
        None
    }
}

fn is_site_local_ipv6(address: Ipv6Addr) -> bool {
    address.segments()[0] & 0xffc0 == 0xfec0
}

fn is_reserved_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let is_allocated_global_unicast = segments[0] & 0xe000 == 0x2000;
    let is_ietf_special = segments[0] == 0x2001 && segments[1] & 0xfe00 == 0;
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let is_deprecated_6to4 = segments[0] == 0x2002;
    let is_extended_documentation = segments[0] == 0x3fff && segments[1] & 0xf000 == 0;

    !is_allocated_global_unicast
        || is_ietf_special
        || is_documentation
        || is_deprecated_6to4
        || is_extended_documentation
}

fn validate_config(timeouts: HttpTimeouts, limits: RequestLimits) -> Result<(), ConfigError> {
    if timeouts.connect.is_zero() || timeouts.request.is_zero() || timeouts.resource.is_zero() {
        return Err(ConfigError::ZeroTimeout);
    }
    if limits.max_response_bytes == 0
        || limits.max_redirect_hops == 0
        || limits.max_concurrent_requests_per_plugin == 0
        || limits.max_requests_per_window == 0
        || limits.request_rate_window.is_zero()
    {
        return Err(ConfigError::ZeroLimit);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("name resolution failed")]
pub struct ResolveError;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    #[error("connection timed out")]
    ConnectTimeout,
    #[error("connection failed")]
    Connect,
    #[error("request timed out")]
    RequestTimeout,
    #[error("request failed")]
    Request,
    #[error("response body failed")]
    Body,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum UrlPolicyError {
    #[error("URL has no host")]
    MissingHost,
    #[error("URL scheme is unsupported")]
    UnsupportedScheme,
    #[error("plain HTTP is restricted to structurally verified loopback hosts")]
    PlaintextRemote,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PermissionError {
    #[error("network origin is malformed")]
    MalformedOrigin,
    #[error("network origin contains credentials")]
    CredentialsNotAllowed,
    #[error("network origin must not contain a path, query, or fragment")]
    OriginMustNotContainResource,
    #[error(transparent)]
    UrlPolicy(UrlPolicyError),
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("HTTP timeout values must be non-zero")]
    ZeroTimeout,
    #[error("HTTP resource and rate limits must be non-zero")]
    ZeroLimit,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum TimeoutPhase {
    #[error("connect")]
    Connect,
    #[error("request")]
    Request,
    #[error("resource")]
    Resource,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HttpError {
    #[error("URL is malformed")]
    MalformedUrl,
    #[error("URL credentials are not allowed")]
    CredentialsNotAllowed,
    #[error(transparent)]
    UrlPolicy(#[from] UrlPolicyError),
    #[error("URL fragments are not allowed")]
    FragmentNotAllowed,
    #[error("URL has no host")]
    MissingHost,
    #[error("URL has no known port")]
    MissingPort,
    #[error("origin is not declared by the plugin")]
    OriginNotAllowed,
    #[error("plain HTTP resolved to a non-loopback destination")]
    HttpDestinationNotLoopback,
    #[error("resolved destination uses forbidden {0:?} addressing")]
    ForbiddenAddress(AddressClass),
    #[error("name resolution returned no usable destination")]
    ResolutionFailed,
    #[error("Cookie request headers are disabled")]
    CookieHeaderNotAllowed,
    #[error("Host request headers cannot override the validated origin")]
    HostHeaderNotAllowed,
    #[error("redirect location is invalid")]
    InvalidRedirect,
    #[error("redirect hop limit exceeded")]
    TooManyRedirects,
    #[error("plugin HTTP concurrency limit exceeded")]
    ConcurrencyLimit,
    #[error("plugin HTTP request-rate limit exceeded; retry after {retry_after:?}")]
    RateLimit { retry_after: Duration },
    #[error("response exceeds the {limit}-byte limit")]
    ResponseTooLarge { limit: usize },
    #[error("HTTP {0} timeout")]
    Timeout(TimeoutPhase),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Transport(TransportError),
}

impl From<TransportError> for HttpError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::ConnectTimeout => Self::Timeout(TimeoutPhase::Connect),
            TransportError::RequestTimeout => Self::Timeout(TimeoutPhase::Request),
            other => Self::Transport(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures_util::stream;
    use http::HeaderValue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    use super::*;

    const PUBLIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34));

    #[derive(Clone)]
    struct FakeResolver {
        addresses: Vec<SocketAddr>,
    }

    impl Resolver for FakeResolver {
        async fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<SocketAddr>, ResolveError> {
            Ok(self.addresses.clone())
        }
    }

    struct QueueTransport {
        responses: Mutex<VecDeque<TransportResponse>>,
        calls: AtomicUsize,
    }

    impl QueueTransport {
        fn new(responses: Vec<TransportResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl Transport for QueueTransport {
        async fn send(
            &self,
            _request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front()
                .ok_or(TransportError::Request)
        }
    }

    #[derive(Clone, Default)]
    struct FakeClock(Arc<Mutex<Duration>>);

    impl FakeClock {
        fn advance(&self, duration: Duration) {
            let mut now = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *now += duration;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    fn response(status: StatusCode, headers: HeaderMap, chunks: &[&[u8]]) -> TransportResponse {
        let chunks = chunks
            .iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>();
        TransportResponse {
            status,
            headers,
            body: Box::pin(stream::iter(chunks)),
        }
    }

    fn client<T: Transport>(transport: T) -> HttpClient<FakeResolver, T, FakeClock> {
        HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            RequestLimits::default(),
        )
        .unwrap()
    }

    fn public_permission() -> NetworkPermission {
        NetworkPermission::parse("https://example.com").unwrap()
    }

    #[test]
    fn classifies_ipv4_and_ipv6_security_boundaries() {
        assert_eq!(
            classify_address("127.0.0.1".parse().unwrap()),
            Some(AddressClass::Loopback)
        );
        assert_eq!(
            classify_address("169.254.1.1".parse().unwrap()),
            Some(AddressClass::LinkLocal)
        );
        assert_eq!(
            classify_address("10.0.0.1".parse().unwrap()),
            Some(AddressClass::Private)
        );
        assert_eq!(
            classify_address("::1".parse().unwrap()),
            Some(AddressClass::Loopback)
        );
        assert_eq!(
            classify_address("fe80::1".parse().unwrap()),
            Some(AddressClass::LinkLocal)
        );
        assert_eq!(
            classify_address("fd00::1".parse().unwrap()),
            Some(AddressClass::Private)
        );
        assert_eq!(
            classify_address("::ffff:127.0.0.1".parse().unwrap()),
            Some(AddressClass::Loopback)
        );
        assert_eq!(
            classify_address("203.0.113.1".parse().unwrap()),
            Some(AddressClass::Reserved)
        );
        assert_eq!(
            classify_address("fec0::1".parse().unwrap()),
            Some(AddressClass::Private)
        );
        for address in ["2001:db8::1", "100::1", "4000::1", "3fff::1"] {
            assert_eq!(
                classify_address(address.parse().unwrap()),
                Some(AddressClass::Reserved),
                "{address} must not be treated as public"
            );
        }
        assert_eq!(
            classify_address("2001:4860:4860::8888".parse().unwrap()),
            None
        );
    }

    #[test]
    fn enforces_https_and_rejects_fake_localhost_names() {
        assert_eq!(
            NetworkPermission::parse("http://example.com").unwrap_err(),
            PermissionError::UrlPolicy(UrlPolicyError::PlaintextRemote)
        );
        assert_eq!(
            NetworkPermission::parse("http://localhost.evil.example").unwrap_err(),
            PermissionError::UrlPolicy(UrlPolicyError::PlaintextRemote)
        );
        assert!(NetworkPermission::parse("http://localhost:8080").is_ok());
        assert!(NetworkPermission::parse("http://127.0.0.1:8080").is_ok());
        assert!(NetworkPermission::parse("http://[::1]:8080").is_ok());
        assert_eq!(
            NetworkPermission::parse("ftp://example.com").unwrap_err(),
            PermissionError::UrlPolicy(UrlPolicyError::UnsupportedScheme)
        );
    }

    #[tokio::test]
    async fn errors_do_not_expose_url_credentials() {
        let transport = QueueTransport::new(vec![]);
        let client = client(transport);
        let secret_url = "https://user:super-secret@example.com/status";
        let error = client
            .execute(
                "plugin",
                &[public_permission()],
                HttpRequest::get(secret_url),
            )
            .await
            .unwrap_err();
        assert_eq!(error, HttpError::CredentialsNotAllowed);
        assert!(!error.to_string().contains("super-secret"));
    }

    #[tokio::test]
    async fn exact_origin_permission_includes_scheme_host_and_port() {
        let transport = QueueTransport::new(vec![]);
        let client = client(transport);
        for url in [
            "https://sub.example.com/path",
            "https://example.com:444/path",
        ] {
            assert_eq!(
                client
                    .execute("plugin", &[public_permission()], HttpRequest::get(url))
                    .await
                    .unwrap_err(),
                HttpError::OriginNotAllowed
            );
        }
    }

    #[tokio::test]
    async fn rejects_dns_rebinding_unless_address_class_is_explicit() {
        let resolver = FakeResolver {
            addresses: vec!["127.0.0.1:443".parse().unwrap()],
        };
        let transport = QueueTransport::new(vec![]);
        let client = HttpClient::new(
            resolver,
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            RequestLimits::default(),
        )
        .unwrap();
        let error = client
            .execute(
                "plugin",
                &[public_permission()],
                HttpRequest::get("https://example.com"),
            )
            .await
            .unwrap_err();
        assert_eq!(error, HttpError::ForbiddenAddress(AddressClass::Loopback));
    }

    #[tokio::test]
    async fn rejects_http_localhost_resolving_only_to_public_address() {
        let transport = QueueTransport::new(vec![]);
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 8080)],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            RequestLimits::default(),
        )
        .unwrap();
        let permission = NetworkPermission::parse("http://localhost:8080")
            .unwrap()
            .allow_address_class(AddressClass::Loopback);
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[permission],
                    HttpRequest::get("http://localhost:8080/status"),
                )
                .await
                .unwrap_err(),
            HttpError::HttpDestinationNotLoopback
        );
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_http_localhost_with_mixed_loopback_and_public_addresses() {
        let transport = QueueTransport::new(vec![]);
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![
                    "127.0.0.1:8080".parse().unwrap(),
                    SocketAddr::new(PUBLIC_IP, 8080),
                ],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            RequestLimits::default(),
        )
        .unwrap();
        let permission = NetworkPermission::parse("http://localhost:8080")
            .unwrap()
            .allow_address_class(AddressClass::Loopback);
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[permission],
                    HttpRequest::get("http://localhost:8080/status"),
                )
                .await
                .unwrap_err(),
            HttpError::HttpDestinationNotLoopback
        );
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn redirect_is_not_followed_by_default() {
        let mut headers = HeaderMap::new();
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://other.example/next"),
        );
        let transport = QueueTransport::new(vec![response(StatusCode::FOUND, headers, &[])]);
        let client = client(transport);
        let result = client
            .execute(
                "plugin",
                &[public_permission()],
                HttpRequest::get("https://example.com/start"),
            )
            .await
            .unwrap();
        assert_eq!(result.status, StatusCode::FOUND);
    }

    #[tokio::test]
    async fn redirect_escape_is_rejected_before_second_connection() {
        let mut headers = HeaderMap::new();
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://other.example/next"),
        );
        let transport = QueueTransport::new(vec![response(StatusCode::FOUND, headers, &[])]);
        let client = client(transport);
        let mut request = HttpRequest::get("https://example.com/start");
        request.redirects = Some(RedirectPolicy { max_hops: 3 });
        let error = client
            .execute("plugin", &[public_permission()], request)
            .await
            .unwrap_err();
        assert_eq!(error, HttpError::OriginNotAllowed);
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn caller_redirect_limit_cannot_exceed_host_hard_limit() {
        let redirect = || {
            let mut headers = HeaderMap::new();
            headers.insert(LOCATION, HeaderValue::from_static("/again"));
            response(StatusCode::FOUND, headers, &[])
        };
        let transport = QueueTransport::new(vec![redirect(), redirect(), redirect()]);
        let limits = RequestLimits {
            max_redirect_hops: 2,
            ..RequestLimits::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            limits,
        )
        .unwrap();
        let mut request = HttpRequest::get("https://example.com/start");
        request.redirects = Some(RedirectPolicy {
            max_hops: usize::MAX,
        });
        assert_eq!(
            client
                .execute("plugin", &[public_permission()], request)
                .await
                .unwrap_err(),
            HttpError::TooManyRedirects
        );
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn each_redirect_hop_consumes_plugin_rate_budget() {
        let redirect = || {
            let mut headers = HeaderMap::new();
            headers.insert(LOCATION, HeaderValue::from_static("/again"));
            response(StatusCode::FOUND, headers, &[])
        };
        let transport = QueueTransport::new(vec![redirect(), redirect()]);
        let limits = RequestLimits {
            max_requests_per_window: 2,
            ..RequestLimits::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            limits,
        )
        .unwrap();
        let mut request = HttpRequest::get("https://example.com/start");
        request.redirects = Some(RedirectPolicy { max_hops: 4 });
        assert!(matches!(
            client
                .execute("plugin", &[public_permission()], request)
                .await,
            Err(HttpError::RateLimit { .. })
        ));
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn streaming_body_stops_at_byte_limit() {
        let pulls = Arc::new(AtomicUsize::new(0));
        let observed_pulls = Arc::clone(&pulls);
        let body = stream::iter([
            Ok(Bytes::from_static(b"1234")),
            Ok(Bytes::from_static(b"5678")),
            Ok(Bytes::from_static(b"must-not-be-polled")),
        ])
        .inspect(move |_| {
            observed_pulls.fetch_add(1, Ordering::SeqCst);
        });
        let transport = QueueTransport::new(vec![TransportResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Box::pin(body),
        }]);
        let limits = RequestLimits {
            max_response_bytes: 6,
            ..RequestLimits::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            HttpTimeouts::default(),
            limits,
        )
        .unwrap();
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com/data"),
                )
                .await
                .unwrap_err(),
            HttpError::ResponseTooLarge { limit: 6 }
        );
        assert_eq!(pulls.load(Ordering::SeqCst), 2);
    }

    struct DelayedRedirectTransport {
        calls: AtomicUsize,
        delay: Duration,
    }

    impl Transport for DelayedRedirectTransport {
        async fn send(
            &self,
            _request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            let mut headers = HeaderMap::new();
            headers.insert(LOCATION, HeaderValue::from_static("/next"));
            Ok(response(StatusCode::FOUND, headers, &[]))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn resource_timeout_accumulates_across_redirect_hops() {
        let timeouts = HttpTimeouts {
            request: Duration::from_millis(100),
            resource: Duration::from_millis(45),
            ..HttpTimeouts::default()
        };
        let transport = DelayedRedirectTransport {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(20),
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            timeouts,
            RequestLimits::default(),
        )
        .unwrap();
        let mut request = HttpRequest::get("https://example.com/start");
        request.redirects = Some(RedirectPolicy { max_hops: 5 });
        assert_eq!(
            client
                .execute("plugin", &[public_permission()], request)
                .await
                .unwrap_err(),
            HttpError::Timeout(TimeoutPhase::Resource)
        );
        assert_eq!(client.transport.calls.load(Ordering::SeqCst), 3);
    }

    struct PendingTransport;

    impl Transport for PendingTransport {
        async fn send(
            &self,
            _request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn request_timeout_is_distinct() {
        let timeouts = HttpTimeouts {
            request: Duration::from_millis(10),
            ..HttpTimeouts::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            PendingTransport,
            FakeClock::default(),
            timeouts,
            RequestLimits::default(),
        )
        .unwrap();
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
                .unwrap_err(),
            HttpError::Timeout(TimeoutPhase::Request)
        );
    }

    #[tokio::test]
    async fn rejects_explicit_cookie_headers() {
        let transport = QueueTransport::new(vec![]);
        let client = client(transport);
        let mut request = HttpRequest::get("https://example.com");
        request
            .headers
            .insert(COOKIE, HeaderValue::from_static("session=secret"));
        assert_eq!(
            client
                .execute("plugin", &[public_permission()], request)
                .await
                .unwrap_err(),
            HttpError::CookieHeaderNotAllowed
        );
    }

    struct ConnectTimeoutTransport {
        observed_timeout: Arc<Mutex<Option<Duration>>>,
    }

    impl Transport for ConnectTimeoutTransport {
        async fn send(
            &self,
            request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            *self
                .observed_timeout
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(request.connect_timeout);
            Err(TransportError::ConnectTimeout)
        }
    }

    #[tokio::test]
    async fn connect_timeout_is_configured_and_typed_separately() {
        let observed_timeout = Arc::new(Mutex::new(None));
        let timeouts = HttpTimeouts {
            connect: Duration::from_millis(17),
            ..HttpTimeouts::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            ConnectTimeoutTransport {
                observed_timeout: Arc::clone(&observed_timeout),
            },
            FakeClock::default(),
            timeouts,
            RequestLimits::default(),
        )
        .unwrap();
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
                .unwrap_err(),
            HttpError::Timeout(TimeoutPhase::Connect)
        );
        assert_eq!(
            *observed_timeout
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Some(timeouts.connect)
        );
    }

    #[tokio::test]
    async fn resource_timeout_is_distinct() {
        let pending_body: BodyStream = Box::pin(stream::pending());
        let transport = QueueTransport::new(vec![TransportResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: pending_body,
        }]);
        let timeouts = HttpTimeouts {
            resource: Duration::from_millis(10),
            ..HttpTimeouts::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            FakeClock::default(),
            timeouts,
            RequestLimits::default(),
        )
        .unwrap();
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
                .unwrap_err(),
            HttpError::Timeout(TimeoutPhase::Resource)
        );
    }

    #[tokio::test]
    async fn rate_limits_are_isolated_per_plugin() {
        let transport = QueueTransport::new(vec![
            response(StatusCode::OK, HeaderMap::new(), &[]),
            response(StatusCode::OK, HeaderMap::new(), &[]),
            response(StatusCode::OK, HeaderMap::new(), &[]),
            response(StatusCode::OK, HeaderMap::new(), &[]),
        ]);
        let clock = FakeClock::default();
        let limits = RequestLimits {
            max_requests_per_window: 2,
            ..RequestLimits::default()
        };
        let client = HttpClient::new(
            FakeResolver {
                addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
            },
            transport,
            clock.clone(),
            HttpTimeouts::default(),
            limits,
        )
        .unwrap();
        for _ in 0..2 {
            client
                .execute(
                    "plugin-a",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
                .unwrap();
        }
        assert!(matches!(
            client
                .execute(
                    "plugin-a",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await,
            Err(HttpError::RateLimit { .. })
        ));
        client
            .execute(
                "plugin-b",
                &[public_permission()],
                HttpRequest::get("https://example.com"),
            )
            .await
            .unwrap();
        clock.advance(limits.request_rate_window);
        client
            .execute(
                "plugin-a",
                &[public_permission()],
                HttpRequest::get("https://example.com"),
            )
            .await
            .unwrap();
    }

    struct BlockingTransport {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl Transport for BlockingTransport {
        async fn send(
            &self,
            _request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(response(StatusCode::OK, HeaderMap::new(), &[]))
        }
    }

    #[tokio::test]
    async fn concurrency_limit_rejects_overlapping_plugin_request() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let transport = BlockingTransport {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        };
        let limits = RequestLimits {
            max_concurrent_requests_per_plugin: 1,
            ..RequestLimits::default()
        };
        let client = Arc::new(
            HttpClient::new(
                FakeResolver {
                    addresses: vec![SocketAddr::new(PUBLIC_IP, 443)],
                },
                transport,
                FakeClock::default(),
                HttpTimeouts::default(),
                limits,
            )
            .unwrap(),
        );
        let first_client = Arc::clone(&client);
        let first = tokio::spawn(async move {
            first_client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
        });
        started.notified().await;
        assert_eq!(
            client
                .execute(
                    "plugin",
                    &[public_permission()],
                    HttpRequest::get("https://example.com"),
                )
                .await
                .unwrap_err(),
            HttpError::ConcurrencyLimit
        );
        release.notify_one();
        first.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn real_transport_uses_injected_resolver_and_never_persists_cookies() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 512];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&chunk[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let expected_path = if index == 0 { "/first" } else { "/second" };
                assert!(request.starts_with(&format!("GET {expected_path} HTTP/1.1")));
                assert!(!request.to_ascii_lowercase().contains("\r\ncookie:"));
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: session=must-not-return\r\nConnection: close\r\n\r\nok",
                    )
                    .await
                    .unwrap();
            }
        });
        let resolver = FakeResolver {
            addresses: vec![address],
        };
        let client = HttpClient::new(
            resolver,
            ReqwestTransport,
            FakeClock::default(),
            HttpTimeouts::default(),
            RequestLimits::default(),
        )
        .unwrap();
        let origin = format!("http://localhost:{}", address.port());
        let permission = NetworkPermission::parse(&origin)
            .unwrap()
            .allow_address_class(AddressClass::Loopback);
        let permissions = [permission];
        for path in ["first", "second"] {
            let result = client
                .execute(
                    "plugin",
                    &permissions,
                    HttpRequest::get(format!("{origin}/{path}")),
                )
                .await
                .unwrap();
            assert_eq!(result.body, Bytes::from_static(b"ok"));
        }
        server.await.unwrap();
    }
}
