use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct RelayClient {
    base_url: String,
    http: Client,
}

#[derive(Clone, Debug, Serialize)]
pub struct RegisterUserRequest {
    pub username: String,
    pub identity_key: String,
    pub signed_prekey: String,
    pub signed_prekey_signature: String,
    pub inbox_id: String,
    pub one_time_prekeys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegisterUserResponse {
    pub id: String,
    pub username: String,
    pub inbox_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RelayBundle {
    pub username: String,
    pub identity_key: String,
    pub signed_prekey: String,
    pub signed_prekey_signature: String,
    pub inbox_id: String,
    pub prekey_id: Option<String>,
    pub one_time_prekey: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPrekeysRequest {
    pub one_time_prekeys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AddPrekeysResponse {
    pub count: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PostMessageRequest {
    pub inbox_id: String,
    pub kind: String,
    pub header: String,
    pub ciphertext: String,
    pub used_prekey_id: Option<String>,
    pub client_message_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PostMessageResponse {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RelayMessage {
    pub id: String,
    pub inbox_id: String,
    pub kind: String,
    pub header: String,
    pub ciphertext: String,
    pub used_prekey_id: Option<String>,
    pub client_message_id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ErrorEnvelope {
    error: String,
}

impl RelayClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("failed to build relay http client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn register_user(&self, request: &RegisterUserRequest) -> Result<RegisterUserResponse> {
        self.handle_response(
            self.http
                .post(format!("{}/users", self.base_url))
                .json(request)
                .send()
                .context("failed to call POST /users")?,
        )
    }

    pub fn get_bundle(&self, username: &str) -> Result<RelayBundle> {
        self.handle_response(
            self.http
                .get(format!("{}/users/{username}/bundle", self.base_url))
                .send()
                .with_context(|| format!("failed to fetch bundle for {username}"))?,
        )
    }

    pub fn add_prekeys(&self, username: &str, prekeys: &[String]) -> Result<AddPrekeysResponse> {
        self.handle_response(
            self.http
                .post(format!("{}/users/{username}/prekeys", self.base_url))
                .json(&AddPrekeysRequest {
                    one_time_prekeys: prekeys.to_vec(),
                })
                .send()
                .with_context(|| format!("failed to upload prekeys for {username}"))?,
        )
    }

    pub fn post_message(&self, request: &PostMessageRequest) -> Result<PostMessageResponse> {
        self.handle_response(
            self.http
                .post(format!("{}/messages", self.base_url))
                .json(request)
                .send()
                .context("failed to call POST /messages")?,
        )
    }

    pub fn list_messages(&self, inbox_id: &str, limit: usize) -> Result<Vec<RelayMessage>> {
        self.handle_response(
            self.http
                .get(format!("{}/messages", self.base_url))
                .query(&[("inbox_id", inbox_id), ("limit", &limit.to_string())])
                .send()
                .with_context(|| format!("failed to list messages for inbox {inbox_id}"))?,
        )
    }

    fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::blocking::Response,
    ) -> Result<T> {
        if response.status().is_success() {
            return response
                .json()
                .context("failed to decode relay response body");
        }

        let status = response.status();
        let body = response.text().unwrap_or_default();
        let error = serde_json::from_str::<ErrorEnvelope>(&body)
            .map(|parsed| parsed.error)
            .ok()
            .or_else(|| {
                let trimmed = body.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .unwrap_or_else(|| format!("relay request failed with status {status}"));
        bail!("{status}: {error}")
    }
}

pub fn ensure_server_health(base_url: &str) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("failed to build health-check client")?;
    let response = client
        .get(format!("{}/health", base_url.trim_end_matches('/')))
        .send()
        .context("failed to call relay health endpoint")?;
    if !response.status().is_success() {
        return Err(anyhow!("relay health-check returned {}", response.status()));
    }
    Ok(())
}
