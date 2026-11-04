//! Metadata API `deploy` and `checkDeployStatus` wire operations.
//!
//! These types only build requests and decode responses. Transport, retries and
//! the decision about whether an unacknowledged submission may be repeated
//! belong to the caller.

use thiserror::Error;

use super::soap::{SoapError, SoapRequest, SoapResponse};
use super::xml::{XmlElement, XmlError, XmlLimits};

const REQUEST_NAMESPACE: &str = "urn:metadata.soap.sforce.com";

/// Test level requested for a deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestLevel {
    /// Run no tests (not allowed for production deployments by the server).
    NoTestRun,
    /// Run the tests of the deployed components.
    RunSpecifiedTests,
    /// Run all local tests.
    RunLocalTests,
    /// Run all tests in the org.
    RunAllTestsInOrg,
}

impl TestLevel {
    /// Returns the Metadata API enumeration value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTestRun => "NoTestRun",
            Self::RunSpecifiedTests => "RunSpecifiedTests",
            Self::RunLocalTests => "RunLocalTests",
            Self::RunAllTestsInOrg => "RunAllTestsInOrg",
        }
    }
}

/// Options sent with a deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeployOptions {
    /// Validate only; nothing is saved in the org.
    pub check_only: bool,
    /// Roll everything back when any component fails.
    pub rollback_on_error: bool,
    /// The archive contains a single package rooted at `package.xml`.
    pub single_package: bool,
    /// Requested test level.
    pub test_level: TestLevel,
}

impl Default for DeployOptions {
    fn default() -> Self {
        Self {
            check_only: false,
            rollback_on_error: true,
            single_package: true,
            test_level: TestLevel::NoTestRun,
        }
    }
}

/// Builds a `deploy` request carrying a base64 encoded zip archive.
pub fn deploy_request(
    zip_base64: &str,
    options: DeployOptions,
    session_id: &str,
) -> Result<SoapRequest, DeployWireError> {
    let flag = |value: bool| if value { "true" } else { "false" };
    let options_element = XmlElement::new(REQUEST_NAMESPACE, "DeployOptions")?
        .with_child(text("checkOnly", flag(options.check_only))?)
        .with_child(text("rollbackOnError", flag(options.rollback_on_error))?)
        .with_child(text("singlePackage", flag(options.single_package))?)
        .with_child(text("testLevel", options.test_level.as_str())?);
    let operation = XmlElement::new(REQUEST_NAMESPACE, "deploy")?
        .with_child(text("ZipFile", zip_base64)?)
        .with_child(options_element);
    Ok(SoapRequest::new(operation).with_session_id(session_id))
}

/// Builds a `checkDeployStatus` request for one deployment ID.
pub fn check_deploy_status_request(
    deploy_id: &DeployId,
    include_details: bool,
    session_id: &str,
) -> Result<SoapRequest, DeployWireError> {
    let operation = XmlElement::new(REQUEST_NAMESPACE, "checkDeployStatus")?
        .with_child(text("asyncProcessId", deploy_id.as_str())?)
        .with_child(text(
            "includeDetails",
            if include_details { "true" } else { "false" },
        )?);
    Ok(SoapRequest::new(operation).with_session_id(session_id))
}

fn text(name: &str, value: &str) -> Result<XmlElement, XmlError> {
    Ok(XmlElement::new(REQUEST_NAMESPACE, name)?.with_text(value))
}

/// A validated Salesforce deployment (AsyncResult) identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployId(String);

impl DeployId {
    /// Accepts a 15 or 18 character alphanumeric ID beginning with `0Af`.
    pub fn parse(value: &str) -> Result<Self, DeployWireError> {
        let valid = matches!(value.len(), 15 | 18)
            && value.starts_with("0Af")
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric());
        if valid {
            Ok(Self(value.to_string()))
        } else {
            Err(DeployWireError::InvalidDeployId)
        }
    }

    /// Returns the ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Server-side deployment status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeployStatus {
    /// Accepted and waiting to start.
    Pending,
    /// Running.
    InProgress,
    /// Completed and saved (or validated) without errors.
    Succeeded,
    /// Completed with some components failing.
    SucceededPartial,
    /// Finished with errors.
    Failed,
    /// Cancellation requested but not yet complete.
    Canceling,
    /// Cancelled.
    Canceled,
}

impl DeployStatus {
    fn parse(value: &str) -> Result<Self, DeployWireError> {
        Ok(match value {
            "Pending" => Self::Pending,
            "InProgress" => Self::InProgress,
            "Succeeded" => Self::Succeeded,
            "SucceededPartial" => Self::SucceededPartial,
            "Failed" => Self::Failed,
            "Canceling" => Self::Canceling,
            "Canceled" => Self::Canceled,
            _ => return Err(DeployWireError::Malformed("unknown deployment status")),
        })
    }

    /// Returns whether the deployment will not change any further.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::SucceededPartial | Self::Failed | Self::Canceled
        )
    }

    /// Returns the Metadata API name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "InProgress",
            Self::Succeeded => "Succeeded",
            Self::SucceededPartial => "SucceededPartial",
            Self::Failed => "Failed",
            Self::Canceling => "Canceling",
            Self::Canceled => "Canceled",
        }
    }
}

/// One failed component from the deployment details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentFailure {
    /// Metadata type name.
    pub component_type: String,
    /// Component full name.
    pub full_name: String,
    /// Source file name reported by the server.
    pub file_name: String,
    /// Problem text.
    pub problem: String,
    /// Problem type, usually `Error` or `Warning`.
    pub problem_type: String,
    /// One based line, when reported.
    pub line_number: Option<u32>,
    /// One based column, when reported.
    pub column_number: Option<u32>,
}

/// Decoded deployment state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployResult {
    /// Deployment ID.
    pub id: String,
    /// Server status.
    pub status: DeployStatus,
    /// Whether this was a validation-only run.
    pub check_only: bool,
    /// Components processed successfully so far.
    pub components_deployed: u32,
    /// Total components in the archive.
    pub components_total: u32,
    /// Components that failed.
    pub component_errors: u32,
    /// Top-level error text, if any.
    pub error_message: Option<String>,
    /// Failed components (only with details requested).
    pub failures: Vec<ComponentFailure>,
}

impl DeployResult {
    /// Decodes a `checkDeployStatusResponse` document.
    pub fn from_status_response(document: &[u8]) -> Result<Self, DeployWireError> {
        let response = SoapResponse::decode(document, XmlLimits::default())?;
        let result = first_result(&response, "checkDeployStatusResponse")?;
        let status = DeployStatus::parse(&child_text(result, "status").unwrap_or_default())?;
        let id = child_text(result, "id").ok_or(DeployWireError::Malformed("missing id"))?;
        let mut failures = Vec::new();
        if let Some(details) = result.children().find(|c| c.local_name() == "details") {
            for failure in details
                .children()
                .filter(|c| c.local_name() == "componentFailures")
            {
                failures.push(ComponentFailure {
                    component_type: child_text(failure, "componentType").unwrap_or_default(),
                    full_name: child_text(failure, "fullName").unwrap_or_default(),
                    file_name: child_text(failure, "fileName").unwrap_or_default(),
                    problem: child_text(failure, "problem").unwrap_or_default(),
                    problem_type: child_text(failure, "problemType").unwrap_or_default(),
                    line_number: child_text(failure, "lineNumber").and_then(|v| v.parse().ok()),
                    column_number: child_text(failure, "columnNumber").and_then(|v| v.parse().ok()),
                });
            }
        }
        Ok(Self {
            id,
            status,
            check_only: child_text(result, "checkOnly").as_deref() == Some("true"),
            components_deployed: count(result, "numberComponentsDeployed"),
            components_total: count(result, "numberComponentsTotal"),
            component_errors: count(result, "numberComponentErrors"),
            error_message: child_text(result, "errorMessage"),
            failures,
        })
    }
}

/// Decodes a `deployResponse` document and returns the accepted deployment ID.
pub fn parse_deploy_response(document: &[u8]) -> Result<DeployId, DeployWireError> {
    let response = SoapResponse::decode(document, XmlLimits::default())?;
    let result = first_result(&response, "deployResponse")?;
    let id = child_text(result, "id").ok_or(DeployWireError::Malformed("missing id"))?;
    DeployId::parse(&id)
}

fn first_result<'a>(
    response: &'a SoapResponse,
    wrapper: &'static str,
) -> Result<&'a XmlElement, DeployWireError> {
    response
        .body()
        .iter()
        .find(|element| element.local_name() == wrapper)
        .and_then(|element| element.children().find(|c| c.local_name() == "result"))
        .ok_or(DeployWireError::Malformed("missing result element"))
}

fn child_text(element: &XmlElement, name: &str) -> Option<String> {
    element
        .children()
        .find(|child| child.local_name() == name)
        .map(XmlElement::text_content)
        .filter(|value| !value.is_empty())
}

fn count(element: &XmlElement, name: &str) -> u32 {
    child_text(element, name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Errors building or decoding deployment wire messages.
#[derive(Debug, Error)]
pub enum DeployWireError {
    /// The ID is not a deployment ID.
    #[error("invalid deployment ID")]
    InvalidDeployId,
    /// The response was well formed SOAP but not the expected shape.
    #[error("malformed deployment response: {0}")]
    Malformed(&'static str),
    /// SOAP envelope or fault error.
    #[error(transparent)]
    Soap(#[from] SoapError),
    /// XML construction error.
    #[error(transparent)]
    Xml(#[from] XmlError),
}
