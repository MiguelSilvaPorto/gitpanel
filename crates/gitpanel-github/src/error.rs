use chrono::{DateTime, TimeDelta, Utc};
use reqwest::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GithubError {
    #[error("gh was not found in PATH")]
    GhMissing,
    #[error("could not execute gh: {0}")]
    GhCommand(String),
    #[error("gh is not authenticated: {details}")]
    GhNotLoggedIn { details: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("GitHub authentication failed: {message}")]
    AuthInvalid { message: String },
    #[error("GitHub authorization failed: {message}")]
    Forbidden { message: String },
    #[error("GitHub resource was not found: {message}")]
    NotFound { message: String },
    #[error("GitHub rate limit reached: {message}")]
    RateLimited {
        message: String,
        reset_at: DateTime<Utc>,
    },
    #[error("GitHub validation failed: {message}")]
    Validation { message: String, details: String },
    #[error("GitHub GraphQL error: {message}")]
    GraphQl { message: String },
    #[error("could not decode GitHub response ({context}): {details}")]
    Decode { context: String, details: String },
    #[error("GitHub returned HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        request_id: Option<String>,
    },
    #[error("the gh token is missing the required scope {scope}")]
    ScopeMissing { scope: String },
}

impl GithubError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::GhMissing => "error-gh-missing",
            Self::GhCommand(_) => "error-gh-not-logged-in",
            Self::GhNotLoggedIn { .. } => "error-gh-not-logged-in",
            Self::Network(_) => "error-network",
            Self::AuthInvalid { .. } => "error-auth",
            Self::Forbidden { .. } => "error-forbidden",
            Self::NotFound { .. } => "error-not-found",
            Self::RateLimited { .. } => "error-rate-limit",
            Self::Validation { .. } => "error-validation",
            Self::GraphQl { .. } => "error-graphql",
            Self::Decode { .. } => "error-decode",
            Self::Http { .. } => "error-generic",
            Self::ScopeMissing { .. } => "error-scope-missing",
        }
    }

    pub(crate) fn from_http(
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
        body: &str,
    ) -> Self {
        let api_error = serde_json::from_str::<ApiError>(body).ok();
        let message = api_error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| body.trim().to_owned());
        let details = api_error
            .and_then(|error| error.errors)
            .map(|errors| errors.to_string())
            .unwrap_or_default();
        let request_id = headers
            .get("x-github-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        match status.as_u16() {
            401 => Self::AuthInvalid { message },
            403 => {
                let remaining = headers
                    .get("x-ratelimit-remaining")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u32>().ok());
                let lower = message.to_ascii_lowercase();
                if remaining == Some(0) || lower.contains("rate limit") {
                    let reset_at = headers
                        .get("x-ratelimit-reset")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<i64>().ok())
                        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
                        .unwrap_or_else(|| Utc::now() + TimeDelta::minutes(1));
                    Self::RateLimited { message, reset_at }
                } else {
                    Self::Forbidden { message }
                }
            }
            404 => Self::NotFound { message },
            422 => Self::Validation { message, details },
            _ => Self::Http {
                status: status.as_u16(),
                message,
                request_id,
            },
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }
}

#[derive(serde::Deserialize)]
struct ApiError {
    message: String,
    #[serde(default)]
    errors: Option<serde_json::Value>,
}
