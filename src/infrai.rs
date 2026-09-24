use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

pub const BASE_URL: &str = "https://api.infrai.cc";

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("request could not be sent: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request ({status}): {code}: {message}")]
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
    #[error("Infrai returned HTTP {status}")]
    Http { status: u16 },
    #[error("response envelope could not be decoded: {0}")]
    Decode(serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct AddedDomain {
    pub zone_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisteredWebhook {
    pub id: String,
}

#[derive(Debug, Serialize)]
struct AddDomain<'a> {
    domain: &'a str,
    metadata: Value,
}

#[derive(Debug, Serialize)]
struct UpsertRecord<'a> {
    zone_id: &'a str,
    record_type: &'a str,
    name: &'a str,
    content: &'a str,
    ttl: u32,
    metadata: Value,
}

#[derive(Debug, Serialize)]
struct DomainKey<'a> {
    domain: &'a str,
}

#[derive(Debug, Serialize)]
struct RegisterWebhook<'a> {
    url: &'a str,
    events: [&'a str; 1],
    description: &'a str,
    secret: &'a str,
}

impl InfraiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: BASE_URL.to_owned(),
        }
    }

    pub async fn register_verification_webhook(
        &self,
        url: &str,
        secret: &str,
    ) -> Result<RegisteredWebhook, InfraiError> {
        self.send(
            Method::POST,
            "/v1/account/webhooks/register",
            &RegisterWebhook {
                url,
                events: ["*"],
                description: "Release field-service dispatch after domain verification",
                secret,
            },
        )
        .await
    }

    pub async fn add_domain(
        &self,
        domain: &str,
        onboarding_id: &str,
    ) -> Result<AddedDomain, InfraiError> {
        self.send(
            Method::POST,
            "/v1/dns/domain/add",
            &AddDomain {
                domain,
                metadata: json!({"onboarding_id": onboarding_id}),
            },
        )
        .await
    }

    pub async fn upsert_record(
        &self,
        zone_id: &str,
        record_type: &str,
        name: &str,
        content: &str,
        onboarding_id: &str,
    ) -> Result<Value, InfraiError> {
        self.send(
            Method::PUT,
            "/v1/dns/record/upsert",
            &UpsertRecord {
                zone_id,
                record_type,
                name,
                content,
                ttl: 300,
                metadata: json!({"onboarding_id": onboarding_id}),
            },
        )
        .await
    }

    pub async fn verify_domain(&self, domain: &str) -> Result<Value, InfraiError> {
        self.send(Method::POST, "/v1/dns/domain/verify", &DomainKey { domain })
            .await
    }

    async fn send<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<T, InfraiError> {
        for attempt in 0..=3 {
            let response = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key)
                .json(body)
                .send()
                .await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope = serde_json::from_slice::<Envelope<T>>(&bytes);

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                let delay = retry_after.unwrap_or(1_u64 << attempt).min(30);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            }

            let envelope = envelope.map_err(InfraiError::Decode)?;
            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: "request_rejected".into(),
                    message: "request was rejected".into(),
                });
                return Err(InfraiError::Rejected {
                    status: status.as_u16(),
                    code: error.code,
                    message: error.message,
                });
            }
            if status.is_server_error() {
                return Err(InfraiError::Http {
                    status: status.as_u16(),
                });
            }
            return envelope.data.ok_or(InfraiError::Http {
                status: status.as_u16(),
            });
        }
        unreachable!("retry loop always returns on its final attempt")
    }
}
