mod error;
mod query;

pub use error::{RestError, SalesforceApiError, SalesforceErrorCategory};
pub use query::{ApiVersion, QueryPage, QueryRequest, QueryResult, RestClient};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use reqwest::StatusCode;
    use serde_json::Value;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    use crate::{
        BearerToken, HttpClient, InstanceOrigin, RetryAfter, TransportSettings,
        rest::{
            ApiVersion, QueryPage, QueryRequest, QueryResult, RestClient, RestError,
            SalesforceErrorCategory,
        },
    };

    fn query_request(soql: &str) -> QueryRequest {
        QueryRequest::new(ApiVersion::new("62.0").expect("API version"), soql)
            .expect("valid SOQL request")
    }

    #[tokio::test]
    async fn queries_the_explicit_version_and_decodes_records() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let soql = "SELECT Id, Name FROM Account WHERE Name = 'Acme'";
        let request = QueryRequest::new(ApiVersion::new("62.0").expect("API version"), soql)
            .expect("typed query request");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .and(query_param("q", soql))
            .and(header("authorization", "Bearer unit-test-token"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 1,
                    "done": true,
                    "records": [
                        {
                            "attributes": {
                                "type": "Account",
                                "url": "/services/data/v62.0/sobjects/Account/001000000000001"
                            },
                            "Id": "001000000000001",
                            "Name": "Acme"
                        }
                    ]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result: QueryResult<Value> = client
            .query_all(&origin, &token, request)
            .await
            .expect("SOQL query succeeds");

        assert!(!format!("{result:?}").contains("001000000000001"));
        assert_eq!(result.total_size(), 1);
        assert_eq!(result.records().len(), 1);
        assert_eq!(result.records()[0]["Id"], "001000000000001");
        assert_eq!(result.records()[0]["Name"], "Acme");
    }

    #[tokio::test]
    async fn collects_multiple_pages_in_server_order() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let soql = "SELECT Id FROM Account";

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .and(query_param("q", soql))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 3,
                    "done": false,
                    "records": [
                        {"Id": "001"},
                        {"Id": "002"}
                    ],
                    "nextRecordsUrl": "/services/data/v62.0/query/locator-2000"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query/locator-2000"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 3,
                    "done": false,
                    "records": [],
                    "nextRecordsUrl": "/services/data/v62.0/query/locator-4000"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query/locator-4000"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 3,
                    "done": true,
                    "records": [{"Id": "003"}]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result: QueryResult<Value> = client
            .query_all(&origin, &token, query_request(soql))
            .await
            .expect("all query pages");

        assert_eq!(
            result
                .records()
                .iter()
                .map(|record| record["Id"].as_str().expect("record Id"))
                .collect::<Vec<_>>(),
            ["001", "002", "003"]
        );
        assert_eq!(result.total_size(), 3);
    }

    #[tokio::test]
    async fn exposes_bounded_pages_for_incremental_consumers() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let version = ApiVersion::new("62.0").expect("API version");
        let soql = "SELECT Id FROM Account";
        let request = QueryRequest::new(version.clone(), soql).expect("typed query request");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .and(query_param("q", soql))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 2,
                    "done": false,
                    "records": [{"Id": "001"}],
                    "nextRecordsUrl": "/services/data/v62.0/query/locator"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query/locator"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 2,
                    "done": true,
                    "records": [{"Id": "002"}]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let first_page: QueryPage<Value> = client
            .query_page(&origin, &token, request)
            .await
            .expect("first page");
        assert!(!format!("{first_page:?}").contains("001"));
        assert!(!first_page.is_done());
        assert_eq!(first_page.records()[0]["Id"], "001");

        let next_url = first_page
            .next_records_url()
            .expect("first page has a next locator");
        let last_page: QueryPage<Value> = client
            .query_next_page(&origin, &token, version, next_url)
            .await
            .expect("next page");
        assert!(last_page.is_done());
        assert_eq!(last_page.records()[0]["Id"], "002");
    }

    #[tokio::test]
    async fn rejects_cross_origin_pagination_without_forwarding_credentials() {
        let server = MockServer::start().await;
        let second_origin_server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let soql = "SELECT Id FROM Account";
        let external_locator = format!(
            "{}/services/data/v62.0/query/external-locator",
            second_origin_server.uri()
        );

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .and(query_param("q", soql))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 1,
                    "done": false,
                    "records": [],
                    "nextRecordsUrl": external_locator
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request(soql))
            .await
            .expect_err("cross-origin locator must be rejected");

        assert_eq!(error, RestError::InvalidPaginationLink);
        assert!(
            second_origin_server
                .received_requests()
                .await
                .expect("request count")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn parses_safe_rate_limit_error_metadata() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let soql = "SELECT Id FROM Account";
        let sensitive_message = "query contains a private customer value";

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .and(query_param("q", soql))
            .respond_with(
                ResponseTemplate::new(StatusCode::FORBIDDEN)
                    .insert_header("x-sfdc-request-id", "req-123")
                    .insert_header("Retry-After", "17")
                    .set_body_json(serde_json::json!([{
                        "message": sensitive_message,
                        "errorCode": "REQUEST_LIMIT_EXCEEDED",
                        "fields": []
                    }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request(soql))
            .await
            .expect_err("rate limit response must be an error");
        let RestError::SalesforceApi(api_error) = error else {
            panic!("expected a structured Salesforce API error");
        };

        assert_eq!(api_error.category(), SalesforceErrorCategory::RateLimited);
        assert_eq!(api_error.status(), StatusCode::FORBIDDEN);
        assert_eq!(api_error.error_codes(), ["REQUEST_LIMIT_EXCEEDED"]);
        assert_eq!(api_error.request_id(), Some("req-123"));
        assert_eq!(
            api_error.retry_after(),
            Some(RetryAfter::Delay(std::time::Duration::from_secs(17)))
        );
        assert!(!api_error.to_string().contains(sensitive_message));
        assert!(!format!("{api_error:?}").contains(sensitive_message));
    }

    #[tokio::test]
    async fn classifies_malformed_soql_without_retaining_server_messages() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let sensitive_message = "unknown field PrivateCustomerSecret__c";

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::BAD_REQUEST).set_body_json(serde_json::json!([{
                    "message": sensitive_message,
                    "errorCode": "MALFORMED_QUERY",
                    "fields": []
                }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(
                &origin,
                &token,
                query_request("SELECT PrivateCustomerSecret__c FROM Account"),
            )
            .await
            .expect_err("invalid SOQL must fail");
        let RestError::SalesforceApi(api_error) = error else {
            panic!("expected a structured Salesforce API error");
        };

        assert_eq!(api_error.category(), SalesforceErrorCategory::InvalidQuery);
        assert_eq!(api_error.error_codes(), ["MALFORMED_QUERY"]);
        assert!(!api_error.to_string().contains(sensitive_message));
        assert!(!format!("{api_error:?}").contains(sensitive_message));
    }

    #[tokio::test]
    async fn classifies_query_timeouts_separately() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::BAD_REQUEST).set_body_json(serde_json::json!([{
                    "message": "query timed out",
                    "errorCode": "QUERY_TIMEOUT",
                    "fields": []
                }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("query timeout must fail");
        let RestError::SalesforceApi(api_error) = error else {
            panic!("expected a structured Salesforce API error");
        };

        assert_eq!(api_error.category(), SalesforceErrorCategory::QueryTimeout);
        assert_eq!(api_error.error_codes(), ["QUERY_TIMEOUT"]);
    }

    #[tokio::test]
    async fn returns_empty_query_results_without_fabricating_records() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 0,
                    "done": true,
                    "records": []
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result: QueryResult<Value> = client
            .query_all(
                &origin,
                &token,
                query_request("SELECT Id FROM Account WHERE Name = 'no match'"),
            )
            .await
            .expect("empty query succeeds");

        assert_eq!(result.total_size(), 0);
        assert!(result.records().is_empty());
    }

    #[tokio::test]
    async fn malformed_json_is_a_sanitized_query_error() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let private_body = "malformed private customer data";

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(ResponseTemplate::new(StatusCode::OK).set_body_string(private_body))
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("malformed JSON response must fail");

        assert_eq!(error, RestError::MalformedResponse);
        assert!(!error.to_string().contains(private_body));
        assert!(!format!("{error:?}").contains(private_body));
    }

    #[tokio::test]
    async fn refuses_to_return_a_silently_truncated_result() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client =
            RestClient::with_limits(transport, 1, 1024 * 1024).expect("bounded REST client");
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 2,
                    "done": true,
                    "records": [{"Id": "001"}, {"Id": "002"}]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("record limit must fail instead of truncating");

        assert_eq!(error, RestError::QueryLimitExceeded { max_records: 1 });
    }

    #[tokio::test]
    async fn bounds_aggregate_response_bytes() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::with_limits(transport, 100, 1).expect("bounded REST client");
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 0,
                    "done": true,
                    "records": []
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("response byte limit must be enforced");

        assert_eq!(error, RestError::ResponseLimitExceeded { max_bytes: 1 });
    }

    #[tokio::test]
    async fn rejects_a_pagination_link_for_another_api_version() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 1,
                    "done": false,
                    "records": [],
                    "nextRecordsUrl": "/services/data/v61.0/query/locator"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("wrong-version locator must be rejected");

        assert_eq!(error, RestError::InvalidPaginationLink);
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("request count")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn detects_a_repeated_pagination_locator() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 0,
                    "done": false,
                    "records": [],
                    "nextRecordsUrl": "/services/data/v62.0/query/locator"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query/locator"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 0,
                    "done": false,
                    "records": [],
                    "nextRecordsUrl": "/services/data/v62.0/query/locator"
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("repeated locator must fail");

        assert_eq!(error, RestError::PaginationCycle);
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("request count")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn rejects_inconsistent_query_totals() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::OK).set_body_json(serde_json::json!({
                    "totalSize": 2,
                    "done": true,
                    "records": [{"Id": "001"}]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("inconsistent result count must fail");

        assert_eq!(error, RestError::InvalidQueryResult);
    }

    #[tokio::test]
    async fn omits_unknown_server_error_codes_from_diagnostics() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let untrusted_code = "PRIVATE_CUSTOMER_123";

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::BAD_REQUEST).set_body_json(serde_json::json!([{
                    "message": "private body",
                    "errorCode": untrusted_code,
                    "fields": []
                }])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("server error response must fail");
        let RestError::SalesforceApi(api_error) = error else {
            panic!("expected a structured Salesforce API error");
        };

        assert_eq!(api_error.category(), SalesforceErrorCategory::Validation);
        assert!(api_error.error_codes().is_empty());
        assert!(!format!("{api_error:?}").contains(untrusted_code));
        assert!(!format!("{api_error:?}").contains("private body"));
    }

    #[test]
    fn api_versions_accept_only_numeric_major_minor_pairs() {
        assert_eq!(
            ApiVersion::new("062.00").expect("numeric version").as_str(),
            "62.0"
        );
        for invalid in ["", "v62.0", "62", "62.0.1", "+62.0", "62.0/path"] {
            assert_eq!(
                ApiVersion::new(invalid).expect_err("invalid version"),
                RestError::InvalidApiVersion
            );
        }
    }

    #[test]
    fn query_requests_bound_and_redact_soql() {
        let query = "SELECT CustomerSecret__c FROM Account WHERE Name = 'private'";
        let request = query_request(query);
        assert_eq!(request.api_version().as_str(), "62.0");
        assert_eq!(request.soql(), query);
        assert!(!format!("{request:?}").contains(query));
        assert_eq!(
            QueryRequest::new(ApiVersion::new("62.0").expect("API version"), " \t"),
            Err(RestError::EmptyQuery)
        );
        assert_eq!(
            QueryRequest::new(
                ApiVersion::new("62.0").expect("API version"),
                "q".repeat(100_001)
            ),
            Err(RestError::QueryTooLarge)
        );
    }

    #[tokio::test]
    async fn retains_http_date_retry_guidance() {
        let server = MockServer::start().await;
        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("loopback origin");
        let transport = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let client = RestClient::new(transport);
        let token = BearerToken::new("unit-test-token").expect("test token");
        let retry_header = httpdate::fmt_http_date(
            std::time::SystemTime::now() + std::time::Duration::from_secs(60),
        );
        let expected_retry_at =
            httpdate::parse_http_date(&retry_header).expect("well-formed HTTP date");

        Mock::given(method("GET"))
            .and(path("/services/data/v62.0/query"))
            .respond_with(
                ResponseTemplate::new(StatusCode::SERVICE_UNAVAILABLE)
                    .insert_header("Retry-After", retry_header)
                    .set_body_json(serde_json::json!([])),
            )
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .query_all::<Value>(&origin, &token, query_request("SELECT Id FROM Account"))
            .await
            .expect_err("server error response must fail");
        let RestError::SalesforceApi(api_error) = error else {
            panic!("expected a structured Salesforce API error");
        };

        assert_eq!(
            api_error.retry_after(),
            Some(RetryAfter::At(expected_retry_at))
        );
    }
}
