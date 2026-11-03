#![allow(clippy::expect_used, clippy::unwrap_used)]

use proptest::prelude::*;
use sf_api::metadata::{
    soap::{SoapError, SoapRequest, SoapResponse},
    xml::{XmlAttribute, XmlContent, XmlElement, XmlError, XmlLimits, encode_xml, parse_xml},
};

#[test]
fn metadata_requests_escape_values_and_preserve_namespaces() {
    let operation = XmlElement::new("urn:example:metadata", "update")
        .expect("valid operation")
        .with_child(
            XmlElement::new("urn:example:metadata", "fullName")
                .expect("valid field")
                .with_text("A & B <C>"),
        );
    let request = SoapRequest::new(operation).with_session_id("session<&>");

    let encoded = request.encode().expect("valid SOAP request");
    let encoded = std::str::from_utf8(&encoded).expect("SOAP is UTF-8");

    assert!(!format!("{request:?}").contains("session<&>"));
    assert!(encoded.contains("xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\""));
    assert!(encoded.contains("soapenv:Header"));
    assert!(encoded.contains("xmlns:met=\"urn:metadata.soap.sforce.com\""));
    assert!(encoded.contains("session&lt;&amp;&gt;"));
    assert!(encoded.contains("xmlns:n0=\"urn:example:metadata\""));
    assert!(encoded.contains("A &amp; B &lt;C&gt;"));
}

#[test]
fn parser_resolves_namespaces_and_decodes_escaped_text() {
    let response = include_bytes!("fixtures/metadata/soap_success.xml");
    let root = parse_xml(response, XmlLimits::default()).expect("valid SOAP XML");

    assert_eq!(
        root.namespace_uri(),
        "http://schemas.xmlsoap.org/soap/envelope/"
    );
    assert_eq!(root.local_name(), "Envelope");

    let body = root
        .children()
        .find(|child| child.local_name() == "Body")
        .expect("SOAP body");
    let result = body.children().next().expect("metadata result");
    assert_eq!(result.namespace_uri(), "urn:metadata.soap.sforce.com");
    assert_eq!(
        result
            .children()
            .next()
            .expect("value element")
            .text_content(),
        "A & B <C>"
    );
}

#[test]
fn metadata_soap_fault_is_returned_as_a_typed_error() {
    let response = include_bytes!("fixtures/metadata/soap_fault_11.xml");
    let error = SoapResponse::decode(response, XmlLimits::default())
        .expect_err("SOAP fault must not be treated as a successful response");
    let debug = format!("{error:?}");

    match error {
        SoapError::Fault(fault) => {
            assert_eq!(fault.code(), Some("sf:INVALID_TYPE"));
            assert_eq!(fault.message(), "Metadata type is not supported");
            assert!(!debug.contains("Metadata type is not supported"));
            assert!(!debug.contains("Synthetic fault detail"));
            assert_eq!(
                fault
                    .detail()
                    .expect("fault detail")
                    .children()
                    .next()
                    .expect("fault detail message")
                    .text_content(),
                "Synthetic fault detail"
            );
        }
        other => panic!("expected typed SOAP fault, got {other:?}"),
    }
}

#[test]
fn metadata_soap_success_response_retains_the_operation_element() {
    let response = SoapResponse::decode(
        include_bytes!("fixtures/metadata/soap_success.xml"),
        XmlLimits::default(),
    )
    .expect("valid SOAP response");

    assert_eq!(response.body().len(), 1);
    assert_eq!(
        response.body()[0].namespace_uri(),
        "urn:metadata.soap.sforce.com"
    );
    assert_eq!(response.body()[0].local_name(), "result");
}

#[test]
fn metadata_soap_12_fault_is_decoded() {
    let error = SoapResponse::decode(
        include_bytes!("fixtures/metadata/soap_fault_12.xml"),
        XmlLimits::default(),
    )
    .expect_err("SOAP 1.2 fault must be returned");

    let SoapError::Fault(fault) = error else {
        panic!("expected SOAP fault");
    };
    assert_eq!(fault.code(), Some("env:Receiver"));
    assert_eq!(fault.message(), "Synthetic SOAP 1.2 fault");
}

#[test]
fn parser_rejects_malformed_oversized_and_invalidly_encoded_documents() {
    let malformed = include_bytes!("fixtures/metadata/soap_malformed.xml");
    assert!(matches!(
        parse_xml(malformed, XmlLimits::default()),
        Err(XmlError::Malformed | XmlError::InvalidDocument)
    ));

    let oversized = include_bytes!("fixtures/metadata/soap_oversized.xml");
    let small_limits = XmlLimits::new(32, 8, 10, 32).expect("small XML limits");
    assert_eq!(
        parse_xml(oversized, small_limits),
        Err(XmlError::BodyTooLarge { limit: 32 })
    );

    let invalid_encoding = include_bytes!("fixtures/metadata/soap_invalid_encoding.xml");
    assert_eq!(
        parse_xml(invalid_encoding, XmlLimits::default()),
        Err(XmlError::InvalidEncoding)
    );
    assert_eq!(
        parse_xml(b"<root>\xff</root>", XmlLimits::default()),
        Err(XmlError::InvalidEncoding)
    );
}

#[test]
fn parser_rejects_dtds_and_enforces_depth_node_and_text_limits() {
    let dtd = br#"<!DOCTYPE root SYSTEM "file:///etc/passwd"><root/>"#;
    assert_eq!(
        parse_xml(dtd, XmlLimits::default()),
        Err(XmlError::ForbiddenDtd)
    );

    let shallow_limits = XmlLimits::new(128, 2, 10, 128).expect("shallow XML limits");
    assert_eq!(
        parse_xml(b"<a><b><c/></b></a>", shallow_limits),
        Err(XmlError::DepthLimitExceeded { limit: 2 })
    );

    let few_nodes = XmlLimits::new(128, 8, 2, 128).expect("small node limit");
    assert_eq!(
        parse_xml(b"<a><b><c/></b></a>", few_nodes),
        Err(XmlError::NodeLimitExceeded { limit: 2 })
    );

    let little_text = XmlLimits::new(128, 8, 10, 4).expect("small text limit");
    assert_eq!(
        parse_xml(b"<value>hello</value>", little_text),
        Err(XmlError::TextLimitExceeded { limit: 4 })
    );
}

#[test]
fn encoder_rejects_invalid_names_and_enforces_output_size() {
    assert_eq!(
        XmlElement::new("urn:example", "invalid:name"),
        Err(XmlError::InvalidName)
    );
    assert_eq!(
        XmlAttribute::new("", "xmlns", "urn:forbidden"),
        Err(XmlError::InvalidName)
    );

    let element = XmlElement::new("urn:example", "value")
        .expect("valid name")
        .with_text("small");
    let limits = XmlLimits::new(32, 8, 10, 32).expect("bounded XML limits");
    assert_eq!(
        encode_xml(&element, limits),
        Err(XmlError::BodyTooLarge { limit: 32 })
    );
}

fn xml_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..96)
        .prop_filter("text must contain XML 1.0 characters", |characters| {
            characters.iter().all(|character| {
                matches!(
                    *character as u32,
                    0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
                )
            })
        })
        .prop_map(|characters| characters.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"),
        )),
        .. ProptestConfig::default()
    })]

    #[test]
    fn xml_values_round_trip_without_losing_namespace_or_order(text in xml_text()) {
        let element = XmlElement::new("urn:round-trip", "value")
            .expect("valid element")
            .with_attribute(
                XmlAttribute::new("urn:round-trip", "kind", text.clone())
                    .expect("valid attribute"),
            )
            .with_text(text.clone())
            .with_child(
                XmlElement::new("urn:round-trip", "nested")
                    .expect("valid child")
                    .with_text(text.clone()),
            );
        let encoded = encode_xml(&element, XmlLimits::default()).expect("XML encodes");
        let decoded = parse_xml(&encoded, XmlLimits::default()).expect("encoded XML parses");

        prop_assert_eq!(decoded.namespace_uri(), "urn:round-trip");
        prop_assert_eq!(decoded.local_name(), "value");
        prop_assert_eq!(decoded.attributes()[0].value(), text.as_str());
        prop_assert_eq!(decoded.text_content(), text.as_str());
        let child_position = decoded
            .content()
            .iter()
            .position(|content| matches!(content, XmlContent::Element(_)))
            .expect("nested element is preserved");
        prop_assert!(decoded.content()[..child_position]
            .iter()
            .all(|content| matches!(content, XmlContent::Text(_))));
        prop_assert!(matches!(
            decoded.content().get(child_position),
            Some(XmlContent::Element(_))
        ));
        prop_assert_eq!(
            decoded.children().next().expect("nested element").text_content(),
            text.as_str()
        );
    }
}
