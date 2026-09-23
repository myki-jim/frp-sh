use super::{
    action_token, normalize_domain, verification_name, ChallengeRequest, Operation, SignedRequest,
    TunnelRequest, TunnelResponse,
};
use crate::{device::Challenges, domains::storage::Worker};
use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::Message, ws::WebSocket, ws::WebSocketUpgrade, DefaultBodyLimit, Query, Request, State,
    },
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, Mutex, Semaphore};

const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, clap::Args)]
#[group(id = "domain_options")]
pub struct Options {
    /// Enable custom-domain control and ingress using this SQLite database
    #[arg(long)]
    pub domains_db: Option<PathBuf>,
    /// Public signaling origin used to bind signed domain operations
    #[arg(long, requires = "domains_db")]
    pub domains_origin: Option<String>,
    /// Loopback HTTP ingress listener used by the TLS gateway
    #[arg(long, requires = "domains_db")]
    pub ingress_addr: Option<String>,
    /// DNS target users point custom domains to
    #[arg(long, requires = "domains_db")]
    pub ingress_cname: Option<String>,
    /// Public HTTPS port shown in binding instructions
    #[arg(long, default_value_t = 443)]
    pub ingress_https_port: u16,
}

#[derive(Clone)]
pub struct Service(Arc<Inner>);

struct Inner {
    db: Worker,
    challenges: Mutex<Challenges>,
    sessions: Mutex<HashMap<String, Session>>,
    publishers: Mutex<HashMap<String, Publisher>>,
    origin: String,
    cname: String,
    https_port: u16,
    http: reqwest::Client,
    slots: Semaphore,
}

struct Session {
    domain: String,
    owner: String,
    expires_at: u64,
}

#[derive(Clone)]
struct Publisher {
    id: String,
    owner: String,
    tx: mpsc::Sender<Dispatch>,
}

struct Dispatch {
    request: TunnelRequest,
    reply: oneshot::Sender<TunnelResponse>,
}

struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}

impl Service {
    pub async fn open(options: &Options) -> anyhow::Result<Self> {
        let origin = options
            .domains_origin
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--domains-origin is required"))?
            .trim_end_matches('/')
            .to_owned();
        let url = reqwest::Url::parse(&origin)?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https"),
            "invalid domains origin"
        );
        let cname = normalize_domain(
            options
                .ingress_cname
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--ingress-cname is required"))?,
        )?;
        let db = Worker::open(
            options
                .domains_db
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--domains-db is required"))?,
        )
        .await?;
        Ok(Self(Arc::new(Inner {
            db,
            challenges: Mutex::new(Challenges::default()),
            sessions: Mutex::new(HashMap::new()),
            publishers: Mutex::new(HashMap::new()),
            origin,
            cname,
            https_port: options.ingress_https_port,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            slots: Semaphore::new(256),
        })))
    }

    pub fn control_router(self) -> Router {
        Router::new()
            .route("/domains/v1/challenge", post(challenge))
            .route("/domains/v1/execute", post(execute))
            .route("/domains/v1/allow", get(allow_certificate))
            .route("/domains/v1/publish", get(upgrade))
            .layer(DefaultBodyLimit::max(16 * 1024))
            .with_state(self)
    }

    pub async fn run_ingress(self, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .fallback(any(public_request))
            .layer(DefaultBodyLimit::max(MAX_REQUEST))
            .with_state(self);
        axum::serve(listener, app).await?;
        Ok(())
    }

    async fn dns_txt(&self, name: &str) -> anyhow::Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Answer {
            data: String,
        }
        #[derive(Deserialize)]
        struct DnsResponse {
            #[serde(rename = "Answer", default)]
            answer: Vec<Answer>,
        }
        let mut last_error = None;
        let mut answered = false;
        for resolver in [
            "https://dns.alidns.com/resolve",
            "https://doh.pub/dns-query",
            "https://cloudflare-dns.com/dns-query",
        ] {
            let response = self
                .0
                .http
                .get(resolver)
                .query(&[("name", name), ("type", "TXT")])
                .header(header::ACCEPT, "application/dns-json")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            match response {
                Ok(response) => {
                    let response = response.json::<DnsResponse>().await?;
                    answered = true;
                    let records = response
                        .answer
                        .into_iter()
                        .map(|answer| answer.data.replace(['"', ' '], ""))
                        .collect::<Vec<_>>();
                    if !records.is_empty() {
                        return Ok(records);
                    }
                }
                Err(error) => last_error = Some(error),
            }
        }
        if answered {
            return Ok(Vec::new());
        }
        Err(last_error
            .map(Into::into)
            .unwrap_or_else(|| anyhow::anyhow!("no DNS resolver configured")))
    }

    async fn issue_session(&self, domain: String, owner: String) -> anyhow::Result<String> {
        let binding = self.0.db.owned(domain.clone(), owner.clone()).await?;
        anyhow::ensure!(binding.verified, "domain is not verified");
        let token = crate::utils::random_hex(32);
        let now = crate::utils::now_unix();
        let mut sessions = self.0.sessions.lock().await;
        sessions.retain(|_, value| value.expires_at > now);
        anyhow::ensure!(sessions.len() < 1024, "publisher session capacity reached");
        sessions.insert(
            digest(&token),
            Session {
                domain,
                owner,
                expires_at: now + 60,
            },
        );
        Ok(token)
    }
}

#[derive(Deserialize)]
struct AllowQuery {
    domain: String,
}

async fn allow_certificate(
    State(service): State<Service>,
    Query(query): Query<AllowQuery>,
) -> StatusCode {
    let Ok(domain) = normalize_domain(&query.domain) else {
        return StatusCode::BAD_REQUEST;
    };
    match service.0.db.public(domain).await {
        Ok(Some(_)) => StatusCode::NO_CONTENT,
        Ok(None) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

async fn challenge(
    State(service): State<Service>,
    Json(request): Json<ChallengeRequest>,
) -> Result<Json<crate::device::Challenge>, ApiError> {
    let hash = request.action_hash;
    service
        .0
        .challenges
        .lock()
        .await
        .issue(
            &service.0.origin,
            &request.device,
            &hash,
            crate::utils::now_unix(),
        )
        .map(Json)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "challenge_unavailable"))
}

async fn execute(
    State(service): State<Service>,
    Json(request): Json<SignedRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = crate::utils::now_unix();
    let hash = request
        .operation
        .digest()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_operation"))?;
    let device = service
        .0
        .challenges
        .lock()
        .await
        .verify(&request.nonce, &request.signature, &hash, now)
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid_device_proof"))?;
    let owner = device.public_key().to_owned();
    let result: anyhow::Result<serde_json::Value> = match request.operation {
        Operation::Bind { domain } => {
            async {
                let domain = normalize_domain(&domain)?;
                let token = crate::utils::random_hex(24);
                let txt_name = verification_name(&domain, &token)?;
                service
                    .0
                    .db
                    .begin(
                        domain.clone(),
                        owner,
                        action_token(&token),
                        txt_name.clone(),
                        now,
                    )
                    .await?;
                Ok(serde_json::json!({
                    "domain":domain,
                    "txt_name":txt_name,
                    "txt_value":action_token(&token),
                    "cname_target":service.0.cname,
                    "https_port":service.0.https_port,
                    "expires_at":now+900
                }))
            }
            .await
        }
        Operation::Verify { domain } => {
            async {
                let domain = normalize_domain(&domain)?;
                let binding = service.0.db.owned(domain.clone(), owner.clone()).await?;
                anyhow::ensure!(
                    !binding.verification_name.is_empty(),
                    "bind the domain again"
                );
                let records = service.dns_txt(&binding.verification_name).await?;
                Ok(serde_json::to_value(
                    service.0.db.verify(domain, owner, records, now).await?,
                )?)
            }
            .await
        }
        Operation::Status { domain } => {
            async {
                let domain = normalize_domain(&domain)?;
                Ok(serde_json::to_value(
                    service.0.db.owned(domain, owner).await?,
                )?)
            }
            .await
        }
        Operation::Unbind { domain } => {
            async {
                let domain = normalize_domain(&domain)?;
                service.0.db.remove(domain.clone(), owner).await?;
                service.0.publishers.lock().await.remove(&domain);
                Ok(serde_json::json!({"ok":true}))
            }
            .await
        }
        Operation::OpenPublisher { domain } => {
            async {
                let domain = normalize_domain(&domain)?;
                let token = service.issue_session(domain, owner).await?;
                Ok(serde_json::json!({"publisher_token":token,"expires_at":now+60}))
            }
            .await
        }
    };
    result
        .map(Json)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "domain_operation_rejected"))
}

async fn upgrade(
    State(service): State<Service>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "publisher_token_required",
        ))?;
    let now = crate::utils::now_unix();
    let session = service
        .0
        .sessions
        .lock()
        .await
        .remove(&digest(token))
        .filter(|session| session.expires_at > now)
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_publisher_token",
        ))?;
    Ok(upgrade
        .on_upgrade(move |socket| publisher_socket(service, session, socket))
        .into_response())
}

async fn publisher_socket(service: Service, session: Session, socket: WebSocket) {
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, mut rx) = mpsc::channel::<Dispatch>(32);
    service.0.publishers.lock().await.insert(
        session.domain.clone(),
        Publisher {
            id: id.clone(),
            owner: session.owner,
            tx,
        },
    );
    let (mut sink, mut source) = socket.split();
    let mut pending = HashMap::<String, oneshot::Sender<TunnelResponse>>::new();
    loop {
        tokio::select! {
            dispatch = rx.recv() => match dispatch {
                Some(dispatch) => {
                    pending.insert(dispatch.request.id.clone(), dispatch.reply);
                    let Ok(text)=serde_json::to_string(&dispatch.request) else { break };
                    if sink.send(Message::Text(text.into())).await.is_err() { break; }
                }
                None => break,
            },
            message = source.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if text.len()>MAX_RESPONSE.saturating_mul(2) { break; }
                    let Ok(response)=serde_json::from_str::<TunnelResponse>(&text) else { break };
                    if let Some(reply)=pending.remove(&response.id) { let _=reply.send(response); }
                }
                Some(Ok(Message::Pong(_))) => {},
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {},
            }
        }
    }
    let mut publishers = service.0.publishers.lock().await;
    if publishers
        .get(&session.domain)
        .is_some_and(|publisher| publisher.id == id)
    {
        publishers.remove(&session.domain);
    }
}

async fn public_request(State(service): State<Service>, request: Request) -> Response {
    match public_request_inner(service, request).await {
        Ok(response) => response,
        Err((status, code)) => (status, Json(serde_json::json!({"error":code}))).into_response(),
    }
}

async fn public_request_inner(
    service: Service,
    request: Request,
) -> Result<Response, (StatusCode, &'static str)> {
    let _slot = service
        .0
        .slots
        .try_acquire()
        .map_err(|_| (StatusCode::TOO_MANY_REQUESTS, "ingress_busy"))?;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(':').next())
        .ok_or((StatusCode::BAD_REQUEST, "invalid_host"))?;
    let domain = normalize_domain(host).map_err(|_| (StatusCode::BAD_REQUEST, "invalid_host"))?;
    let binding = service
        .0
        .db
        .public(domain.clone())
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "ingress_unavailable"))?
        .ok_or((StatusCode::NOT_FOUND, "domain_not_bound"))?;
    let publisher = service
        .0
        .publishers
        .lock()
        .await
        .get(&domain)
        .cloned()
        .filter(|publisher| publisher.owner == binding.owner)
        .ok_or((StatusCode::BAD_GATEWAY, "publisher_offline"))?;
    let method = request.method().as_str().to_owned();
    if method == "CONNECT" {
        return Err((StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
    }
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_owned();
    let headers = filtered_headers(request.headers())?;
    let body = to_bytes(request.into_body(), MAX_REQUEST)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "request_too_large"))?;
    let (reply, response) = oneshot::channel();
    publisher
        .tx
        .try_send(Dispatch {
            request: TunnelRequest {
                id: uuid::Uuid::new_v4().to_string(),
                host: domain,
                method,
                path,
                headers,
                body: BASE64.encode(body),
            },
            reply,
        })
        .map_err(|_| (StatusCode::TOO_MANY_REQUESTS, "publisher_busy"))?;
    let response = tokio::time::timeout(Duration::from_secs(30), response)
        .await
        .map_err(|_| (StatusCode::GATEWAY_TIMEOUT, "publisher_timeout"))?
        .map_err(|_| (StatusCode::BAD_GATEWAY, "publisher_disconnected"))?;
    let status = StatusCode::from_u16(response.status)
        .map_err(|_| (StatusCode::BAD_GATEWAY, "invalid_publisher_response"))?;
    let body = BASE64
        .decode(response.body)
        .map_err(|_| (StatusCode::BAD_GATEWAY, "invalid_publisher_response"))?;
    if body.len() > MAX_RESPONSE {
        return Err((StatusCode::BAD_GATEWAY, "response_too_large"));
    }
    let mut builder = Response::builder().status(status);
    for (name, value) in response.headers {
        let name = HeaderName::try_from(name)
            .map_err(|_| (StatusCode::BAD_GATEWAY, "invalid_publisher_response"))?;
        if hop_header(&name) || name == header::CONTENT_LENGTH {
            continue;
        }
        let value = HeaderValue::try_from(value)
            .map_err(|_| (StatusCode::BAD_GATEWAY, "invalid_publisher_response"))?;
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from(body))
        .map_err(|_| (StatusCode::BAD_GATEWAY, "invalid_publisher_response"))
}

fn filtered_headers(
    headers: &HeaderMap,
) -> Result<Vec<(String, String)>, (StatusCode, &'static str)> {
    let mut result = Vec::new();
    let mut size = 0usize;
    for (name, value) in headers {
        if hop_header(name)
            || matches!(
                name.as_str(),
                "host" | "content-length" | "x-forwarded-for" | "x-forwarded-proto" | "x-real-ip"
            )
        {
            continue;
        }
        let value = value
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_header"))?;
        size = size.saturating_add(name.as_str().len() + value.len());
        if size > 32 * 1024 || result.len() >= 100 {
            return Err((
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "headers_too_large",
            ));
        }
        result.push((name.as_str().to_owned(), value.to_owned()));
    }
    Ok(result)
}

fn hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn service() -> Service {
        Service::open(&Options {
            domains_db: Some(
                std::env::temp_dir().join(format!("frpsh-ingress-{}.sqlite", uuid::Uuid::new_v4())),
            ),
            domains_origin: Some("https://control.example.com".into()),
            ingress_addr: Some("127.0.0.1:0".into()),
            ingress_cname: Some("edge.example.com".into()),
            ingress_https_port: 18443,
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn ingress_routes_only_verified_live_publishers() {
        let service = service().await;
        let domain = "app.example.com".to_owned();
        let owner = "ab".repeat(32);
        service
            .0
            .db
            .begin(
                domain.clone(),
                owner.clone(),
                "frpsh-verify=secret".into(),
                "_frpsh-verify-deadbeef0000.app.example.com".into(),
                10,
            )
            .await
            .unwrap();
        service
            .0
            .db
            .verify(
                domain.clone(),
                owner.clone(),
                vec!["frpsh-verify=secret".into()],
                20,
            )
            .await
            .unwrap();
        let (tx, mut rx) = mpsc::channel::<Dispatch>(1);
        service.0.publishers.lock().await.insert(
            domain.clone(),
            Publisher {
                id: "publisher".into(),
                owner,
                tx,
            },
        );
        tokio::spawn(async move {
            let dispatch = rx.recv().await.unwrap();
            assert_eq!(dispatch.request.host, "app.example.com");
            assert_eq!(dispatch.request.path, "/hello?q=1");
            dispatch
                .reply
                .send(TunnelResponse {
                    id: dispatch.request.id,
                    status: 201,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    body: BASE64.encode("tunneled"),
                })
                .unwrap();
        });
        let request = Request::builder()
            .uri("/hello?q=1")
            .header(header::HOST, domain)
            .body(Body::empty())
            .unwrap();
        let response = public_request_inner(service, request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            to_bytes(response.into_body(), 100).await.unwrap(),
            "tunneled"
        );
    }
}
