use std::{
    collections::{HashMap, HashSet},
    fs::{self, DirEntry},
    io,
    path::{Path, PathBuf},
};

use sf_api::{
    metadata::xml::XmlError,
    metadata::{MetadataManifest, MetadataManifestError, MetadataType, MetadataTypeSelection},
    rest::ApiVersion,
};
use thiserror::Error;

/// One source-format file and its Metadata API package path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataFileMapping {
    metadata_type: MetadataType,
    member: String,
    source_path: PathBuf,
    metadata_path: PathBuf,
}

impl MetadataFileMapping {
    /// Returns the Metadata API component type.
    pub fn metadata_type(&self) -> MetadataType {
        self.metadata_type
    }

    /// Returns the component member name.
    pub fn member(&self) -> &str {
        &self.member
    }

    /// Returns the path relative to the source package directory.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the path relative to the Metadata API `unpackaged` directory.
    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }
}

/// A typed manifest and the complete file mapping for its supported components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataMapping {
    manifest: MetadataManifest,
    files: Vec<MetadataFileMapping>,
}

impl MetadataMapping {
    /// Returns the typed Metadata API manifest.
    pub fn manifest(&self) -> &MetadataManifest {
        &self.manifest
    }

    /// Returns source-to-metadata file pairs in deterministic path order.
    pub fn files(&self) -> &[MetadataFileMapping] {
        &self.files
    }
}

/// Converts supported source-format components into Metadata API paths.
///
/// The package root must use `main/default`. Only Apex classes and triggers,
/// including their required `-meta.xml` sidecars, are included.
pub fn map_source_to_metadata(
    package_directory: impl AsRef<Path>,
    api_version: &str,
) -> Result<MetadataMapping, MetadataMappingError> {
    let package_directory = canonical_directory(package_directory.as_ref())?;
    let api_version =
        ApiVersion::new(api_version).map_err(|_| MetadataMappingError::InvalidApiVersion)?;
    let default_directory = source_default_directory(&package_directory)?;
    let mut selections = Vec::new();
    let mut files = Vec::new();

    reject_unsupported_source_entries(&default_directory)?;
    for metadata_type in [MetadataType::ApexClass, MetadataType::ApexTrigger] {
        let component_directory = default_directory.join(source_directory(metadata_type));
        let Some(entries) = read_optional_directory(&component_directory)? else {
            continue;
        };
        let (members, mappings) =
            parse_source_component_files(&component_directory, metadata_type, entries)?;
        if !members.is_empty() {
            selections.push(MetadataTypeSelection::new(metadata_type, members)?);
            files.extend(mappings);
        }
    }

    files.sort_by(|left, right| left.metadata_path.cmp(&right.metadata_path));
    let manifest = MetadataManifest::new(api_version, selections)?;
    Ok(MetadataMapping { manifest, files })
}

/// Converts a Metadata API `unpackaged` directory to source-format paths.
///
/// The directory must contain a supported `package.xml`. Unlisted files and
/// metadata types outside the initial ApexClass/ApexTrigger slice are rejected.
pub fn map_metadata_to_source(
    unpackaged_directory: impl AsRef<Path>,
) -> Result<MetadataMapping, MetadataMappingError> {
    let unpackaged_directory = canonical_directory(unpackaged_directory.as_ref())?;
    let manifest_path = unpackaged_directory.join("package.xml");
    ensure_regular_file(&manifest_path)?;
    let manifest_length = fs::metadata(&manifest_path)
        .map_err(|source| MetadataMappingError::io("inspect", &manifest_path, source))?
        .len();
    const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
    if manifest_length > MAX_MANIFEST_BYTES {
        return Err(MetadataMappingError::Manifest(MetadataManifestError::Xml(
            XmlError::BodyTooLarge {
                limit: MAX_MANIFEST_BYTES as usize,
            },
        )));
    }
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|source| MetadataMappingError::io("read", &manifest_path, source))?;
    let manifest = MetadataManifest::parse_xml(&manifest_bytes)?;
    validate_metadata_directories(&unpackaged_directory)?;
    let mut files = Vec::new();
    let mut listed = HashMap::<MetadataType, HashSet<String>>::new();

    for selection in manifest.types() {
        let metadata_type = selection.metadata_type();
        let mut members = HashSet::with_capacity(selection.members().len());
        for member in selection.members() {
            validate_member_name(member, &manifest_path)?;
            members.insert(member.clone());
            for (metadata_path, source_path) in component_file_pairs(metadata_type, member) {
                let full_metadata_path = unpackaged_directory.join(&metadata_path);
                ensure_regular_file(&full_metadata_path)?;
                files.push(MetadataFileMapping {
                    metadata_type,
                    member: member.clone(),
                    source_path,
                    metadata_path,
                });
            }
        }
        listed.insert(metadata_type, members);
    }

    reject_unlisted_metadata_files(&unpackaged_directory, &listed)?;
    files.sort_by(|left, right| left.metadata_path.cmp(&right.metadata_path));
    Ok(MetadataMapping { manifest, files })
}

fn canonical_directory(path: &Path) -> Result<PathBuf, MetadataMappingError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| MetadataMappingError::io("inspect", path, source))?;
    if metadata.file_type().is_symlink() {
        return Err(MetadataMappingError::SymbolicLink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_dir() {
        return Err(MetadataMappingError::InvalidDirectory {
            path: path.to_path_buf(),
        });
    }
    fs::canonicalize(path).map_err(|source| MetadataMappingError::io("resolve", path, source))
}

fn source_default_directory(package_directory: &Path) -> Result<PathBuf, MetadataMappingError> {
    let main_directory = package_directory.join("main");
    let Some(main_entries) = read_optional_directory(&main_directory)? else {
        return Err(MetadataMappingError::MissingSourceRoot {
            path: main_directory.join("default"),
        });
    };
    for entry in main_entries {
        let entry_path = entry.path();
        let name = entry.file_name();
        if name != "default"
            || !entry
                .file_type()
                .map_err(|source| MetadataMappingError::io("inspect", &entry_path, source))?
                .is_dir()
        {
            return Err(MetadataMappingError::UnsupportedEntry { path: entry_path });
        }
    }
    let default_directory = main_directory.join("default");
    match read_optional_directory(&default_directory)? {
        Some(_) => Ok(default_directory),
        None => Err(MetadataMappingError::MissingSourceRoot {
            path: default_directory,
        }),
    }
}

fn reject_unsupported_source_entries(default_directory: &Path) -> Result<(), MetadataMappingError> {
    for entry in read_directory(default_directory)? {
        let path = entry.path();
        let name = entry.file_name();
        let file_type = entry
            .file_type()
            .map_err(|source| MetadataMappingError::io("inspect", &path, source))?;
        if file_type.is_symlink() {
            return Err(MetadataMappingError::SymbolicLink { path });
        }
        if !file_type.is_dir()
            || (name != source_directory(MetadataType::ApexClass)
                && name != source_directory(MetadataType::ApexTrigger))
        {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        }
    }
    Ok(())
}

fn parse_source_component_files(
    component_directory: &Path,
    metadata_type: MetadataType,
    entries: Vec<DirEntry>,
) -> Result<(Vec<String>, Vec<MetadataFileMapping>), MetadataMappingError> {
    let extension = source_extension(metadata_type);
    let sidecar_suffix = format!(".{extension}-meta.xml");
    let primary_suffix = format!(".{extension}");
    let mut components = HashMap::<String, ComponentFiles>::new();
    let mut case_insensitive_names = HashMap::<String, String>::new();

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| MetadataMappingError::io("inspect", &path, source))?;
        if file_type.is_symlink() {
            return Err(MetadataMappingError::SymbolicLink { path });
        }
        if !file_type.is_file() {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        }
        let file_name = entry
            .file_name()
            .into_string()
            .map_err(|_| MetadataMappingError::InvalidMemberName { path: path.clone() })?;
        let (member, is_sidecar) = if let Some(member) = file_name.strip_suffix(&sidecar_suffix) {
            (member, true)
        } else if let Some(member) = file_name.strip_suffix(&primary_suffix) {
            (member, false)
        } else {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        };
        validate_member_name(member, &path)?;
        let lower_name = member.to_lowercase();
        if let Some(previous) = case_insensitive_names.insert(lower_name, member.to_owned())
            && previous != member
        {
            return Err(MetadataMappingError::CaseCollision {
                first: previous,
                second: member.to_owned(),
            });
        }
        let component = components.entry(member.to_owned()).or_default();
        let slot = if is_sidecar {
            &mut component.sidecar
        } else {
            &mut component.primary
        };
        if slot.replace(path).is_some() {
            return Err(MetadataMappingError::DuplicateFile {
                path: component_directory.join(file_name),
            });
        }
    }

    let mut members = Vec::with_capacity(components.len());
    let mut mappings = Vec::with_capacity(components.len() * 2);
    for (member, component) in components {
        if component.primary.is_none() {
            return Err(MetadataMappingError::MissingComponentFile {
                path: component_directory.join(format!("{member}.{extension}")),
            });
        }
        if component.sidecar.is_none() {
            return Err(MetadataMappingError::MissingComponentFile {
                path: component_directory.join(format!("{member}{sidecar_suffix}")),
            });
        }
        members.push(member.clone());
        for (metadata_path, source_path) in component_file_pairs(metadata_type, &member) {
            mappings.push(MetadataFileMapping {
                metadata_type,
                member: member.clone(),
                source_path,
                metadata_path,
            });
        }
    }
    members.sort();
    mappings.sort_by(|left, right| left.metadata_path.cmp(&right.metadata_path));
    Ok((members, mappings))
}

fn component_file_pairs(metadata_type: MetadataType, member: &str) -> [(PathBuf, PathBuf); 2] {
    let extension = source_extension(metadata_type);
    let directory = source_directory(metadata_type);
    let primary_name = format!("{member}.{extension}");
    let sidecar_name = format!("{member}.{extension}-meta.xml");
    [
        (
            PathBuf::from(directory).join(&primary_name),
            PathBuf::from("main/default")
                .join(directory)
                .join(primary_name),
        ),
        (
            PathBuf::from(directory).join(&sidecar_name),
            PathBuf::from("main/default")
                .join(directory)
                .join(sidecar_name),
        ),
    ]
}

fn reject_unlisted_metadata_files(
    unpackaged_directory: &Path,
    listed: &HashMap<MetadataType, HashSet<String>>,
) -> Result<(), MetadataMappingError> {
    for entry in read_directory(unpackaged_directory)? {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| MetadataMappingError::io("inspect", &path, source))?;
        let name = entry.file_name();
        if name == "package.xml" {
            if file_type.is_symlink() || !file_type.is_file() {
                return Err(MetadataMappingError::UnsupportedEntry { path });
            }
            continue;
        }
        let Some(metadata_type) = [MetadataType::ApexClass, MetadataType::ApexTrigger]
            .into_iter()
            .find(|metadata_type| name == source_directory(*metadata_type))
        else {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        }
        let expected_names = listed.get(&metadata_type).cloned().unwrap_or_default();
        let mut expected_files = HashSet::new();
        for member in expected_names {
            for (metadata_path, _) in component_file_pairs(metadata_type, &member) {
                if let Some(file_name) = metadata_path.file_name() {
                    expected_files.insert(file_name.to_os_string());
                }
            }
        }
        for component in read_directory(&path)? {
            let component_path = component.path();
            let component_file_type = component
                .file_type()
                .map_err(|source| MetadataMappingError::io("inspect", &component_path, source))?;
            if component_file_type.is_symlink()
                || !component_file_type.is_file()
                || !expected_files.contains(&component.file_name())
            {
                return Err(MetadataMappingError::UnlistedFile {
                    path: component_path,
                });
            }
        }
    }
    Ok(())
}

fn validate_metadata_directories(unpackaged_directory: &Path) -> Result<(), MetadataMappingError> {
    for entry in read_directory(unpackaged_directory)? {
        let path = entry.path();
        let name = entry.file_name();
        let file_type = entry
            .file_type()
            .map_err(|source| MetadataMappingError::io("inspect", &path, source))?;
        if name == "package.xml" {
            if file_type.is_symlink() || !file_type.is_file() {
                return Err(MetadataMappingError::UnsupportedEntry { path });
            }
            continue;
        }
        let is_supported_directory = [MetadataType::ApexClass, MetadataType::ApexTrigger]
            .into_iter()
            .any(|metadata_type| name == source_directory(metadata_type));
        if !is_supported_directory || file_type.is_symlink() || !file_type.is_dir() {
            return Err(MetadataMappingError::UnsupportedEntry { path });
        }
    }
    Ok(())
}

fn validate_member_name(member: &str, path: &Path) -> Result<(), MetadataMappingError> {
    if member.is_empty()
        || member.trim() != member
        || member == "."
        || member == ".."
        || member.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(
                    byte,
                    b'/' | b'\\' | b':' | b'<' | b'>' | b'"' | b'|' | b'?' | b'*'
                )
        })
        || member.ends_with('.')
        || member.ends_with(' ')
        || is_reserved_windows_name(member)
    {
        return Err(MetadataMappingError::InvalidMemberName {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn is_reserved_windows_name(member: &str) -> bool {
    let base = member
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].into_iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
}

fn source_directory(metadata_type: MetadataType) -> &'static str {
    match metadata_type {
        MetadataType::ApexClass => "classes",
        MetadataType::ApexTrigger => "triggers",
    }
}

fn source_extension(metadata_type: MetadataType) -> &'static str {
    match metadata_type {
        MetadataType::ApexClass => "cls",
        MetadataType::ApexTrigger => "trigger",
    }
}

fn ensure_regular_file(path: &Path) -> Result<(), MetadataMappingError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            MetadataMappingError::MissingComponentFile {
                path: path.to_path_buf(),
            }
        } else {
            MetadataMappingError::io("inspect", path, source)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(MetadataMappingError::SymbolicLink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(MetadataMappingError::UnsupportedEntry {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn read_optional_directory(path: &Path) -> Result<Option<Vec<DirEntry>>, MetadataMappingError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(MetadataMappingError::io("inspect", path, source)),
    };
    if metadata.file_type().is_symlink() {
        return Err(MetadataMappingError::SymbolicLink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_dir() {
        return Err(MetadataMappingError::InvalidDirectory {
            path: path.to_path_buf(),
        });
    }
    read_directory(path).map(Some)
}

fn read_directory(path: &Path) -> Result<Vec<DirEntry>, MetadataMappingError> {
    let entries =
        fs::read_dir(path).map_err(|source| MetadataMappingError::io("read", path, source))?;
    let mut entries = entries
        .map(|entry| entry.map_err(|source| MetadataMappingError::io("read", path, source)))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(DirEntry::file_name);
    Ok(entries)
}

#[derive(Default)]
struct ComponentFiles {
    primary: Option<PathBuf>,
    sidecar: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum MetadataMappingError {
    /// The package manifest could not be decoded or constructed.
    #[error(transparent)]
    Manifest(#[from] MetadataManifestError),
    /// The configured Salesforce API version is invalid.
    #[error("invalid Salesforce API version")]
    InvalidApiVersion,
    /// A filesystem operation needed by conversion failed.
    #[error("unable to {operation} metadata path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A supplied source or unpackaged path is not a directory.
    #[error("metadata package path is not a directory: {path}")]
    InvalidDirectory { path: PathBuf },
    /// The source package does not use the supported `main/default` layout.
    #[error("metadata source package is missing the supported main/default directory: {path}")]
    MissingSourceRoot { path: PathBuf },
    /// A symbolic link appeared in a source or Metadata API package.
    #[error("symbolic links are not supported in metadata packages: {path}")]
    SymbolicLink { path: PathBuf },
    /// An entry uses a path or type outside the supported metadata slice.
    #[error("unsupported metadata entry: {path}")]
    UnsupportedEntry { path: PathBuf },
    /// A component name cannot be represented safely as a package path.
    #[error("invalid metadata member name at {path}")]
    InvalidMemberName { path: PathBuf },
    /// A component file was listed more than once.
    #[error("duplicate metadata file: {path}")]
    DuplicateFile { path: PathBuf },
    /// One half of an Apex component pair is missing.
    #[error("missing component file: {path}")]
    MissingComponentFile { path: PathBuf },
    /// The Metadata API package contains a component not named in package.xml.
    #[error("metadata package contains an unlisted file: {path}")]
    UnlistedFile { path: PathBuf },
    /// Two members collide on case-insensitive filesystems.
    #[error("metadata members collide by case: {first} and {second}")]
    CaseCollision { first: String, second: String },
}

impl MetadataMappingError {
    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}
