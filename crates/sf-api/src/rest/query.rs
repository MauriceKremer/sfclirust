use std::collections::HashSet;

use serde::{Deserialize, de::DeserializeOwned};

use crate::{ApiError, BearerToken, HttpClient, InstanceOrigin, Method, Url};

use super::{RestError, SalesforceApiError};

const DEFAULT_MAX_QUERY_RECORDS: usize = 50_000;
const DEFAULT_MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;
const MAX_QUERY_RECORDS: usize = 1_000_000;
const MAX_QUERY_BYTES: usize = 512 * 1024 * 1024;
const MAX_QUERY_PAGES: usize = 1_000;
const MAX_NEXT_URL_BYTES: usize = 4_096;
const MAX_SOQL_QUERY_BYTES: usize = 100_000;

/// A canonical Salesforce REST API version, such as `62.0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiVersion {
    value: String,
}

impl ApiVersion {
    /// Validates and canonicalizes a numeric major/minor API version.
    pub fn new(value: &str) -> Result<Self, RestError> {
        let mut components = value.split('.');
        let major = parse_version_component(components.next())?;
        let minor = parse_version_component(components.next())?;
        if components.next().is_some() || major == 0 {
            return Err(RestError::InvalidApiVersion);
        }

        Ok(Self {
            value: format!("{major}.{minor}"),
        })
    }

    /// Returns the canonical major/minor version string.
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

fn parse_version_component(component: Option<&str>) -> Result<u16, RestError> {
    let component = component.ok_or(RestError::InvalidApiVersion)?;
    if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RestError::InvalidApiVersion);
    }
    component
        .parse::<u16>()
        .map_err(|_| RestError::InvalidApiVersion)
}

/// A typed SOQL request with an explicit Salesforce API version.
#[derive(Clone, PartialEq, Eq)]
pub struct QueryRequest {
    version: ApiVersion,
    soql: String,
}

impl QueryRequest {
    /// Validates a query and binds it to the API version selected by its workflow.
    pub fn new(version: ApiVersion, soql: impl Into<String>) -> Result<Self, RestError> {
        let soql = soql.into();
        if soql.trim().is_empty() {
            return Err(RestError::EmptyQuery);
        }
        if soql.len() > MAX_SOQL_QUERY_BYTES {
            return Err(RestError::QueryTooLarge);
        }
        Ok(Self { version, soql })
    }

    /// Returns the API version selected for this query.
    pub fn api_version(&self) -> &ApiVersion {
        &self.version
    }

    /// Returns the SOQL text.
    pub fn soql(&self) -> &str {
        &self.soql
    }
}

impl std::fmt::Debug for QueryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryRequest")
            .field("api_version", &self.version)
            .field("soql", &"***")
            .finish()
    }
}

/// A complete SOQL query result, with records kept in Salesforce response order.
#[derive(PartialEq, Eq)]
pub struct QueryResult<T> {
    total_size: u64,
    records: Vec<T>,
}

impl<T> std::fmt::Debug for QueryResult<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryResult")
            .field("total_size", &self.total_size)
            .field("record_count", &self.records.len())
            .finish()
    }
}

impl<T> QueryResult<T> {
    /// Returns the server-reported number of matching records.
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Returns records in the order received from Salesforce.
    pub fn records(&self) -> &[T] {
        &self.records
    }

    /// Consumes the result and returns its records in response order.
    pub fn into_records(self) -> Vec<T> {
        self.records
    }
}

/// One bounded page from a SOQL query.
#[derive(PartialEq, Eq)]
pub struct QueryPage<T> {
    total_size: u64,
    done: bool,
    records: Vec<T>,
    next_records_url: Option<String>,
}

impl<T> std::fmt::Debug for QueryPage<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryPage")
            .field("total_size", &self.total_size)
            .field("done", &self.done)
            .field("record_count", &self.records.len())
            .field("has_next_page", &self.next_records_url.is_some())
            .finish()
    }
}

impl<T> QueryPage<T> {
    /// Returns the server-reported number of matching records.
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Returns whether Salesforce has returned every matching record.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Returns records in the order received from Salesforce.
    pub fn records(&self) -> &[T] {
        &self.records
    }

    /// Returns the untrusted next-page locator, if Salesforce provided one.
    ///
    /// Pass the locator to [`RestClient::query_next_page`] to validate it before
    /// making another authenticated request.
    pub fn next_records_url(&self) -> Option<&str> {
        self.next_records_url.as_deref()
    }
}

/// Executes authenticated Salesforce REST queries with an explicit API version.
pub struct RestClient {
    transport: HttpClient,
    max_records: usize,
    max_total_bytes: usize,
}

impl RestClient {
    /// Creates a REST client with bounded query aggregation defaults.
    pub fn new(transport: HttpClient) -> Self {
        Self {
            transport,
            max_records: DEFAULT_MAX_QUERY_RECORDS,
            max_total_bytes: DEFAULT_MAX_QUERY_BYTES,
        }
    }

    /// Creates a REST client with explicit aggregate query limits.
    pub fn with_limits(
        transport: HttpClient,
        max_records: usize,
        max_total_bytes: usize,
    ) -> Result<Self, RestError> {
        if max_records == 0
            || max_records > MAX_QUERY_RECORDS
            || max_total_bytes == 0
            || max_total_bytes > MAX_QUERY_BYTES
        {
            return Err(RestError::InvalidLimits);
        }
        Ok(Self {
            transport,
            max_records,
            max_total_bytes,
        })
    }

    /// Fetches one bounded page for a SOQL query.
    pub async fn query_page<T: DeserializeOwned>(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        request: QueryRequest,
    ) -> Result<QueryPage<T>, RestError> {
        let mut url = query_url(origin, &request.version)?;
        url.query_pairs_mut().append_pair("q", &request.soql);
        self.fetch_page(origin, token, &request.version, &url)
            .await
            .map(|(page, _)| page)
    }

    /// Fetches a next page only after validating its origin, path, and API version.
    pub async fn query_next_page<T: DeserializeOwned>(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        version: ApiVersion,
        next_records_url: &str,
    ) -> Result<QueryPage<T>, RestError> {
        let url = validate_next_url(origin, &version, next_records_url)?;
        self.fetch_page(origin, token, &version, &url)
            .await
            .map(|(page, _)| page)
    }

    /// Executes a SOQL query and collects its result pages within configured bounds.
    pub async fn query_all<T: DeserializeOwned>(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        request: QueryRequest,
    ) -> Result<QueryResult<T>, RestError> {
        let mut url = query_url(origin, &request.version)?;
        url.query_pairs_mut().append_pair("q", &request.soql);
        let (mut page, mut total_bytes) = self
            .fetch_page(origin, token, &request.version, &url)
            .await?;
        if total_bytes > self.max_total_bytes {
            return Err(RestError::ResponseLimitExceeded {
                max_bytes: self.max_total_bytes,
            });
        }
        let total_size = page.total_size;
        if total_size > self.max_records as u64 {
            return Err(RestError::QueryLimitExceeded {
                max_records: self.max_records,
            });
        }
        let mut records = Vec::new();
        let mut page_count = 0usize;
        let mut seen_next_urls = HashSet::new();

        loop {
            page_count += 1;
            if page_count > MAX_QUERY_PAGES {
                return Err(RestError::TooManyPages);
            }
            if records.len().saturating_add(page.records.len()) > self.max_records {
                return Err(RestError::QueryLimitExceeded {
                    max_records: self.max_records,
                });
            }
            records.extend(page.records);

            if page.done {
                if records.len() as u64 != total_size {
                    return Err(RestError::InvalidQueryResult);
                }
                return Ok(QueryResult {
                    total_size,
                    records,
                });
            }

            let next_records_url = page
                .next_records_url
                .as_deref()
                .ok_or(RestError::IncompleteQuery)?;
            let next_url = validate_next_url(origin, &request.version, next_records_url)?;
            if !seen_next_urls.insert(next_url.as_str().to_owned()) {
                return Err(RestError::PaginationCycle);
            }
            let (next_page, response_bytes) = self
                .fetch_page(origin, token, &request.version, &next_url)
                .await?;
            if next_page.total_size != total_size {
                return Err(RestError::InvalidQueryResult);
            }
            total_bytes = total_bytes.saturating_add(response_bytes);
            if total_bytes > self.max_total_bytes {
                return Err(RestError::ResponseLimitExceeded {
                    max_bytes: self.max_total_bytes,
                });
            }
            page = next_page;
        }
    }

    async fn fetch_page<T: DeserializeOwned>(
        &self,
        origin: &InstanceOrigin,
        token: &BearerToken<'_>,
        version: &ApiVersion,
        url: &Url,
    ) -> Result<(QueryPage<T>, usize), RestError> {
        let response = self
            .transport
            .execute(origin, token, Method::GET, url, None)
            .await?;
        if !response.status().is_success() {
            return Err(RestError::SalesforceApi(SalesforceApiError::from_response(
                response.status(),
                response.body(),
                response.request_id(),
                response.retry_after(),
            )));
        }
        let response_bytes = response.body().len();
        let page: WireQueryPage<T> = response.json().map_err(|error| match error {
            ApiError::InvalidJson => RestError::MalformedResponse,
            other => RestError::Transport(other),
        })?;
        if page.done && page.next_records_url.is_some()
            || !page.done && page.next_records_url.is_none()
        {
            return Err(RestError::IncompleteQuery);
        }
        if let Some(next_url) = &page.next_records_url {
            validate_next_url(origin, version, next_url)?;
        }
        Ok((
            QueryPage {
                total_size: page.total_size,
                done: page.done,
                records: page.records,
                next_records_url: page.next_records_url,
            },
            response_bytes,
        ))
    }
}

fn query_url(origin: &InstanceOrigin, version: &ApiVersion) -> Result<Url, RestError> {
    origin
        .request_url(&format!("/services/data/v{}/query", version.as_str()))
        .map_err(RestError::Transport)
}

fn validate_next_url(
    origin: &InstanceOrigin,
    version: &ApiVersion,
    value: &str,
) -> Result<Url, RestError> {
    if value.len() > MAX_NEXT_URL_BYTES {
        return Err(RestError::InvalidPaginationLink);
    }
    let url = origin
        .request_url(value)
        .map_err(|_| RestError::InvalidPaginationLink)?;
    let expected_prefix = format!("/services/data/v{}/query/", version.as_str());
    if !url.path().starts_with(&expected_prefix)
        || url.path().len() == expected_prefix.len()
        || url.query().is_some()
    {
        return Err(RestError::InvalidPaginationLink);
    }
    Ok(url)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireQueryPage<T> {
    total_size: u64,
    done: bool,
    records: Vec<T>,
    #[serde(default)]
    next_records_url: Option<String>,
}
