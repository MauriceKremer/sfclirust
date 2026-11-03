#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::time::Duration;

use sf_api::{ApiError, BearerToken, HttpClient, InstanceOrigin, TransportSettings};

#[test]
fn production_origins_require_https_and_salesforce_hosts() {
    let origin = InstanceOrigin::parse("https://na123.salesforce.com").expect("Salesforce origin");
    let request_url = origin
        .request_url("/services/data/v61.0/sobjects")
        .expect("same-origin request URL");

    assert_eq!(
        request_url.as_str(),
        "https://na123.salesforce.com/services/data/v61.0/sobjects"
    );
    assert!(matches!(
        InstanceOrigin::parse("http://na123.salesforce.com"),
        Err(ApiError::InvalidOrigin)
    ));
    assert!(matches!(
        InstanceOrigin::parse("https://attacker.example"),
        Err(ApiError::InvalidOrigin)
    ));
    assert!(matches!(
        InstanceOrigin::parse("https://user@na123.salesforce.com"),
        Err(ApiError::InvalidOrigin)
    ));
    assert!(matches!(
        InstanceOrigin::parse("https://na123.salesforce.com/custom"),
        Err(ApiError::InvalidOrigin)
    ));
    assert!(InstanceOrigin::parse("https://na123.salesforce.mil").is_ok());
    assert!(InstanceOrigin::parse("https://na123.force.com").is_ok());
}

#[test]
fn request_urls_cannot_escape_the_validated_origin() {
    let origin = InstanceOrigin::parse("https://na123.salesforce.com").expect("Salesforce origin");

    assert!(matches!(
        origin.request_url("https://other.salesforce.com/services/data"),
        Err(ApiError::OriginMismatch)
    ));
    assert!(matches!(
        origin.request_url("//other.salesforce.com/services/data"),
        Err(ApiError::OriginMismatch)
    ));
}

#[test]
fn bearer_token_rejects_empty_values_and_redacts_debug() {
    assert!(matches!(BearerToken::new(""), Err(ApiError::InvalidToken)));
    assert!(matches!(
        BearerToken::new("token\ninjected"),
        Err(ApiError::InvalidToken)
    ));
    assert!(matches!(
        BearerToken::new(&"t".repeat(16 * 1024 + 1)),
        Err(ApiError::InvalidToken)
    ));

    let token = BearerToken::new("top-secret-access-token").expect("valid bearer token");
    let debug = format!("{token:?}");
    assert!(!debug.contains("top-secret-access-token"));
    assert!(debug.contains("***"));
}

#[test]
fn client_uses_verified_tls_and_rejects_invalid_ca_configuration() {
    let client = HttpClient::new(TransportSettings::default()).expect("default rustls client");
    drop(client);

    let error = HttpClient::new(
        TransportSettings::default().add_root_certificate_pem(b"not a PEM certificate".to_vec()),
    )
    .err()
    .expect("invalid CA input must fail explicitly");
    assert!(matches!(error, ApiError::InvalidTlsConfiguration));
    assert!(!error.to_string().contains("not a PEM certificate"));
}

#[test]
fn transport_configuration_is_bounded_and_proxy_errors_are_sanitized() {
    let error = HttpClient::new(TransportSettings::default().with_request_timeout(Duration::ZERO))
        .err()
        .expect("zero timeout must be rejected");
    assert_eq!(error, ApiError::InvalidConfiguration);

    let error = HttpClient::new(TransportSettings::default().with_max_concurrent_requests(0))
        .err()
        .expect("zero concurrency must be rejected");
    assert_eq!(error, ApiError::InvalidConfiguration);

    let error = HttpClient::new(TransportSettings::default().with_proxy("not a valid proxy URL"))
        .err()
        .expect("invalid proxy must be rejected");
    assert_eq!(error, ApiError::InvalidProxy);
    assert!(!error.to_string().contains("not a valid proxy URL"));

    HttpClient::new(
        TransportSettings::default()
            .without_system_proxy()
            .with_proxy("http://127.0.0.1:8080"),
    )
    .expect("valid explicit proxy configuration");
}
