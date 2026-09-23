use super::{ChallengeRequest, Operation, SignedRequest, TunnelRequest, TunnelResponse};
use crate::device::key::DeviceKey;
use anyhow::ensure;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

const MAX_RESPONSE: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: String,
}

impl Client {
    pub fn new(origin: &str) -> anyhow::Result<Self> {
        let origin = origin.trim_end_matches('/');
        let parsed = reqwest::Url::parse(origin)?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "invalid server origin"
        );
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
            origin: origin.into(),
        })
    }

    pub async fn execute(
        &self,
        key: &DeviceKey,
        operation: Operation,
    ) -> anyhow::Result<serde_json::Value> {
        let hash = operation.digest()?;
        let endpoint = format!("{}/domains/v1", self.origin);
        let challenge = self
            .http
            .post(format!("{endpoint}/challenge"))
            .json(&ChallengeRequest {
                device: key.public_key(),
                action_hash: hash.clone(),
            })
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("domain server unavailable"))?;
        let challenge: crate::device::Challenge = response(challenge).await?;
        let signature = key.sign(&challenge, &self.origin, &hash)?;
        response(
            self.http
                .post(format!("{endpoint}/execute"))
                .json(&SignedRequest {
                    nonce: challenge.nonce,
                    signature,
                    operation,
                })
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("domain server unavailable"))?,
        )
        .await
    }

    pub async fn publish(
        &self,
        key: &DeviceKey,
        domain: String,
        target: String,
    ) -> anyhow::Result<()> {
        let session = self
            .execute(
                key,
                Operation::OpenPublisher {
                    domain: domain.clone(),
                },
            )
            .await?;
        let token = session
            .get("publisher_token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("invalid publisher session"))?;
        let mut url = reqwest::Url::parse(&self.origin)?;
        url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
            .map_err(|_| anyhow::anyhow!("invalid server origin"))?;
        url.set_path("/domains/v1/publish");
        url.set_query(None);
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
        let (socket, _) = tokio_tungstenite::connect_async(request).await?;
        let (mut sink, mut source) = socket.split();
        let (tx, mut rx) = mpsc::channel::<TunnelResponse>(32);
        let local = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(25))
            .build()?;
        crate::ui_println!("Public HTTPS active for {domain} -> {target}");
        loop {
            tokio::select! {
                response = rx.recv() => match response {
                    Some(response) => sink.send(Message::Text(serde_json::to_string(&response)?.into())).await?,
                    None => anyhow::bail!("publisher worker stopped"),
                },
                message = source.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        let request=serde_json::from_str::<TunnelRequest>(&text)?;
                        let tx=tx.clone();
                        let local=local.clone();
                        let target=target.clone();
                        tokio::spawn(async move {
                            let response=forward(local,&target,request).await;
                            let _=tx.send(response).await;
                        });
                    }
                    Some(Ok(Message::Ping(value))) => sink.send(Message::Pong(value)).await?,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => anyhow::bail!("publisher disconnected"),
                    _ => {},
                }
            }
        }
    }
}

async fn response<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> anyhow::Result<T> {
    ensure!(
        response.status().is_success(),
        "domain request rejected (HTTP {})",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 512 * 1024,
            "domain response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

async fn forward(client: reqwest::Client, target: &str, request: TunnelRequest) -> TunnelResponse {
    let id = request.id.clone();
    match forward_inner(client, target, request).await {
        Ok(response) => response,
        Err(_) => TunnelResponse {
            id,
            status: 502,
            headers: vec![("content-type".into(), "application/json".into())],
            body: BASE64.encode(br#"{"error":"local_service_unavailable"}"#),
        },
    }
}

async fn forward_inner(
    client: reqwest::Client,
    target: &str,
    request: TunnelRequest,
) -> anyhow::Result<TunnelResponse> {
    ensure!(
        request.path.starts_with('/') && !request.path.starts_with("//"),
        "invalid path"
    );
    let method = reqwest::Method::from_bytes(request.method.as_bytes())?;
    ensure!(method != reqwest::Method::CONNECT, "CONNECT is not allowed");
    let mut outbound = client.request(method, format!("{target}{}", request.path));
    for (name, value) in request.headers {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())?;
        if hop_header(name.as_str()) {
            continue;
        }
        outbound = outbound.header(name, value);
    }
    outbound = outbound
        .header("host", &request.host)
        .header("x-forwarded-host", &request.host)
        .header("x-forwarded-proto", "https")
        .body(BASE64.decode(request.body)?);
    let mut response = outbound.send().await?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| !hop_header(name.as_str()) && name.as_str() != "content-length")
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_RESPONSE,
            "local response too large"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(TunnelResponse {
        id: request.id,
        status,
        headers,
        body: BASE64.encode(body),
    })
}

fn hop_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
    )
}
