use std::fmt;

use thiserror::Error;

use super::xml::{XmlElement, XmlError, XmlLimits, encode_xml, parse_xml};

const SOAP_ENVELOPE_NAMESPACE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const SOAP12_ENVELOPE_NAMESPACE: &str = "http://www.w3.org/2003/05/soap-envelope";
const METADATA_NAMESPACE: &str = "urn:metadata.soap.sforce.com";

/// A typed Metadata SOAP request envelope.
pub struct SoapRequest {
    operation: XmlElement,
    session_id: Option<String>,
}

impl fmt::Debug for SoapRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SoapRequest")
            .field("operation", &self.operation)
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

impl SoapRequest {
    /// Creates a request for a typed operation element.
    pub fn new(operation: XmlElement) -> Self {
        Self {
            operation,
            session_id: None,
        }
    }

    /// Adds the authenticated Metadata API session header.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Encodes the request as a bounded SOAP 1.1 XML document.
    pub fn encode(&self) -> Result<Vec<u8>, SoapError> {
        let mut envelope = XmlElement::new(SOAP_ENVELOPE_NAMESPACE, "Envelope")?;
        if let Some(session_id) = &self.session_id {
            let session = XmlElement::new(METADATA_NAMESPACE, "SessionHeader")?.with_child(
                XmlElement::new(METADATA_NAMESPACE, "sessionId")?.with_text(session_id),
            );
            envelope = envelope.with_child(
                XmlElement::new(SOAP_ENVELOPE_NAMESPACE, "Header")?.with_child(session),
            );
        }
        envelope = envelope.with_child(
            XmlElement::new(SOAP_ENVELOPE_NAMESPACE, "Body")?.with_child(self.operation.clone()),
        );
        Ok(encode_xml(&envelope, XmlLimits::default())?)
    }
}

/// A Metadata SOAP response whose body is decoded independently of HTTP status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoapResponse {
    header: Option<XmlElement>,
    body: Vec<XmlElement>,
}

impl SoapResponse {
    /// Parses a SOAP envelope and converts either SOAP 1.1 or 1.2 faults to errors.
    pub fn decode(document: &[u8], limits: XmlLimits) -> Result<Self, SoapError> {
        let envelope = parse_xml(document, limits)?;
        let namespace = envelope.namespace_uri();
        if envelope.local_name() != "Envelope"
            || !matches!(
                namespace,
                SOAP_ENVELOPE_NAMESPACE | SOAP12_ENVELOPE_NAMESPACE
            )
        {
            return Err(SoapError::InvalidEnvelope);
        }

        let mut header = None;
        let mut body = None;
        for child in envelope.children() {
            if child.namespace_uri() != namespace {
                return Err(SoapError::InvalidEnvelope);
            }
            match child.local_name() {
                "Header" if header.is_none() && body.is_none() => {
                    header = Some(child.clone());
                }
                "Body" if body.is_none() => body = Some(child.clone()),
                _ => return Err(SoapError::InvalidEnvelope),
            }
        }
        let body = body.ok_or(SoapError::InvalidEnvelope)?;
        let body = body.children().cloned().collect::<Vec<_>>();
        for element in &body {
            if element.local_name() == "Fault"
                && (element.namespace_uri() == namespace
                    || (namespace == SOAP_ENVELOPE_NAMESPACE && element.namespace_uri().is_empty()))
            {
                return Err(SoapError::Fault(Box::new(parse_fault(element, namespace)?)));
            }
        }
        Ok(Self { header, body })
    }

    /// Returns the optional SOAP header element.
    pub fn header(&self) -> Option<&XmlElement> {
        self.header.as_ref()
    }

    /// Returns response elements in their original order.
    pub fn body(&self) -> &[XmlElement] {
        &self.body
    }
}

/// A SOAP fault with decoded code, message, and optional detail element.
#[derive(Clone, PartialEq, Eq)]
pub struct SoapFault {
    code: Option<String>,
    message: String,
    detail: Option<XmlElement>,
}

impl fmt::Debug for SoapFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SoapFault")
            .field("code", &self.code)
            .field("message", &"[redacted]")
            .field("detail", &self.detail.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl SoapFault {
    /// Returns the fault code, if supplied.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Returns the fault message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the optional fault detail element.
    pub fn detail(&self) -> Option<&XmlElement> {
        self.detail.as_ref()
    }
}

/// Safe SOAP protocol errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SoapError {
    /// XML input or output was malformed or exceeded configured limits.
    #[error(transparent)]
    Xml(#[from] XmlError),
    /// The document is not a valid SOAP Envelope and Body.
    #[error("invalid Metadata SOAP envelope")]
    InvalidEnvelope,
    /// The SOAP Body contains a fault, even when HTTP reports success.
    #[error("Metadata SOAP fault")]
    Fault(Box<SoapFault>),
}

fn parse_fault(fault: &XmlElement, envelope_namespace: &str) -> Result<SoapFault, SoapError> {
    let (code, message, detail) = if envelope_namespace == SOAP_ENVELOPE_NAMESPACE {
        (
            child_text(fault, "faultcode", soap11_fault_child),
            child_text(fault, "faultstring", soap11_fault_child)
                .ok_or(SoapError::InvalidEnvelope)?,
            child_element(fault, "detail", soap11_fault_child).cloned(),
        )
    } else {
        let code = child_element(fault, "Code", soap12_fault_child)
            .and_then(|code| child_text(code, "Value", soap12_fault_child));
        let reason = child_element(fault, "Reason", soap12_fault_child)
            .and_then(|reason| child_text(reason, "Text", soap12_fault_child))
            .ok_or(SoapError::InvalidEnvelope)?;
        (
            code,
            reason,
            child_element(fault, "Detail", soap12_fault_child).cloned(),
        )
    };
    Ok(SoapFault {
        code,
        message,
        detail,
    })
}

fn child_element<'a>(
    parent: &'a XmlElement,
    local_name: &str,
    namespace_matches: impl Fn(&str) -> bool,
) -> Option<&'a XmlElement> {
    parent
        .children()
        .find(|child| child.local_name() == local_name && namespace_matches(child.namespace_uri()))
}

fn child_text(
    parent: &XmlElement,
    local_name: &str,
    namespace_matches: impl Fn(&str) -> bool,
) -> Option<String> {
    child_element(parent, local_name, namespace_matches).map(XmlElement::text_content)
}

fn soap11_fault_child(namespace: &str) -> bool {
    namespace.is_empty() || namespace == SOAP_ENVELOPE_NAMESPACE
}

fn soap12_fault_child(namespace: &str) -> bool {
    namespace == SOAP12_ENVELOPE_NAMESPACE
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use crate::{BearerToken, HttpClient, InstanceOrigin, Method, StatusCode, TransportSettings};

    use super::{SoapError, SoapResponse};
    use crate::metadata::xml::XmlLimits;

    #[tokio::test]
    async fn decodes_a_soap_fault_from_a_successful_http_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/services/Soap/m/62.0"))
            .respond_with(ResponseTemplate::new(StatusCode::OK).set_body_bytes(
                include_bytes!("../../tests/fixtures/metadata/soap_fault_11.xml").to_vec(),
            ))
            .expect(1)
            .mount(&server)
            .await;

        let origin = InstanceOrigin::loopback_test(&server.uri()).expect("test server origin");
        let url = origin
            .request_url("/services/Soap/m/62.0")
            .expect("same-origin SOAP endpoint");
        let client = HttpClient::new(TransportSettings::default().without_system_proxy())
            .expect("HTTP client");
        let token = BearerToken::new("synthetic-session").expect("synthetic token");
        let response = client
            .execute(
                &origin,
                &token,
                Method::POST,
                &url,
                Some(("text/xml; charset=UTF-8", b"<request/>")),
            )
            .await
            .expect("wire response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(matches!(
            SoapResponse::decode(response.body(), XmlLimits::default()),
            Err(SoapError::Fault(_))
        ));
    }
}
