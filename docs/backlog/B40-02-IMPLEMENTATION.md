# B40-02: Build metadata mapping and archive safety

## Initial conversion contract

The pinned [`plugin-deploy-retrieve` and
`source-deploy-retrieve` revisions](../technical-architecture.md#verified-source-map)
expose broad manifest, metadata, source-directory, and metadata-directory
workflows. This implementation deliberately does not copy their full metadata
registry or claim command parity.

The first native conversion slice, and the planned initial deploy/retrieve
type scope, covers:

- `ApexClass`: `main/default/classes/<member>.cls` and its required
  `<member>.cls-meta.xml` sidecar.
- `ApexTrigger`: `main/default/triggers/<member>.trigger` and its required
  `<member>.trigger-meta.xml` sidecar.
- Metadata API `package.xml` selections with concrete member names for those
  two types and a validated Salesforce API version.

Other metadata types, wildcard members, additional package source roots, and
unlisted files fail explicitly. Deploy is planned for source/manifest
selections that resolve only to these types; retrieve is planned for concrete
members of these types. The complete CLI selectors, flags, output, and error
behavior remain unimplemented and must be inventoried before B40-03/B40-04
command work.

## Archive safety

`sf-core::project::metadata::extract_metadata_archive` validates the complete
ZIP before creating the destination. It rejects unsafe or colliding paths,
symlinks and special files, multi-disk and ZIP64 archives, duplicate entries,
and compressed/expanded/work/entry-limit violations. It decompresses and
checksum-validates file contents in memory before extraction, writes only
regular files with default permissions, requires a new destination below an
existing parent, and removes the new root if writing fails. Retrieved files are
never executed.

Default limits are 128 MiB compressed input, 256 MiB expanded content, 10,000
entries, and 256 MiB of actual decompressed output during validation. Hard
limits are 256 MiB compressed input, 512 MiB expanded content, 10,000 entries,
and 512 MiB of decompressed output. The decompressed-output budget bounds the
work performed by the supported Stored/Deflate readers; it is not a wall-clock
CPU-time guarantee.

The ZIP dependency is pinned to 9.3.0, whose published MSRV is Rust 1.88. Rust
1.85's compatible ZIP release was flagged by `cargo audit` with four extraction
advisories; the workspace MSRV was therefore raised to 1.88 rather than
retaining a known-vulnerable parser. The CI test matrix runs on that declared
MSRV across Linux, macOS, and Windows. The default feature set is disabled;
only Deflate support is enabled. The crate reports an MIT license and its
upstream repository is [zip-rs/zip2](https://github.com/zip-rs/zip2). The
selected `zlib-rs` backend reports the permissive Zlib license, now explicitly
allowed in the dependency policy.

## Tests

`cargo test -p sf-core --test metadata_mapping --locked` covers typed manifest
round-tripping, both supported source forms, missing sidecars, unsupported
types, unlisted source components, archive traversal and platform-specific
paths, symlinks, entry/size/work limits, existing destinations, full-archive
preflight, and non-execution of script-shaped files. Fixtures are synthetic
and contain no org data or credentials.
