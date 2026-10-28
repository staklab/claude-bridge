use super::auth::{Auth, http_client, read_json};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(super) struct ResponseInfo {
    pub status: StatusCode,
    pub content_type: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Clone)]
pub(super) struct ResponseContext {
    pub info: ResponseInfo,
    token: String,
    redactions: Vec<String>,
}

impl ResponseContext {
    fn new(response: &reqwest::Response, token: &str, body: Option<&Value>) -> Self {
        let mut redactions = Vec::new();
        if let Some(body) = body {
            redactions.push(body.to_string());
            collect_request_text(body, &mut redactions);
        }
        redactions.retain(|s| !s.is_empty());
        redactions.sort_by_key(|s| std::cmp::Reverse(s.len()));
        redactions.dedup();
        let mut context = Self {
            info: ResponseInfo {
                status: response.status(),
                content_type: None,
                request_id: None,
            },
            token: token.to_string(),
            redactions,
        };
        context.info.content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| context.text(v, 256, true));
        context.info.request_id = ["x-request-id", "x-oai-request-id", "x-openai-request-id"]
            .iter()
            .find_map(|key| response.headers().get(*key).and_then(|v| v.to_str().ok()))
            .map(|v| context.text(v, 256, true));
        context
    }

    pub fn from_response(response: &reqwest::Response) -> Self {
        response
            .extensions()
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| Self::new(response, "", None))
    }

    fn text(&self, value: &str, limit: usize, redact_request: bool) -> String {
        static CREDENTIAL: std::sync::LazyLock<regex_lite::Regex> =
            std::sync::LazyLock::new(|| {
                regex_lite::Regex::new(
                    r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*|\b(?:sk|sess)-[A-Za-z0-9_-]+",
                )
                .expect("credential redaction pattern")
            });
        let mut safe = value.to_string();
        if !self.token.is_empty() {
            safe = safe.replace(&self.token, "[REDACTED]");
        }
        if redact_request {
            for secret in &self.redactions {
                safe = safe.replace(secret, "[REDACTED]");
            }
        }
        CREDENTIAL
            .replace_all(&safe, "[REDACTED]")
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(limit)
            .collect()
    }

    pub fn sanitize(&self, value: Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.text(&s, 4096, true)),
            Value::Array(values) => {
                Value::Array(values.into_iter().map(|v| self.sanitize(v)).collect())
            }
            Value::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(k, v)| {
                        let sensitive = matches!(
                            k.to_ascii_lowercase().as_str(),
                            "authorization"
                                | "access_token"
                                | "refresh_token"
                                | "id_token"
                                | "api_key"
                                | "request"
                                | "request_body"
                                | "headers"
                                | "input"
                                | "messages"
                                | "instructions"
                                | "prompt"
                        );
                        let identifier = matches!(k.as_str(), "code" | "param" | "type");
                        (
                            k,
                            if sensitive {
                                json!("[REDACTED]")
                            } else if identifier && let Some(text) = v.as_str() {
                                // A prompt may itself discuss an API error code or parameter.
                                // Preserve machine identifiers while still removing credentials.
                                json!(self.text(text, 4096, false))
                            } else {
                                self.sanitize(v)
                            },
                        )
                    })
                    .collect(),
            ),
            value => value,
        }
    }

    pub fn http_error(&self, body: Value) -> ApiError {
        ApiError {
            status: self.info.status,
            body: self.sanitize(body),
            request_id: self.info.request_id.clone(),
            retry_after: None,
            upstream: Some(Box::new(self.info.clone())),
        }
    }

    pub fn unexpected_type(&self, body: Value) -> ApiError {
        let mut error = ApiError::local(
            StatusCode::BAD_GATEWAY,
            "siwc_unexpected_content_type",
            "Responses API returned a successful HTTP status without a valid SSE stream",
        );
        error.body["upstream_error"] = self.sanitize(body);
        error.with_upstream(&self.info)
    }
}

fn collect_request_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if !matches!(
                    key.as_str(),
                    "model" | "type" | "role" | "name" | "call_id" | "id"
                ) {
                    collect_request_text(value, out);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_request_text(value, out);
            }
        }
        Value::String(text) if text.len() >= 8 => out.push(text.clone()),
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub(super) struct ApiError {
    pub status: StatusCode,
    pub body: Value,
    pub request_id: Option<String>,
    pub retry_after: Option<String>,
    pub upstream: Option<Box<ResponseInfo>>,
}

impl ApiError {
    pub fn local(status: StatusCode, code: &str, message: &str) -> Self {
        Self {
            status,
            body: json!({"error":{"code":code,"message":message}}),
            request_id: None,
            retry_after: None,
            upstream: None,
        }
    }
    pub fn with_upstream(mut self, info: &ResponseInfo) -> Self {
        self.request_id = info.request_id.clone();
        self.upstream = Some(Box::new(info.clone()));
        self
    }
    pub fn diagnostics(&self) -> Value {
        let code = self
            .body
            .pointer("/upstream_error/error/code")
            .or_else(|| self.body.pointer("/error/code"))
            .and_then(Value::as_str);
        let safe_code = code.filter(|s| {
            s.len() <= 128
                && !s.starts_with("sk-")
                && s.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
                })
        });
        json!({"upstream_status":self.upstream.as_ref().map(|u| u.status.as_u16()),
            "upstream_content_type":self.upstream.as_ref().and_then(|u| u.content_type.as_deref()),
            "upstream_request_id":self.request_id,"upstream_error_code":safe_code,
            "proxy_status":self.status.as_u16()})
    }
    pub fn report(&self) {
        crate::logging::create_logger("siwc").warn(
            "siwc_upstream_error",
            self.diagnostics().as_object().cloned(),
        );
    }
    pub fn error_type(&self) -> &'static str {
        match self.status.as_u16() {
            400 | 404 | 422 => "invalid_request_error",
            401 => "authentication_error",
            403 => "permission_error",
            429 => "rate_limit_error",
            503 => "overloaded_error",
            _ => "api_error",
        }
    }
    pub fn envelope(&self) -> Value {
        let message = self
            .body
            .pointer("/error/message")
            .or_else(|| self.body.get("detail"))
            .and_then(Value::as_str)
            .unwrap_or("SIWC upstream request failed");
        let mut envelope = self.diagnostics();
        envelope["type"] = json!("error");
        envelope["error"] = json!({"type":self.error_type(),"message":message,"code":self.body.pointer("/error/code"),"param":self.body.pointer("/error/param")});
        envelope["upstream_body"] = self.body.clone();
        envelope
    }
    pub fn response(&self) -> Response {
        let mut response = (self.status, Json(self.envelope())).into_response();
        for (key, value) in [
            ("x-openai-request-id", &self.request_id),
            ("retry-after", &self.retry_after),
        ] {
            if let Some(value) = value
                && let Ok(value) = value.parse()
            {
                response.headers_mut().insert(key, value);
            }
        }
        response
    }
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately exclude upstream text from CLI diagnostics and logs.
        write!(
            f,
            "SIWC HTTP {} ({})",
            self.status.as_u16(),
            self.diagnostics()
                .get("upstream_error_code")
                .and_then(Value::as_str)
                .unwrap_or("upstream_error")
        )
    }
}
impl std::error::Error for ApiError {}

#[derive(Clone)]
pub(super) struct Client {
    pub auth: Auth,
    http: reqwest::Client,
    pub usage_pause: std::sync::Arc<std::sync::Mutex<Option<ApiError>>>,
}

impl Client {
    pub fn new(auth: Auth) -> Self {
        Self {
            auth,
            http: http_client(),
            usage_pause: Default::default(),
        }
    }
    async fn request(&self, body: Option<&Value>) -> Result<reqwest::Response, ApiError> {
        if body.is_some()
            && let Some(error) = self.usage_pause.lock().expect("usage pause").clone()
        {
            return Err(error);
        }
        let token = self.auth.access_token().await.map_err(|e| {
            ApiError::local(
                StatusCode::UNAUTHORIZED,
                "siwc_authorization_required",
                &e.to_string(),
            )
        })?;
        for attempt in 0..3 {
            if body.is_some()
                && let Some(error) = self.usage_pause.lock().expect("usage pause").clone()
            {
                return Err(error);
            }
            if !self.auth.session_active().await.unwrap_or(false) {
                return Err(ApiError::local(
                    StatusCode::UNAUTHORIZED,
                    "siwc_signed_out",
                    "SIWC account signed out or permission removed",
                ));
            }
            let request = if let Some(body) = body {
                self.http
                    .post(format!("{}/responses", self.auth.endpoints.api))
                    .header("accept", "text/event-stream")
                    .json(body)
            } else {
                self.http.get(format!("{}/models", self.auth.endpoints.api))
            };
            let result =
                tokio::time::timeout(Duration::from_secs(60), request.bearer_auth(&token).send())
                    .await;
            let mut response = result.ok().and_then(Result::ok).ok_or_else(|| {
                ApiError::local(
                    StatusCode::BAD_GATEWAY,
                    "siwc_network_error",
                    "SIWC request failed or timed out before headers; no automatic replay",
                )
            })?;
            let context = ResponseContext::new(&response, &token, body);
            if response.status().is_success() {
                response.extensions_mut().insert(context);
                return Ok(response);
            }
            let status = response.status();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let raw = tokio::time::timeout(Duration::from_secs(15), read_json(response))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(
                    || json!({"detail":"Upstream returned an unreadable error response"}),
                );
            let mut error = context.http_error(raw);
            error.retry_after = retry_after;
            error.report();
            if error.body.pointer("/error/code").and_then(Value::as_str)
                == Some("subscription_sharing_usage_limit_exceeded")
            {
                *self.usage_pause.lock().expect("usage pause") = Some(error.clone());
            }
            let code = error
                .body
                .pointer("/error/code")
                .and_then(Value::as_str)
                .unwrap_or("");
            let temporary = status == StatusCode::SERVICE_UNAVAILABLE
                && matches!(
                    code,
                    "" | "subscription_sharing_usage_unavailable"
                        | "subscription_sharing_user_unavailable"
                );
            if !temporary || attempt == 2 {
                return Err(error);
            }
            let delay = error
                .retry_after
                .as_deref()
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_millis(250 * (1 << attempt)));
            if delay > Duration::from_secs(5) {
                return Err(error);
            }
            tokio::time::sleep(delay).await;
        }
        unreachable!("bounded request loop")
    }
    pub async fn responses(&self, body: &Value) -> Result<reqwest::Response, ApiError> {
        self.request(Some(body)).await
    }
    pub async fn models(&self) -> Result<Vec<Value>, ApiError> {
        let response = self.request(None).await?;
        let raw = tokio::time::timeout(Duration::from_secs(20), read_json(response))
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| {
                ApiError::local(
                    StatusCode::BAD_GATEWAY,
                    "invalid_model_catalog",
                    "Could not read SIWC model catalog",
                )
            })?;
        let models = raw["models"].as_array().ok_or_else(|| {
            ApiError::local(
                StatusCode::BAD_GATEWAY,
                "invalid_model_catalog",
                "SIWC model catalog has no models array",
            )
        })?;
        models
            .iter()
            .filter(|m| m["visibility"] == "list")
            .map(|m| {
                let slug = m["slug"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        ApiError::local(
                            StatusCode::BAD_GATEWAY,
                            "invalid_model_catalog",
                            "SIWC model entry has no slug",
                        )
                    })?;
                let display = m["display_name"].as_str().unwrap_or(slug);
                Ok(json!({"type":"model","object":"model","id":slug,"display_name":display}))
            })
            .collect()
    }
}
