# B30-04: Metadata SOAP and XML handling

## Delivered

- Added namespace-aware `XmlElement` and `XmlAttribute` models plus library-backed XML encoding and parsing in `sf-api`.
- Added typed SOAP 1.1 request envelopes with the Metadata API `SessionHeader`, escaped values, and namespaced operation elements.
- Added SOAP 1.1 and 1.2 response decoding, including typed fault code/message/detail extraction independent of HTTP status.
- Rejected DTDs and unsupported entity references. XML parsing accepts UTF-8 only and enforces document, nesting, element-count, and aggregate text limits.
- Redacted XML text, attribute values, session IDs, and SOAP fault details from `Debug` output.

Default XML limits are 16 MiB per document, depth 64, 100,000 elements, and 8 MiB of text. Hard configurable caps are 64 MiB, depth 256, one million elements, and at most 64 namespace declarations per element. XML output is bounded while it is written.

## Tests

`cargo test -p sf-api --locked` covers escaped request values, namespaces, SOAP success and fault fixtures, HTTP 200 SOAP faults, malformed and oversized documents, invalid encodings, DTD rejection, parser limits, and bounded output. A `proptest` round-trip invariant exercises namespace, attribute, text, and content ordering; regression seeds retain minimized edge cases.

Fuzzing remains conditional on establishing a bounded fuzz runtime and adding its toolchain to CI. The current CI workflow has no nightly or fuzz-runner job; no unbounded fuzz process was added to the default checks.

## Scope

The module provides protocol primitives only. It does not select API versions, send Metadata SOAP requests itself, implement deploy/retrieve workflows, or claim CLI command support.
