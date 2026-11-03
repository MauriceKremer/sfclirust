# B40-01: Load Salesforce project context

## Upstream scope

The implementation was checked against `forcedotcom/sfdx-core` at the pinned
commit [`23d7f942fa79b4a8eec9b75b4b9d97a425eb508b`](https://github.com/forcedotcom/sfdx-core/tree/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b):

- [`SfProject.resolve` and `getPackageDirectoriesSync`](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/sfProject.ts) define upward project discovery, package paths, directory existence, and default-package selection.
- [`ProjectJsonSchema`](https://github.com/forcedotcom/sfdx-core/blob/23d7f942fa79b4a8eec9b75b4b9d97a425eb508b/src/schema/sfdx-project/sfdxProjectJson.ts) requires non-empty `packageDirectories`, defines `path`/`default`, and declares the `sourceApiVersion` default of `48.0`.

The loader interprets only `packageDirectories[].path`,
`packageDirectories[].default`, and `sourceApiVersion`. Other fields, including
package metadata and plugin configuration, remain available in the original
read-only JSON value; the loader neither rejects unknown fields nor rewrites the
project file. The pinned upstream schema's `48.0` default is used when
`sourceApiVersion` is absent.

The pinned project schema has no manifest-path field. The loader therefore
resolves the conventional `<project>/manifest` directory; it may be absent until
a workflow uses it. No deploy/retrieve flag or manifest-selection behavior is
claimed here.

## Path and error behavior

- Project discovery starts at a caller-supplied directory and searches its
  ancestors for a regular `sfdx-project.json` file.
- Package paths are relative to the project root. Both slash conventions are
  normalized on every platform; absolute POSIX, drive, and UNC paths, parent
  traversal, Windows alternate data stream syntax, missing directories, and
  symlinks resolving outside the project root fail explicitly.
- Existing manifest directories are also checked for type and root containment.
- Project JSON is read-only and capped at 16 MiB.
- `ProjectError::code()` returns stable `PROJECT_NOT_FOUND`,
  `PROJECT_IO_ERROR`, `PROJECT_INVALID`, or `PROJECT_UNSUPPORTED` codes for
  mapping to the CLI's operational-error contract (exit code 1).

## Tests

`cargo test -p sf-core --test project_loading --locked` covers nested discovery,
missing and malformed context, unknown-field preservation, no file rewrites,
single- and multi-package default selection, missing package directories,
Windows-style separators on all hosts, and absolute/traversal/UNC/drive and
symlink-escape paths. The test uses temporary project roots and does not change
the process working directory or read user state.

No CLI command was added, so no command inventory entry is applicable.
