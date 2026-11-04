#![allow(clippy::expect_used, clippy::unwrap_used)]

use sf_api::metadata::deploy::{
    DeployId, DeployOptions, DeployResult, DeployStatus, DeployWireError, TestLevel,
    check_deploy_status_request, deploy_request, parse_deploy_response,
};

const ID: &str = "0AfXX0000000001AAA";

fn envelope(body: &str) -> String {
    format!(
        "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\">\
         <soapenv:Body>{body}</soapenv:Body></soapenv:Envelope>"
    )
}

#[test]
fn deploy_request_carries_options_and_hides_the_session() {
    let options = DeployOptions {
        check_only: true,
        test_level: TestLevel::RunLocalTests,
        ..DeployOptions::default()
    };
    let request = deploy_request("UEsDBA==", options, "secret-session").expect("request");
    assert!(!format!("{request:?}").contains("secret-session"));
    let xml = String::from_utf8(request.encode().expect("encode")).expect("utf8");
    assert!(xml.contains("UEsDBA=="));
    assert!(xml.contains(">true</"));
    assert!(xml.contains("RunLocalTests"));
    assert!(xml.contains("deploy"));
}

#[test]
fn deploy_response_yields_a_validated_id() {
    let body = envelope(&format!(
        "<deployResponse xmlns=\"http://soap.sforce.com/2006/04/metadata\"><result>\
         <done>false</done><id>{ID}</id><state>Queued</state></result></deployResponse>"
    ));
    assert_eq!(
        parse_deploy_response(body.as_bytes()).expect("id").as_str(),
        ID
    );
}

#[test]
fn deploy_response_with_a_bad_id_is_rejected() {
    let body = envelope(
        "<deployResponse xmlns=\"http://soap.sforce.com/2006/04/metadata\"><result>\
         <id>not-an-id</id></result></deployResponse>",
    );
    assert!(matches!(
        parse_deploy_response(body.as_bytes()),
        Err(DeployWireError::InvalidDeployId)
    ));
}

#[test]
fn status_request_names_the_deployment() {
    let id = DeployId::parse(ID).expect("id");
    let request = check_deploy_status_request(&id, true, "s").expect("request");
    let xml = String::from_utf8(request.encode().expect("encode")).expect("utf8");
    assert!(xml.contains("checkDeployStatus"));
    assert!(xml.contains(ID));
}

#[test]
fn failed_status_reports_component_failures() {
    let body = envelope(&format!(
        "<checkDeployStatusResponse xmlns=\"http://soap.sforce.com/2006/04/metadata\"><result>\
         <id>{ID}</id><checkOnly>true</checkOnly><done>true</done><status>Failed</status>\
         <numberComponentsDeployed>0</numberComponentsDeployed>\
         <numberComponentsTotal>2</numberComponentsTotal>\
         <numberComponentErrors>1</numberComponentErrors>\
         <details><componentFailures><componentType>ApexClass</componentType>\
         <fullName>Foo</fullName><fileName>classes/Foo.cls</fileName>\
         <problem>Unexpected token</problem><problemType>Error</problemType>\
         <lineNumber>3</lineNumber><columnNumber>9</columnNumber></componentFailures></details>\
         </result></checkDeployStatusResponse>"
    ));
    let result = DeployResult::from_status_response(body.as_bytes()).expect("result");
    assert_eq!(result.status, DeployStatus::Failed);
    assert!(result.status.is_terminal());
    assert!(result.check_only);
    assert_eq!(result.components_total, 2);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].line_number, Some(3));
    assert_eq!(result.failures[0].full_name, "Foo");
}

#[test]
fn in_progress_is_not_terminal_and_unknown_status_is_an_error() {
    let ok = envelope(&format!(
        "<checkDeployStatusResponse xmlns=\"http://soap.sforce.com/2006/04/metadata\"><result>\
         <id>{ID}</id><status>InProgress</status></result></checkDeployStatusResponse>"
    ));
    let result = DeployResult::from_status_response(ok.as_bytes()).expect("result");
    assert!(!result.status.is_terminal());

    let bad = envelope(&format!(
        "<checkDeployStatusResponse xmlns=\"http://soap.sforce.com/2006/04/metadata\"><result>\
         <id>{ID}</id><status>Bogus</status></result></checkDeployStatusResponse>"
    ));
    assert!(DeployResult::from_status_response(bad.as_bytes()).is_err());
}

#[test]
fn soap_faults_surface_as_errors() {
    let body = envelope(
        "<soapenv:Fault><faultcode>sf:INVALID_SESSION_ID</faultcode>\
         <faultstring>Invalid Session</faultstring></soapenv:Fault>",
    );
    assert!(matches!(
        DeployResult::from_status_response(body.as_bytes()),
        Err(DeployWireError::Soap(_))
    ));
}
