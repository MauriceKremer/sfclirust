use serde::Deserialize;
use thiserror::Error;

use crate::{ApiError, RetryAfter, StatusCode};

const SAFE_ERROR_CODES: &[&str] = &[
    "INSUFFICIENT_ACCESS",
    "INSUFFICIENT_ACCESS_OR_READONLY",
    "INSUFFICIENT_PRIVILEGES",
    "INVALID_FIELD",
    "INVALID_QUERY_FILTER_OPERATOR",
    "INVALID_SESSION_ID",
    "INVALID_TYPE",
    "MALFORMED_QUERY",
    "QUERY_TIMEOUT",
    "REQUEST_LIMIT_EXCEEDED",
];

/// A safe category for a Salesforce REST API failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalesforceErrorCategory {
    /// The access token is missing, expired, or invalid.
    Authentication,
    /// The authenticated user lacks permission for the operation.
    Authorization,
    /// The SOQL query is invalid.
    InvalidQuery,
    /// Salesforce stopped the query because it exceeded its execution limit.
    QueryTimeout,
    /// The org or API has denied the request due to limits.
    RateLimited,
    /// The requested Salesforce resource was not found.
    NotFound,
    /// The request conflicts with current Salesforce state.
    Conflict,
    /// Salesforce returned a server-side failure.
    Server,
    /// Salesforce rejected the request input.
    Validation,
    /// The failure did not match a known safe category.
    Other,
}

/// A sanitized Salesforce error with safe diagnostic and retry metadata.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("Salesforce API request failed ({category:?})")]
pub struct SalesforceApiError {
    status: StatusCode,
    category: SalesforceErrorCategory,
    error_codes: Vec<String>,
    request_id: Option<String>,
    retry_after: Option<RetryAfter>,
}

impl SalesforceApiError {
    pub(crate) fn from_response(
        status: StatusCode,
        body: &[u8],
        request_id: Option<&str>,
        retry_after: Option<RetryAfter>,
    ) -> Self {
        let error_codes = parse_error_codes(body);
        let category = classify_error(status, &error_codes);
        Self {
            status,
            category,
            error_codes,
            request_id: request_id.map(str::to_owned),
            retry_after,
        }
    }

    /// Returns the HTTP status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the safe failure category.
    pub fn category(&self) -> SalesforceErrorCategory {
        self.category
    }

    /// Returns validated Salesforce error codes, without error messages or bodies.
    pub fn error_codes(&self) -> &[String] {
        &self.error_codes
    }

    /// Returns a validated request identifier, if the server provided one.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Returns parsed delta-seconds or HTTP-date `Retry-After` guidance.
    pub fn retry_after(&self) -> Option<RetryAfter> {
        self.retry_after
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorPayload {
    Multiple(Vec<WireError>),
    Single(WireError),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireError {
    error_code: Option<String>,
}

fn parse_error_codes(body: &[u8]) -> Vec<String> {
    let payload: ErrorPayload = match serde_json::from_slice(body) {
        Ok(payload) => payload,
        Err(_) => return Vec::new(),
    };
    let wire_errors = match payload {
        ErrorPayload::Multiple(errors) => errors,
        ErrorPayload::Single(error) => vec![error],
    };
    wire_errors
        .into_iter()
        .filter_map(|error| error.error_code)
        .filter_map(sanitize_error_code)
        .collect()
}

fn sanitize_error_code(value: String) -> Option<String> {
    if value.is_empty()
        || value.len() > 64
        || !SAFE_ERROR_CODES.contains(&value.as_str())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    Some(value)
}

fn classify_error(status: StatusCode, codes: &[String]) -> SalesforceErrorCategory {
    if status == StatusCode::UNAUTHORIZED || codes.iter().any(|code| code == "INVALID_SESSION_ID") {
        SalesforceErrorCategory::Authentication
    } else if status == StatusCode::TOO_MANY_REQUESTS
        || codes.iter().any(|code| code == "REQUEST_LIMIT_EXCEEDED")
    {
        SalesforceErrorCategory::RateLimited
    } else if status == StatusCode::FORBIDDEN
        || codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "INSUFFICIENT_ACCESS"
                    | "INSUFFICIENT_ACCESS_OR_READONLY"
                    | "INSUFFICIENT_PRIVILEGES"
            )
        })
    {
        SalesforceErrorCategory::Authorization
    } else if status == StatusCode::NOT_FOUND {
        SalesforceErrorCategory::NotFound
    } else if status == StatusCode::CONFLICT {
        SalesforceErrorCategory::Conflict
    } else if status.is_server_error() {
        SalesforceErrorCategory::Server
    } else if codes.iter().any(|code| code == "QUERY_TIMEOUT") {
        SalesforceErrorCategory::QueryTimeout
    } else if codes.iter().any(|code| {
        matches!(
            code.as_str(),
            "INVALID_FIELD" | "INVALID_TYPE" | "MALFORMED_QUERY" | "INVALID_QUERY_FILTER_OPERATOR"
        )
    }) {
        SalesforceErrorCategory::InvalidQuery
    } else if status == StatusCode::BAD_REQUEST {
        SalesforceErrorCategory::Validation
    } else {
        SalesforceErrorCategory::Other
    }
}

/// Safe errors returned by Salesforce REST workflow operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RestError {
    /// The authenticated transport failed.
    #[error(transparent)]
    Transport(#[from] ApiError),
    /// The supplied API version is not a numeric major/minor pair.
    #[error("invalid Salesforce API version")]
    InvalidApiVersion,
    /// The SOQL query is empty.
    #[error("SOQL query must not be empty")]
    EmptyQuery,
    /// The SOQL query exceeds the supported request-size limit.
    #[error("SOQL query exceeds the supported size limit")]
    QueryTooLarge,
    /// A successful response was not a valid query result.
    #[error("Salesforce query response is malformed")]
    MalformedResponse,
    /// Salesforce returned a structured, sanitized REST error.
    #[error(transparent)]
    SalesforceApi(SalesforceApiError),
    /// A page indicates more data but lacks a pagination URL.
    #[error("Salesforce query response has incomplete pagination metadata")]
    IncompleteQuery,
    /// A page locator is not a same-origin URL for the selected query version.
    #[error("Salesforce query pagination URL is invalid")]
    InvalidPaginationLink,
    /// Salesforce returned a locator more than once.
    #[error("Salesforce query pagination cycle detected")]
    PaginationCycle,
    /// The query exceeded the configured record limit.
    #[error("Salesforce query exceeded the configured limit of {max_records} records")]
    QueryLimitExceeded { max_records: usize },
    /// Salesforce returned inconsistent query completion metadata.
    #[error("Salesforce query response has inconsistent result metadata")]
    InvalidQueryResult,
    /// The query exceeded the configured aggregate response limit.
    #[error("Salesforce query exceeded the configured limit of {max_bytes} response bytes")]
    ResponseLimitExceeded { max_bytes: usize },
    /// The query exceeded the bounded page count.
    #[error("Salesforce query exceeded the configured page limit")]
    TooManyPages,
    /// The requested resource limits are outside supported bounds.
    #[error("invalid Salesforce query resource limits")]
    InvalidLimits,
}
