//! Metadata SOAP protocol types and bounded XML handling.

pub mod deploy;
pub mod manifest;
pub mod soap;
pub mod xml;

pub use manifest::{MetadataManifest, MetadataManifestError, MetadataType, MetadataTypeSelection};
