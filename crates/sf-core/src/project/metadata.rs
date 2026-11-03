//! Narrow source/Metadata API conversion and safe archive extraction.

mod archive;
mod mapping;

pub use archive::{ArchiveError, ArchiveLimits, ArchiveSummary, extract_metadata_archive};
pub use mapping::{
    MetadataFileMapping, MetadataMapping, MetadataMappingError, map_metadata_to_source,
    map_source_to_metadata,
};
