# B30-02: REST Query, Pagination, and API Errors

## Result

`sf-api::rest` now implements the selected SOQL REST workflow. `QueryRequest`
requires an explicit `ApiVersion`, validates non-empty bounded SOQL, and
redacts the query in debug output. Generic `QueryPage<T>` and `QueryResult<T>`
models preserve Salesforce record order. Callers may consume bounded pages
incrementally, or use `query_all` for a complete result.

Continuation locators are validated against the same `InstanceOrigin`, exact
API version, and query endpoint before any authenticated request. Locators with
queries, unexpected paths/origins, oversized values, or cycles are rejected.
`query_all` checks completion and total-size consistency and does not return
partial results. Defaults cap aggregation at 50,000 records, 64 MiB of response
bodies, and 1,000 pages; `with_limits` supports workflow-specific bounds within
hard ceilings of 1,000,000 records and 512 MiB. Each individual
response remains subject to B30-01's transport cap.

Salesforce error responses map to safe categories and retain only allowlisted
error codes, validated request identifiers, and parsed `Retry-After` guidance
(delta seconds or HTTP date). Error messages, fields, and raw response bodies
are not retained in `Display`, `Debug`, or error sources.

## Executable command boundary

`sf data query` remains `planned` in the compatibility inventory. B20-03 has
not wired persistent credentials and org selection into CLI workflows, so this
change does not register a command that cannot run end to end. The query API is
ready for that workflow once the authenticated CLI session is available; no
live-org differential test was run.

## Verification

Mock-server tests cover encoded SOQL, explicit API versions, empty and
multi-page results (including an empty intermediate page), response ordering,
incremental page consumption, cross-origin and wrong-version locators,
pagination cycles, malformed query responses, bounded results, Salesforce
error categories, request IDs, delta/date retry guidance, and diagnostic
redaction. Tests use synthetic responses and private loopback-only setup.
