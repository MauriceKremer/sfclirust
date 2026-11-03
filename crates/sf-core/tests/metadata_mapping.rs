#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};

use sf_api::metadata::{MetadataManifest, MetadataManifestError, MetadataType};
use sf_core::project::metadata::{
    ArchiveError, ArchiveLimits, extract_metadata_archive, map_metadata_to_source,
    map_source_to_metadata,
};
use tempfile::TempDir;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/metadata/");
const METADATA_NAMESPACE: &str = "http://soap.sforce.com/2006/04/metadata";

#[test]
fn parses_and_encodes_a_typed_package_manifest() {
    let manifest = MetadataManifest::parse_xml(include_bytes!("fixtures/metadata/package.xml"))
        .expect("parse package.xml");

    assert_eq!(manifest.api_version().as_str(), "63.0");
    assert_eq!(manifest.types().len(), 2);
    assert_eq!(manifest.types()[0].metadata_type(), MetadataType::ApexClass);
    assert_eq!(manifest.types()[0].members(), ["Greeter"]);
    assert_eq!(
        manifest.types()[1].metadata_type(),
        MetadataType::ApexTrigger
    );
    assert_eq!(manifest.types()[1].members(), ["GreetingTrigger"]);

    let encoded = manifest.encode_xml().expect("encode package.xml");
    let decoded = MetadataManifest::parse_xml(&encoded).expect("parse encoded package.xml");
    assert_eq!(decoded, manifest);
    assert!(
        std::str::from_utf8(&encoded)
            .expect("UTF-8 XML")
            .contains(METADATA_NAMESPACE)
    );
}

#[test]
fn rejects_metadata_types_outside_the_initial_scope() {
    let unsupported = format!(
        r#"<Package xmlns="{METADATA_NAMESPACE}"><types><members>Account</members><name>CustomObject</name></types><version>63.0</version></Package>"#
    );

    let error = MetadataManifest::parse_xml(unsupported.as_bytes())
        .expect_err("CustomObject is not supported in this slice");

    assert!(matches!(
        error,
        MetadataManifestError::UnsupportedMetadataType
    ));
}

#[test]
fn maps_apex_classes_and_triggers_to_their_metadata_api_paths() {
    let package = copy_source_fixture();

    let mapping =
        map_source_to_metadata(package.path(), "63.0").expect("map supported source components");

    assert_eq!(mapping.manifest().types().len(), 2);
    let paths = mapping
        .files()
        .iter()
        .map(|file| {
            file.metadata_path()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "classes/Greeter.cls",
            "classes/Greeter.cls-meta.xml",
            "triggers/GreetingTrigger.trigger",
            "triggers/GreetingTrigger.trigger-meta.xml",
        ]
    );
    assert_eq!(
        mapping.files()[0].source_path(),
        Path::new("main/default/classes/Greeter.cls")
    );
}

#[test]
fn rejects_source_packages_without_the_supported_main_default_layout() {
    let package = TempDir::new().expect("create source package");

    let error = map_source_to_metadata(package.path(), "63.0")
        .expect_err("source root layout must be explicit");

    assert!(error.to_string().contains("main/default"));
}

#[test]
fn maps_a_metadata_api_package_back_to_source_paths() {
    let metadata_dir = Path::new(FIXTURES).join("unpackaged");

    let mapping = map_metadata_to_source(&metadata_dir).expect("map retrieved package");

    assert_eq!(mapping.manifest().api_version().as_str(), "63.0");
    assert_eq!(mapping.files().len(), 4);
    assert_eq!(
        mapping.files()[0].source_path(),
        Path::new("main/default/classes/Greeter.cls")
    );
    assert_eq!(
        mapping.files()[3].source_path(),
        Path::new("main/default/triggers/GreetingTrigger.trigger-meta.xml")
    );
}

#[test]
fn rejects_unmapped_source_types_instead_of_silently_omitting_them() {
    let package = copy_source_fixture();
    let unsupported = package.path().join("main/default/objects/Account");
    fs::create_dir_all(&unsupported).expect("create unsupported object directory");
    fs::write(
        unsupported.join("Account.object-meta.xml"),
        "<CustomObject/>",
    )
    .expect("write unsupported metadata");

    let error = map_source_to_metadata(package.path(), "63.0")
        .expect_err("unsupported source type must not be omitted");

    assert!(error.to_string().contains("unsupported"));
}

#[test]
fn rejects_missing_metadata_files_listed_in_the_manifest() {
    let package = TempDir::new().expect("create metadata package");
    fs::write(
        package.path().join("package.xml"),
        include_bytes!("fixtures/metadata/package.xml"),
    )
    .expect("write package manifest");

    let error = map_metadata_to_source(package.path())
        .expect_err("listed components must have corresponding files");

    assert!(error.to_string().contains("missing"));
}

#[test]
fn rejects_a_source_component_missing_its_metadata_sidecar() {
    let package = copy_source_fixture();
    fs::remove_file(
        package
            .path()
            .join("main/default/classes/Greeter.cls-meta.xml"),
    )
    .expect("remove Apex class sidecar");

    let error = map_source_to_metadata(package.path(), "63.0")
        .expect_err("source components require a metadata sidecar");

    assert!(error.to_string().contains("missing component file"));
}

#[test]
fn extracts_a_valid_archive_without_executing_retrieved_scripts() {
    let archive = make_archive(&[
        (
            "unpackaged/scripts/postinstall.sh",
            b"#!/bin/sh\ntouch script-executed\n",
        ),
        ("unpackaged/classes/Greeter.cls", b"class Greeter {}"),
    ]);
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");

    let summary = extract_metadata_archive(&archive, &destination, ArchiveLimits::default())
        .expect("extract safe archive");

    assert_eq!(summary.entry_count(), 2);
    assert!(destination.join("unpackaged/classes/Greeter.cls").is_file());
    assert!(
        destination
            .join("unpackaged/scripts/postinstall.sh")
            .is_file()
    );
    assert!(!directory.path().join("script-executed").exists());
}

#[test]
fn rejects_traversal_absolute_and_windows_archive_paths_before_writing() {
    for (index, unsafe_path) in [
        "../outside.txt",
        "unpackaged/../../outside.txt",
        "/absolute.txt",
        "C:/outside.txt",
        r"\\server\share\outside.txt",
        r"unpackaged\outside.txt",
    ]
    .into_iter()
    .enumerate()
    {
        let archive = make_archive(&[(unsafe_path, b"untrusted")]);
        let directory = TempDir::new().expect("create extraction parent");
        let destination = directory.path().join(format!("extract-{index}"));

        assert!(
            extract_metadata_archive(&archive, &destination, ArchiveLimits::default()).is_err()
        );
        assert!(!destination.exists());
        assert!(!directory.path().join("outside.txt").exists());
    }
}

#[test]
fn rejects_malformed_archives_without_creating_the_destination() {
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");

    let error =
        extract_metadata_archive(b"not a ZIP archive", &destination, ArchiveLimits::default())
            .expect_err("malformed archive must fail");

    assert!(matches!(error, ArchiveError::InvalidArchive));
    assert!(!destination.exists());
}

#[test]
fn validates_every_archive_entry_before_creating_the_destination() {
    let archive = make_archive(&[
        ("unpackaged/first.txt", b"valid"),
        ("unpackaged/../../outside.txt", b"unsafe"),
    ]);
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");

    assert!(extract_metadata_archive(&archive, &destination, ArchiveLimits::default()).is_err());
    assert!(!destination.exists());
    assert!(!directory.path().join("outside.txt").exists());
}

#[test]
fn rejects_symlink_entries_before_writing() {
    let archive = make_symlink_archive("unpackaged/link", "outside.txt");
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");

    let error = extract_metadata_archive(&archive, &destination, ArchiveLimits::default())
        .expect_err("symlink entries must not be extracted");

    assert!(matches!(error, ArchiveError::SymlinkEntry { .. }));
    assert!(!destination.exists());
}

#[test]
fn enforces_entry_expansion_and_decompression_work_limits_before_writing() {
    let archive = make_archive(&[
        ("unpackaged/one.txt", b"0123456789"),
        ("unpackaged/two.txt", b"abcdefghij"),
    ]);
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");

    let entry_error = extract_metadata_archive(
        &archive,
        &destination,
        ArchiveLimits::new(100_000, 100_000, 1, 100_000).expect("valid limits"),
    )
    .expect_err("entry limit must be enforced");
    assert!(matches!(
        entry_error,
        ArchiveError::EntryLimitExceeded { .. }
    ));
    assert!(!destination.exists());

    let compressed_archive = make_archive(&[("unpackaged/file.txt", b"content")]);
    let compressed_error = extract_metadata_archive(
        &compressed_archive,
        &destination,
        ArchiveLimits::new(compressed_archive.len() as u64 - 1, 100_000, 10, 100_000)
            .expect("valid limits"),
    )
    .expect_err("compressed input size must be bounded");
    assert!(matches!(
        compressed_error,
        ArchiveError::ArchiveSizeLimitExceeded { .. }
    ));
    assert!(!destination.exists());

    let expanded_error = extract_metadata_archive(
        &archive,
        &destination,
        ArchiveLimits::new(100_000, 10, 10, 100_000).expect("valid limits"),
    )
    .expect_err("expanded size must be enforced");
    assert!(matches!(
        expanded_error,
        ArchiveError::ExpandedSizeLimitExceeded { .. }
    ));
    assert!(!destination.exists());

    let compressed_archive =
        make_deflated_archive("unpackaged/repetitive.txt", &vec![b'a'; 32 * 1024]);
    let work_error = extract_metadata_archive(
        &compressed_archive,
        &destination,
        ArchiveLimits::new(100_000, 100_000, 10, 1024).expect("valid limits"),
    )
    .expect_err("actual decompression work must be bounded");
    assert!(matches!(
        work_error,
        ArchiveError::DecompressionWorkLimitExceeded { .. }
    ));
    assert!(!destination.exists());
}

#[test]
fn rejects_an_existing_extraction_destination_without_overwriting_it() {
    let archive = make_archive(&[("unpackaged/file.txt", b"content")]);
    let directory = TempDir::new().expect("create extraction parent");
    let destination = directory.path().join("retrieved");
    fs::create_dir(&destination).expect("create existing destination");
    fs::write(destination.join("keep.txt"), b"keep").expect("write existing content");

    let error = extract_metadata_archive(&archive, &destination, ArchiveLimits::default())
        .expect_err("destination must be new");

    assert!(matches!(error, ArchiveError::DestinationExists { .. }));
    assert_eq!(
        fs::read(destination.join("keep.txt")).expect("read existing content"),
        b"keep"
    );
}

fn copy_source_fixture() -> TempDir {
    let package = TempDir::new().expect("create source package");
    for relative in [
        "main/default/classes/Greeter.cls",
        "main/default/classes/Greeter.cls-meta.xml",
        "main/default/triggers/GreetingTrigger.trigger",
        "main/default/triggers/GreetingTrigger.trigger-meta.xml",
    ] {
        let source = Path::new(FIXTURES).join("source").join(relative);
        let destination = package.path().join(relative);
        fs::create_dir_all(destination.parent().expect("file parent"))
            .expect("create source directory");
        fs::copy(source, destination).expect("copy source fixture");
    }
    package
}

fn make_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(*name, options)
            .expect("start archive entry");
        writer.write_all(contents).expect("write archive entry");
    }
    writer.finish().expect("finish archive").into_inner()
}

fn make_deflated_archive(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file(name, options)
        .expect("start archive entry");
    writer.write_all(contents).expect("write archive entry");
    writer.finish().expect("finish archive").into_inner()
}

fn make_symlink_archive(name: &str, target: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer
        .start_file(name, options)
        .expect("start symlink archive entry");
    writer
        .write_all(target.as_bytes())
        .expect("write symlink target");
    let mut bytes = writer.finish().expect("finish archive").into_inner();
    let central_directory = bytes
        .windows(4)
        .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
        .expect("central directory entry");
    // The test writer masks Unix file type bits, so mark the central entry directly.
    bytes[central_directory + 5] = 3;
    bytes[central_directory + 38..central_directory + 42]
        .copy_from_slice(&((0o120777_u32) << 16).to_le_bytes());
    bytes
}
