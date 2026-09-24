use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fieldservice_domain_dispatch::dispatch::{
    apply_verification, DispatchStatus, VerificationEvent, WorkOrder,
};
use fieldservice_domain_dispatch::infrai::{InfraiClient, InfraiError, BASE_URL};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct AppState {
    client: InfraiClient,
    webhook_secret: String,
    orders: Arc<RwLock<HashMap<String, WorkOrder>>>,
}

#[derive(Debug, Deserialize)]
struct OnboardRequest {
    order: WorkOrder,
    cname_target: String,
    ownership_token: String,
}

#[derive(Debug, Error)]
enum ServiceError {
    #[error(transparent)]
    Infrai(#[from] InfraiError),
    #[error("invalid webhook signature")]
    Signature,
    #[error("invalid webhook payload: {0}")]
    Payload(#[from] serde_json::Error),
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            ServiceError::Infrai(InfraiError::Rejected { status, .. }) => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
            }
            ServiceError::Signature | ServiceError::Payload(_) => StatusCode::BAD_REQUEST,
            ServiceError::Infrai(_) => StatusCode::BAD_GATEWAY,
        };
        (status, Json(json!({"error": self.to_string()}))).into_response()
    }
}

#[tokio::main]
async fn main() {
    let api_key = env::var("INFRAI_API_KEY").expect("INFRAI_API_KEY must be set");
    let webhook_url = env::var("PUBLIC_WEBHOOK_URL").expect("PUBLIC_WEBHOOK_URL must be set");
    let webhook_secret =
        env::var("INFRAI_WEBHOOK_SECRET").expect("INFRAI_WEBHOOK_SECRET must be set");
    let client = InfraiClient::new(api_key);
    client
        .register_verification_webhook(&webhook_url, &webhook_secret)
        .await
        .expect("register the domain verification webhook");
    let state = AppState {
        client,
        webhook_secret,
        orders: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/onboard", post(onboard))
        .route("/webhooks/infrai", post(verification_webhook))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("listen on 127.0.0.1:3000");
    println!("field_dispatch listening on http://127.0.0.1:3000 using {BASE_URL}");
    axum::serve(listener, app).await.expect("serve requests");
}

async fn onboard(
    State(state): State<AppState>,
    Json(mut request): Json<OnboardRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    request.order.dispatch_status = DispatchStatus::AwaitingDomain;
    let onboarding_id = request.order.work_order_id.clone();
    let domain = state
        .client
        .add_domain(&request.order.customer_domain, &onboarding_id)
        .await?;
    state
        .client
        .upsert_record(
            &domain.zone_id,
            "CNAME",
            "dispatch",
            &request.cname_target,
            &onboarding_id,
        )
        .await?;
    state
        .client
        .upsert_record(
            &domain.zone_id,
            "TXT",
            "_infrai-verify",
            &request.ownership_token,
            &onboarding_id,
        )
        .await?;
    let customer_domain = request.order.customer_domain.clone();
    state
        .orders
        .write()
        .await
        .insert(onboarding_id.clone(), request.order);
    state.client.verify_domain(&customer_domain).await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "work_order_id": onboarding_id,
            "zone_id": domain.zone_id,
            "dispatch_status": "awaiting_domain"
        })),
    ))
}

async fn verification_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ServiceError> {
    let signature = headers
        .get("x-infrai-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServiceError::Signature)?;
    verify_signature(&state.webhook_secret, &body, signature)?;
    let event: VerificationEvent = serde_json::from_slice(&body)?;

    let mut orders = state.orders.write().await;
    let mut released = Vec::new();
    for order in orders.values_mut() {
        if apply_verification(order, &event) {
            released.push(order.work_order_id.clone());
        }
    }
    Ok(Json(
        json!({"accepted": true, "released_work_orders": released}),
    ))
}

fn verify_signature(secret: &str, body: &[u8], supplied: &str) -> Result<(), ServiceError> {
    let supplied = supplied.strip_prefix("sha256=").unwrap_or(supplied);
    let bytes = decode_hex(supplied).ok_or(ServiceError::Signature)?;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| ServiceError::Signature)?;
    mac.update(body);
    mac.verify_slice(&bytes)
        .map_err(|_| ServiceError::Signature)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}
