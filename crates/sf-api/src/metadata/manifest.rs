use std::collections::HashSet;

use thiserror::Error;

use crate::rest::ApiVersion;

use super::xml::{XmlElement, XmlError, XmlLimits, encode_xml, parse_xml};

const METADATA_NAMESPACE: &str = "http://soap.sforce.com/2006/04/metadata";

/// Metadata types covered by the initial source/Metadata API conversion slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum MetadataType {
    /// An Apex class and its `-meta.xml` sidecar.
    ApexClass,
    /// An Apex trigger and its `-meta.xml` sidecar.
    ApexTrigger,
}

impl MetadataType {
    /// Returns the Metadata API XML name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApexClass => "ApexClass",
            Self::ApexTrigger => "ApexTrigger",
        }
    }
}

/// One supported Metadata API type and its component members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataTypeSelection {
    metadata_type: MetadataType,
    members: Vec<String>,
}

impl MetadataTypeSelection {
    /// Creates a non-empty selection for one supported metadata type.
    pub fn new(
        metadata_type: MetadataType,
        members: Vec<String>,
    ) -> Result<Self, MetadataManifestError> {
        if members.is_empty() {
            return Err(MetadataManifestError::EmptyMembers);
        }
        let mut unique_members = HashSet::with_capacity(members.len());
        for member in &members {
            if member.trim().is_empty() || member.trim() != member {
                return Err(MetadataManifestError::InvalidMember);
            }
            if !unique_members.insert(member) {
                return Err(MetadataManifestError::DuplicateMember);
            }
        }
        Ok(Self {
            metadata_type,
            members,
        })
    }

    /// Returns the selected Metadata API type.
    pub fn metadata_type(&self) -> MetadataType {
        self.metadata_type
    }

    /// Returns component member names in manifest order.
    pub fn members(&self) -> &[String] {
        &self.members
    }
}

/// A typed Metadata API `package.xml` manifest for supported component types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataManifest {
    api_version: ApiVersion,
    types: Vec<MetadataTypeSelection>,
}

impl MetadataManifest {
    /// Creates a manifest with no duplicate metadata types.
    pub fn new(
        api_version: ApiVersion,
        types: Vec<MetadataTypeSelection>,
    ) -> Result<Self, MetadataManifestError> {
        let mut unique_types = HashSet::with_capacity(types.len());
        for selection in &types {
            if !unique_types.insert(selection.metadata_type) {
                return Err(MetadataManifestError::DuplicateMetadataType(
                    selection.metadata_type.as_str().to_owned(),
                ));
            }
        }
        Ok(Self { api_version, types })
    }

    /// Parses a bounded Metadata API `package.xml` document.
    pub fn parse_xml(document: &[u8]) -> Result<Self, MetadataManifestError> {
        let root = parse_xml(document, XmlLimits::default())?;
        if root.local_name() != "Package" || root.namespace_uri() != METADATA_NAMESPACE {
            return Err(MetadataManifestError::InvalidDocument);
        }
        if !root.text_content().trim().is_empty() {
            return Err(MetadataManifestError::InvalidDocument);
        }

        let mut api_version = None;
        let mut types = Vec::new();
        for child in root.children() {
            if child.namespace_uri() != METADATA_NAMESPACE {
                return Err(MetadataManifestError::InvalidDocument);
            }
            match child.local_name() {
                "types" => types.push(parse_selection(child)?),
                "version" if api_version.is_none() => {
                    let value = leaf_text(child, "version")?;
                    api_version = Some(
                        ApiVersion::new(&value)
                            .map_err(|_| MetadataManifestError::InvalidApiVersion)?,
                    );
                }
                _ => return Err(MetadataManifestError::InvalidDocument),
            }
        }

        let api_version = api_version.ok_or(MetadataManifestError::InvalidDocument)?;
        Self::new(api_version, types)
    }

    /// Encodes this manifest as escaped, namespace-aware Metadata API XML.
    pub fn encode_xml(&self) -> Result<Vec<u8>, MetadataManifestError> {
        let mut root = XmlElement::new(METADATA_NAMESPACE, "Package")?;
        for selection in &self.types {
            let mut types_element = XmlElement::new(METADATA_NAMESPACE, "types")?;
            for member in &selection.members {
                types_element = types_element.with_child(
                    XmlElement::new(METADATA_NAMESPACE, "members")?.with_text(member.clone()),
                );
            }
            types_element = types_element.with_child(
                XmlElement::new(METADATA_NAMESPACE, "name")?
                    .with_text(selection.metadata_type.as_str()),
            );
            root = root.with_child(types_element);
        }
        root = root.with_child(
            XmlElement::new(METADATA_NAMESPACE, "version")?.with_text(self.api_version.as_str()),
        );
        Ok(encode_xml(&root, XmlLimits::default())?)
    }

    /// Returns the canonical Salesforce API version.
    pub fn api_version(&self) -> &ApiVersion {
        &self.api_version
    }

    /// Returns metadata selections in manifest order.
    pub fn types(&self) -> &[MetadataTypeSelection] {
        &self.types
    }
}

fn parse_selection(element: &XmlElement) -> Result<MetadataTypeSelection, MetadataManifestError> {
    let mut metadata_type = None;
    let mut members = Vec::new();
    for child in element.children() {
        if child.namespace_uri() != METADATA_NAMESPACE {
            return Err(MetadataManifestError::InvalidDocument);
        }
        match child.local_name() {
            "members" => members.push(leaf_text(child, "members")?),
            "name" if metadata_type.is_none() => {
                let name = leaf_text(child, "name")?;
                metadata_type = Some(match name.as_str() {
                    "ApexClass" => MetadataType::ApexClass,
                    "ApexTrigger" => MetadataType::ApexTrigger,
                    _ => return Err(MetadataManifestError::UnsupportedMetadataType),
                });
            }
            _ => return Err(MetadataManifestError::InvalidDocument),
        }
    }
    let metadata_type = metadata_type.ok_or(MetadataManifestError::InvalidDocument)?;
    if !element.text_content().trim().is_empty() {
        return Err(MetadataManifestError::InvalidDocument);
    }
    MetadataTypeSelection::new(metadata_type, members)
}

fn leaf_text(element: &XmlElement, expected_name: &str) -> Result<String, MetadataManifestError> {
    if element.local_name() != expected_name
        || element.namespace_uri() != METADATA_NAMESPACE
        || element.children().next().is_some()
    {
        return Err(MetadataManifestError::InvalidDocument);
    }
    let value = element.text_content();
    let value = value.trim();
    if value.is_empty() {
        return Err(MetadataManifestError::InvalidDocument);
    }
    Ok(value.to_owned())
}

/// Safe parsing and encoding failures for a Metadata API package manifest.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MetadataManifestError {
    /// XML input was malformed or exceeded parser limits.
    #[error(transparent)]
    Xml(#[from] XmlError),
    /// The XML does not have the expected Metadata API package structure.
    #[error("invalid Metadata API package manifest")]
    InvalidDocument,
    /// The manifest selects a metadata type outside the implemented slice.
    #[error("unsupported Metadata API type")]
    UnsupportedMetadataType,
    /// The API version is not a valid major/minor version.
    #[error("invalid Metadata API version")]
    InvalidApiVersion,
    /// A metadata type appears more than once.
    #[error("duplicate Metadata API type: {0}")]
    DuplicateMetadataType(String),
    /// A type selection must include at least one member.
    #[error("Metadata API type selection has no members")]
    EmptyMembers,
    /// A member name is empty or consists only of whitespace.
    #[error("invalid Metadata API member name")]
    InvalidMember,
    /// A member name appears more than once in a type selection.
    #[error("duplicate Metadata API member")]
    DuplicateMember,
}
