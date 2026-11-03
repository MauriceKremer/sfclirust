use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;
use zip::ZipArchive;

const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_ENTRIES: usize = 10_000;
const DEFAULT_MAX_DECOMPRESSION_WORK_BYTES: u64 = 256 * 1024 * 1024;
const HARD_MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const HARD_MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
const HARD_MAX_ENTRIES: usize = 10_000;
const HARD_MAX_DECOMPRESSION_WORK_BYTES: u64 = 512 * 1024 * 1024;
const IO_BUFFER_SIZE: usize = 8 * 1024;

/// Resource ceilings for validating and extracting a Metadata API archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveLimits {
    max_archive_bytes: u64,
    max_expanded_bytes: u64,
    max_entries: usize,
    max_decompression_work_bytes: u64,
}

impl ArchiveLimits {
    /// Creates limits that are nonzero and do not exceed hard safety bounds.
    pub fn new(
        max_archive_bytes: u64,
        max_expanded_bytes: u64,
        max_entries: usize,
        max_decompression_work_bytes: u64,
    ) -> Result<Self, ArchiveError> {
        if max_archive_bytes == 0
            || max_archive_bytes > HARD_MAX_ARCHIVE_BYTES
            || max_expanded_bytes == 0
            || max_expanded_bytes > HARD_MAX_EXPANDED_BYTES
            || max_entries == 0
            || max_entries > HARD_MAX_ENTRIES
            || max_decompression_work_bytes == 0
            || max_decompression_work_bytes > HARD_MAX_DECOMPRESSION_WORK_BYTES
        {
            return Err(ArchiveError::InvalidLimits);
        }
        Ok(Self {
            max_archive_bytes,
            max_expanded_bytes,
            max_entries,
            max_decompression_work_bytes,
        })
    }
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            max_expanded_bytes: DEFAULT_MAX_EXPANDED_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_decompression_work_bytes: DEFAULT_MAX_DECOMPRESSION_WORK_BYTES,
        }
    }
}

/// Summary of successfully extracted archive contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveSummary {
    entry_count: usize,
    expanded_bytes: u64,
}

impl ArchiveSummary {
    /// Returns the number of archive entries, including directories.
    pub fn entry_count(self) -> usize {
        self.entry_count
    }

    /// Returns the number of bytes produced by decompressing file entries.
    pub fn expanded_bytes(self) -> u64 {
        self.expanded_bytes
    }
}

/// Validates an entire archive before writing any extracted file.
///
/// The destination must not exist, and its parent directory must already exist.
/// Only regular files and directories are accepted. Extracted permissions are
/// not inherited, and retrieved files are never executed.
pub fn extract_metadata_archive(
    archive_bytes: &[u8],
    destination: impl AsRef<Path>,
    limits: ArchiveLimits,
) -> Result<ArchiveSummary, ArchiveError> {
    if archive_bytes.len() as u64 > limits.max_archive_bytes {
        return Err(ArchiveError::ArchiveSizeLimitExceeded {
            limit: limits.max_archive_bytes,
        });
    }

    let entries = validate_archive(archive_bytes, limits)?;
    let expanded_bytes = entries.iter().try_fold(0_u64, |total, entry| {
        total.checked_add(
            entry
                .contents
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64),
        )
    });
    let summary = ArchiveSummary {
        entry_count: entries.len(),
        expanded_bytes: expanded_bytes.ok_or(ArchiveError::InvalidArchive)?,
    };
    let destination = validate_destination(destination.as_ref())?;
    fs::create_dir(&destination).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            ArchiveError::DestinationExists {
                path: destination.clone(),
            }
        } else {
            ArchiveError::io("create extraction root", &destination, source)
        }
    })?;

    let result = write_entries(&destination, &entries);
    if let Err(primary) = result {
        return match fs::remove_dir_all(&destination) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(ArchiveError::CleanupFailed {
                path: destination,
                primary: Box::new(primary),
                cleanup,
            }),
        };
    }

    Ok(summary)
}

fn validate_archive(
    archive_bytes: &[u8],
    limits: ArchiveLimits,
) -> Result<Vec<ValidatedEntry>, ArchiveError> {
    validate_end_record(archive_bytes, limits)?;
    let mut archive =
        ZipArchive::new(Cursor::new(archive_bytes)).map_err(|_| ArchiveError::InvalidArchive)?;
    if archive.len() > limits.max_entries {
        return Err(ArchiveError::EntryLimitExceeded {
            limit: limits.max_entries,
        });
    }

    let mut entries = Vec::with_capacity(archive.len());
    let mut explicit_paths = HashSet::with_capacity(archive.len());
    let mut case_insensitive_paths = HashMap::<String, PathBuf>::with_capacity(archive.len());
    let mut files = HashSet::new();
    let mut directory_prefixes = HashSet::new();
    let mut declared_compressed_bytes = 0_u64;
    let mut declared_expanded_bytes = 0_u64;
    let mut decompression_work_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| ArchiveError::InvalidArchive)?;
        if file.is_symlink() {
            return Err(ArchiveError::SymlinkEntry { entry_index: index });
        }
        validate_unix_entry_type(&file, index)?;
        let is_directory = file.is_dir();
        let path = archive_entry_path(file.name_raw(), is_directory, index)?;

        if !explicit_paths.insert(path.clone()) {
            return Err(ArchiveError::DuplicateEntryPath { entry_index: index });
        }
        let mut current_path = PathBuf::new();
        for component in path.components() {
            current_path.push(component.as_os_str());
            let key = current_path.to_string_lossy().to_lowercase();
            if let Some(previous) = case_insensitive_paths.get(&key) {
                if previous != &current_path {
                    return Err(ArchiveError::PathCaseCollision { entry_index: index });
                }
            } else {
                case_insensitive_paths.insert(key, current_path.clone());
            }
        }
        validate_path_relationships(
            &path,
            is_directory,
            index,
            &mut files,
            &mut directory_prefixes,
        )?;

        let compressed_size = file.compressed_size();
        declared_compressed_bytes = declared_compressed_bytes
            .checked_add(compressed_size)
            .ok_or(ArchiveError::ArchiveSizeLimitExceeded {
                limit: limits.max_archive_bytes,
            })?;
        if declared_compressed_bytes > limits.max_archive_bytes {
            return Err(ArchiveError::ArchiveSizeLimitExceeded {
                limit: limits.max_archive_bytes,
            });
        }

        let declared_size = file.size();
        if is_directory {
            if declared_size != 0 || compressed_size != 0 {
                return Err(ArchiveError::InvalidDirectoryEntry { entry_index: index });
            }
            entries.push(ValidatedEntry {
                path,
                contents: None,
            });
            continue;
        }

        declared_expanded_bytes = declared_expanded_bytes.checked_add(declared_size).ok_or(
            ArchiveError::ExpandedSizeLimitExceeded {
                limit: limits.max_expanded_bytes,
            },
        )?;
        if declared_expanded_bytes > limits.max_expanded_bytes {
            return Err(ArchiveError::ExpandedSizeLimitExceeded {
                limit: limits.max_expanded_bytes,
            });
        }
        let remaining_work = limits
            .max_decompression_work_bytes
            .saturating_sub(decompression_work_bytes);
        if declared_size > remaining_work {
            return Err(ArchiveError::DecompressionWorkLimitExceeded {
                limit: limits.max_decompression_work_bytes,
            });
        }

        let mut contents = Vec::new();
        let mut actual_entry_size = 0_u64;
        let mut buffer = [0_u8; IO_BUFFER_SIZE];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|_| ArchiveError::InvalidEntryData { entry_index: index })?;
            if count == 0 {
                break;
            }
            let count = count as u64;
            decompression_work_bytes = decompression_work_bytes.checked_add(count).ok_or(
                ArchiveError::DecompressionWorkLimitExceeded {
                    limit: limits.max_decompression_work_bytes,
                },
            )?;
            if decompression_work_bytes > limits.max_decompression_work_bytes {
                return Err(ArchiveError::DecompressionWorkLimitExceeded {
                    limit: limits.max_decompression_work_bytes,
                });
            }
            actual_entry_size = actual_entry_size
                .checked_add(count)
                .ok_or(ArchiveError::EntrySizeMismatch { entry_index: index })?;
            if actual_entry_size > declared_size {
                return Err(ArchiveError::EntrySizeMismatch { entry_index: index });
            }
            contents.extend_from_slice(&buffer[..count as usize]);
        }
        if contents.len() as u64 != declared_size {
            return Err(ArchiveError::EntrySizeMismatch { entry_index: index });
        }
        entries.push(ValidatedEntry {
            path,
            contents: Some(contents),
        });
    }

    Ok(entries)
}

fn validate_end_record(archive_bytes: &[u8], limits: ArchiveLimits) -> Result<(), ArchiveError> {
    const END_RECORD_SIZE: usize = 22;
    const MAX_COMMENT_SIZE: usize = u16::MAX as usize;
    const END_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

    if archive_bytes.len() < END_RECORD_SIZE {
        return Err(ArchiveError::InvalidArchive);
    }
    let search_start = archive_bytes
        .len()
        .saturating_sub(END_RECORD_SIZE + MAX_COMMENT_SIZE);
    let relative_end_record = archive_bytes[search_start..]
        .windows(END_SIGNATURE.len())
        .rposition(|window| window == END_SIGNATURE)
        .ok_or(ArchiveError::InvalidArchive)?;
    let end_record = search_start + relative_end_record;
    let record = archive_bytes
        .get(end_record..end_record + END_RECORD_SIZE)
        .ok_or(ArchiveError::InvalidArchive)?;
    let comment_length = u16::from_le_bytes([record[20], record[21]]) as usize;
    if end_record + END_RECORD_SIZE + comment_length != archive_bytes.len() {
        return Err(ArchiveError::InvalidArchive);
    }

    let disk_number = u16::from_le_bytes([record[4], record[5]]);
    let central_directory_disk = u16::from_le_bytes([record[6], record[7]]);
    let entries_on_disk = u16::from_le_bytes([record[8], record[9]]);
    let total_entries = u16::from_le_bytes([record[10], record[11]]);
    let central_directory_size =
        u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
    let central_directory_offset =
        u32::from_le_bytes([record[16], record[17], record[18], record[19]]);
    if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries {
        return Err(ArchiveError::UnsupportedArchiveFeature);
    }
    if entries_on_disk == u16::MAX
        || total_entries == u16::MAX
        || central_directory_size == u32::MAX
        || central_directory_offset == u32::MAX
        || (end_record >= 20
            && archive_bytes.get(end_record - 20..end_record - 16)
                == Some(&[0x50, 0x4b, 0x06, 0x07]))
    {
        return Err(ArchiveError::UnsupportedArchiveFeature);
    }
    if usize::from(total_entries) > limits.max_entries {
        return Err(ArchiveError::EntryLimitExceeded {
            limit: limits.max_entries,
        });
    }
    let central_directory_end = usize::try_from(central_directory_offset)
        .ok()
        .and_then(|offset| {
            usize::try_from(central_directory_size)
                .ok()
                .and_then(|size| offset.checked_add(size))
        })
        .ok_or(ArchiveError::InvalidArchive)?;
    if central_directory_end > end_record {
        return Err(ArchiveError::InvalidArchive);
    }
    Ok(())
}

fn validate_unix_entry_type<R: Read + io::Seek>(
    file: &zip::read::ZipFile<'_, R>,
    index: usize,
) -> Result<(), ArchiveError> {
    let Some(mode) = file.unix_mode() else {
        return Ok(());
    };
    let entry_type = mode & 0o170000;
    if entry_type == 0 || (entry_type == 0o100000 && !file.is_dir()) {
        return Ok(());
    }
    if entry_type == 0o040000 && file.is_dir() {
        return Ok(());
    }
    if entry_type == 0o120000 {
        return Err(ArchiveError::SymlinkEntry { entry_index: index });
    }
    Err(ArchiveError::UnsupportedEntryType { entry_index: index })
}

fn archive_entry_path(
    raw_name: &[u8],
    is_directory: bool,
    entry_index: usize,
) -> Result<PathBuf, ArchiveError> {
    let name = std::str::from_utf8(raw_name)
        .map_err(|_| ArchiveError::InvalidEntryPath { entry_index })?;
    if name.is_empty() || name.contains('\\') {
        return Err(ArchiveError::InvalidEntryPath { entry_index });
    }
    let path_without_trailing_slash = if is_directory {
        name.strip_suffix('/')
            .ok_or(ArchiveError::InvalidEntryPath { entry_index })?
    } else {
        if name.ends_with('/') {
            return Err(ArchiveError::InvalidEntryPath { entry_index });
        }
        name
    };
    if path_without_trailing_slash.is_empty() || path_without_trailing_slash.starts_with('/') {
        return Err(ArchiveError::InvalidEntryPath { entry_index });
    }

    let mut path = PathBuf::new();
    for component in path_without_trailing_slash.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            || component.ends_with(' ')
            || component.bytes().any(|byte| {
                byte.is_ascii_control()
                    || matches!(byte, b':' | b'<' | b'>' | b'"' | b'|' | b'?' | b'*')
            })
            || is_reserved_windows_name(component)
        {
            return Err(ArchiveError::InvalidEntryPath { entry_index });
        }
        path.push(component);
    }
    Ok(path)
}

fn validate_path_relationships(
    path: &Path,
    is_directory: bool,
    entry_index: usize,
    files: &mut HashSet<PathBuf>,
    directory_prefixes: &mut HashSet<PathBuf>,
) -> Result<(), ArchiveError> {
    let mut parent = path.parent();
    while let Some(directory) = parent {
        if files.contains(directory) {
            return Err(ArchiveError::ConflictingEntryPaths { entry_index });
        }
        directory_prefixes.insert(directory.to_path_buf());
        parent = directory.parent();
    }
    if files.contains(path) || (!is_directory && directory_prefixes.contains(path)) {
        return Err(ArchiveError::ConflictingEntryPaths { entry_index });
    }
    if !is_directory {
        files.insert(path.to_path_buf());
    }
    Ok(())
}

fn is_reserved_windows_name(component: &str) -> bool {
    let base = component
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

fn validate_destination(path: &Path) -> Result<PathBuf, ArchiveError> {
    let file_name = path
        .file_name()
        .ok_or_else(|| ArchiveError::InvalidDestination {
            path: path.to_path_buf(),
        })?;
    if file_name == "." || file_name == ".." {
        return Err(ArchiveError::InvalidDestination {
            path: path.to_path_buf(),
        });
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|source| ArchiveError::io("resolve destination parent", parent, source))?;
    if !parent.is_dir() {
        return Err(ArchiveError::InvalidDestination { path: parent });
    }
    let destination = parent.join(file_name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(ArchiveError::DestinationExists { path: destination });
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ArchiveError::io(
                "inspect destination",
                &destination,
                source,
            ));
        }
    }
    Ok(destination)
}

fn write_entries(destination: &Path, entries: &[ValidatedEntry]) -> Result<(), ArchiveError> {
    for entry in entries {
        let path = destination.join(&entry.path);
        if let Some(contents) = &entry.contents {
            let parent = path
                .parent()
                .ok_or_else(|| ArchiveError::InvalidDestination { path: path.clone() })?;
            fs::create_dir_all(parent)
                .map_err(|source| ArchiveError::io("create directory", parent, source))?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|source| ArchiveError::io("create file", &path, source))?;
            output
                .write_all(contents)
                .map_err(|source| ArchiveError::io("write file", &path, source))?;
        } else {
            fs::create_dir_all(&path)
                .map_err(|source| ArchiveError::io("create directory", &path, source))?;
        }
    }
    Ok(())
}

struct ValidatedEntry {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

/// Safe failure modes for bounded Metadata API archive extraction.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// Limits are zero or exceed the hard safety ceilings.
    #[error("invalid metadata archive limits")]
    InvalidLimits,
    /// The compressed archive or declared compressed entries exceed the limit.
    #[error("metadata archive exceeds the compressed size limit of {limit} bytes")]
    ArchiveSizeLimitExceeded { limit: u64 },
    /// The number of archive entries exceeds the limit.
    #[error("metadata archive exceeds the entry limit of {limit}")]
    EntryLimitExceeded { limit: usize },
    /// Declared or actual expanded data exceeds the limit.
    #[error("metadata archive exceeds the expanded size limit of {limit} bytes")]
    ExpandedSizeLimitExceeded { limit: u64 },
    /// Actual decompressed output exceeds the bounded work budget.
    #[error("metadata archive exceeds the decompression work limit of {limit} bytes")]
    DecompressionWorkLimitExceeded { limit: u64 },
    /// The input is not a valid supported ZIP archive.
    #[error("invalid Metadata API ZIP archive")]
    InvalidArchive,
    /// Multi-disk and ZIP64 archives are outside the bounded initial slice.
    #[error("unsupported Metadata API ZIP archive feature")]
    UnsupportedArchiveFeature,
    /// An entry path is invalid or unsafe on a supported platform.
    #[error("unsafe metadata archive path at entry {entry_index}")]
    InvalidEntryPath { entry_index: usize },
    /// An entry is a symbolic link.
    #[error("symbolic link in metadata archive at entry {entry_index}")]
    SymlinkEntry { entry_index: usize },
    /// An entry uses a file type other than a regular file or directory.
    #[error("unsupported metadata archive entry type at entry {entry_index}")]
    UnsupportedEntryType { entry_index: usize },
    /// An entry path is duplicated.
    #[error("duplicate metadata archive path at entry {entry_index}")]
    DuplicateEntryPath { entry_index: usize },
    /// Different entries collide on a case-insensitive filesystem.
    #[error("metadata archive paths collide by case at entry {entry_index}")]
    PathCaseCollision { entry_index: usize },
    /// A file and directory entry have conflicting path relationships.
    #[error("conflicting metadata archive paths at entry {entry_index}")]
    ConflictingEntryPaths { entry_index: usize },
    /// A directory entry claims file content.
    #[error("invalid metadata archive directory entry at entry {entry_index}")]
    InvalidDirectoryEntry { entry_index: usize },
    /// The decompressed entry length differs from its declared size.
    #[error("metadata archive entry size mismatch at entry {entry_index}")]
    EntrySizeMismatch { entry_index: usize },
    /// Entry content failed decompression or checksum validation.
    #[error("invalid metadata archive entry data at entry {entry_index}")]
    InvalidEntryData { entry_index: usize },
    /// The extraction destination already exists.
    #[error("metadata archive destination already exists: {path}")]
    DestinationExists { path: PathBuf },
    /// The destination path cannot be safely used as a new extraction root.
    #[error("invalid metadata archive destination: {path}")]
    InvalidDestination { path: PathBuf },
    /// A filesystem operation failed.
    #[error("unable to {operation} metadata archive path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Writing failed and cleanup of the newly created extraction root also failed.
    #[error(
        "metadata archive extraction failed and cleanup was incomplete at {path}: {primary}; cleanup: {cleanup}"
    )]
    CleanupFailed {
        path: PathBuf,
        primary: Box<ArchiveError>,
        cleanup: io::Error,
    },
}

impl ArchiveError {
    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}
