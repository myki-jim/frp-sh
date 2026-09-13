use super::*;
use crate::{
    device::Challenges,
    storage::{worker::Worker, Quota},
};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, Debug, clap::Args)]
pub struct Options {
    /// Enable durable space control APIs using this SQLite database
    #[arg(long)]
    pub spaces_db: Option<PathBuf>,
    /// Public HTTPS origin used for device proof binding and invitation links
    #[arg(long, requires = "spaces_db")]
    pub spaces_origin: Option<String>,
    /// Default invitation lifetime in seconds
    #[arg(long,default_value_t=900,value_parser=clap::value_parser!(u64).range(1..=86400))]
    pub invite_ttl: u64,
    /// Maximum invitation lifetime in seconds
    #[arg(long,default_value_t=86400,value_parser=clap::value_parser!(u64).range(1..=86400))]
    pub invite_max_ttl: u64,
}
#[derive(Clone)]
pub struct Service(Arc<Inner>);
struct Inner {
    db: Worker,
    challenges: Mutex<Challenges>,
    quota: Quota,
    origin: String,
    admin: String,
    ttl: u64,
    max_ttl: u64,
    slots: Semaphore,
}
pub struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl Service {
    pub async fn open(options: Options, admin: String, quota: Quota) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !admin.trim().is_empty(),
            "space creation requires a configured server password"
        );
        anyhow::ensure!(
            options.invite_ttl > 0
                && options.invite_ttl <= options.invite_max_ttl
                && options.invite_max_ttl <= 86400,
            "invalid invite lifetime policy"
        );
        let origin = options
            .spaces_origin
            .ok_or_else(|| anyhow::anyhow!("--spaces-origin is required"))?;
        let ticket = crate::invite_ticket::Ticket::new(&origin, &"00".repeat(32))?;
        let _ = ticket;
        // Challenge server identifiers have a fixed bound as well as URL validation.
        anyhow::ensure!(origin.len() <= 128, "space origin is too long");
        let db = Worker::open(
            options
                .spaces_db
                .ok_or_else(|| anyhow::anyhow!("--spaces-db is required"))?,
        )
        .await?;
        Ok(Self(Arc::new(Inner {
            db,
            challenges: Mutex::new(Challenges::default()),
            quota,
            origin,
            admin,
            ttl: options.invite_ttl,
            max_ttl: options.invite_max_ttl,
            slots: Semaphore::new(32),
        })))
    }
    pub fn router(self) -> Router {
        Router::new()
            .route("/spaces/v1/challenge", post(challenge))
            .route("/spaces/v1/execute", post(execute))
            .layer(DefaultBodyLimit::max(8192))
            .with_state(self)
    }
}
async fn challenge(
    State(s): State<Service>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<crate::device::Challenge>, ApiError> {
    let _slot =
        s.0.slots
            .try_acquire()
            .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "busy"))?;
    s.0.challenges
        .lock()
        .await
        .issue(
            &s.0.origin,
            &req.device,
            &req.action_hash,
            crate::utils::now_unix(),
        )
        .map(Json)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "challenge_unavailable"))
}
async fn execute(
    State(s): State<Service>,
    headers: HeaderMap,
    Json(req): Json<SignedRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _slot =
        s.0.slots
            .try_acquire()
            .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "busy"))?;
    let now = crate::utils::now_unix();
    let hash = req
        .operation
        .digest()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_operation"))?;
    let device =
        s.0.challenges
            .lock()
            .await
            .verify(&req.nonce, &req.signature, &hash, now)
            .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid_device_proof"))?;
    if matches!(req.operation, Operation::Create { .. }) {
        let supplied = headers
            .get("X-Frp-Sh-Token")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        // Compare fixed-size hashes instead of variable-length password strings.
        use hmac::{Hmac, Mac};
        let mut verifier =
            Hmac::<Sha256>::new_from_slice(b"frp-sh/admin-check/v1").expect("HMAC key");
        verifier.update(s.0.admin.as_bytes());
        let mut candidate =
            Hmac::<Sha256>::new_from_slice(b"frp-sh/admin-check/v1").expect("HMAC key");
        candidate.update(supplied.as_bytes());
        verifier
            .verify_slice(&candidate.finalize().into_bytes())
            .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "creation_not_authorized"))?;
    }
    let quota = s.0.quota;
    let ttl = s.0.ttl;
    let max_ttl = s.0.max_ttl;
    let origin = s.0.origin.clone();
    let result=s.0.db.call(move |db| {
        Ok(match req.operation {
            Operation::Create{name,expires_at,request_id}=>serde_json::to_value(db.create_idempotent(&device,&name,expires_at,now,quota,&request_id)?)?,
            Operation::List=>serde_json::to_value(db.list(&device,now)?)?,
            Operation::Members{space}=>serde_json::to_value(db.members(&space,&device,now)?)?,
            Operation::Invite{space,ttl:requested,uses}=> {
                let lifetime=requested.unwrap_or(ttl);
                anyhow::ensure!(lifetime>0 && lifetime<=max_ttl,"invite policy exceeded");
                let token=db.invite(&space,&device,now,lifetime,uses)?;
                let ticket=crate::invite_ticket::Ticket::new(&origin,&token)?;
                serde_json::json!({"invitation":ticket.link()?,"expires_at":now+lifetime,"uses":uses})
            },
            Operation::Redeem{token,request_id}=>serde_json::to_value(db.redeem(&token,&device,&request_id,now,quota)?)?,
            Operation::RevokeInvites{space}=>{db.revoke_invites(&space,&device,now)?;serde_json::json!({"ok":true})},
            Operation::RemoveMember{space,device:removed}=>{db.membership(&space,&device,now)?;db.remove_member(&space,&device,&removed)?;serde_json::json!({"ok":true})},
            Operation::Leave{space}=>{db.leave(&space,&device,now)?;serde_json::json!({"ok":true})},
            Operation::Delete{space}=>{db.delete(&space,&device,now)?;serde_json::json!({"ok":true})},
        })
    }).await;
    result
        .map(Json)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "space_operation_rejected"))
}
