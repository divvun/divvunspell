//! The member table of a box archive held in memory, for
//! [`Filesystem::from_box`](super::Filesystem::from_box).
//!
//! Version 1 metadata is read by `box_format`'s own parser. Its reader of
//! version 0, the version of the BHFST archives in circulation, needs the
//! host-file-system half of `box_format`, so version 0 is read here from the
//! same parsing primitives: its records are the ones version 1 has, but
//! directories list their entries and the root is a list of record numbers.

use std::io::{self, Result};

use box_format::Compression;
use box_format::parse::{self, ParseError, ParseResult};
use hashbrown::HashSet;

/// A file of the archive: where its bytes are, and whether they are
/// compressed.
pub(super) struct Member {
    /// The path, components joined with `/`.
    pub path: String,
    /// Offset of the first byte from the start of the archive.
    pub offset: usize,
    pub len: usize,
    /// The compression's name, `None` for a member stored as it is.
    pub compression: Option<String>,
}

/// Every file of the box archive `bytes`.
pub(super) fn members(bytes: &[u8]) -> Result<Vec<Member>> {
    let (header, _) = parse::parse_header(bytes).map_err(|e| parse_error("header", e))?;
    let trailer = usize::try_from(header.trailer_offset)
        .ok()
        .filter(|&at| at != 0 && at <= bytes.len())
        .ok_or_else(|| {
            invalid(format!(
                "the box trailer offset {} lies outside the {}-byte archive",
                header.trailer_offset,
                bytes.len()
            ))
        })?;
    let meta = &bytes[trailer..];
    match header.version {
        0 => v0(meta),
        _ => v1(meta),
    }
}

fn v1(meta: &[u8]) -> Result<Vec<Member>> {
    let (metadata, _) = parse::parse_metadata_v1(meta).map_err(|e| parse_error("metadata", e))?;
    metadata
        .iter()
        .filter_map(|item| {
            let file = item.record.as_file()?;
            let path = item.path.iter().collect::<Vec<_>>().join("/");
            Some(member(path, file))
        })
        .collect()
}

/// A version 0 record, as much of it as the member table needs.
enum Record<'a> {
    File(box_format::FileRecord<'a>),
    Directory { name: &'a str, entries: Vec<u64> },
    Link,
}

fn v0(meta: &[u8]) -> Result<Vec<Member>> {
    let mut at = 0;
    let root = record_numbers(meta, &mut at)?;
    let count = take(meta, &mut at, "record count", parse::parse_vu64)?;
    let mut records = Vec::new();
    for _ in 0..count {
        let kind = take(meta, &mut at, "record type", parse::parse_u8)?;
        let record = match kind {
            0 => {
                let id = take(meta, &mut at, "compression", parse::parse_u8)?;
                let compression = match id {
                    0 => Compression::Stored,
                    id => Compression::Unknown(id),
                };
                Record::File(take(meta, &mut at, "file record", |data| {
                    parse::parse_file_record(data, compression)
                })?)
            }
            1 => {
                let name = take(meta, &mut at, "directory name", parse::parse_str)?;
                let entries = record_numbers(meta, &mut at)?;
                take(meta, &mut at, "directory attributes", parse::parse_attrmap)?;
                Record::Directory { name, entries }
            }
            2 => {
                take(meta, &mut at, "link record", parse::parse_link_record)?;
                Record::Link
            }
            3 => {
                take(
                    meta,
                    &mut at,
                    "external link record",
                    parse::parse_external_link_record,
                )?;
                Record::Link
            }
            other => {
                return Err(invalid(format!(
                    "box record type {other} is not one of 0 to 3"
                )));
            }
        };
        records.push(record);
    }

    let mut members = Vec::new();
    let mut seen = HashSet::new();
    walk(&records, &root, "", &mut seen, &mut members)?;
    Ok(members)
}

/// The files under the records numbered `entries`, whose directory is
/// `parent`. Each record is walked once, so a directory listing itself cannot
/// loop.
fn walk(
    records: &[Record<'_>],
    entries: &[u64],
    parent: &str,
    seen: &mut HashSet<u64>,
    members: &mut Vec<Member>,
) -> Result<()> {
    for &number in entries {
        if !seen.insert(number) {
            continue;
        }
        let record = number
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| records.get(i))
            .ok_or_else(|| invalid(format!("box record {number} does not exist")))?;
        match record {
            Record::File(file) => {
                members.push(member(join(parent, &file.name), file)?);
            }
            Record::Directory { name, entries } => {
                walk(records, entries, &join(parent, name), seen, members)?;
            }
            Record::Link => {}
        }
    }
    Ok(())
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn member(path: String, file: &box_format::FileRecord<'_>) -> Result<Member> {
    let offset = usize::try_from(file.data.get());
    let len = usize::try_from(file.length);
    let (Ok(offset), Ok(len)) = (offset, len) else {
        return Err(invalid(format!(
            "box member '{path}' does not fit in this platform's address space"
        )));
    };
    let compression = match file.compression.id() {
        0x00 => None,
        0x10 => Some("zstd".to_string()),
        0x20 => Some("xz".to_string()),
        id => Some(format!("unknown ({id:#04x})")),
    };
    Ok(Member {
        path,
        offset,
        len,
        compression,
    })
}

/// A list of record numbers: a count, then the numbers.
fn record_numbers(meta: &[u8], at: &mut usize) -> Result<Vec<u64>> {
    let count = take(meta, at, "record list length", parse::parse_vu64)?;
    let mut numbers = Vec::new();
    for _ in 0..count {
        numbers.push(take(meta, at, "record number", parse::parse_vu64)?);
    }
    Ok(numbers)
}

/// Parse one `what` at `at` and move past it.
fn take<'a, T>(
    meta: &'a [u8],
    at: &mut usize,
    what: &str,
    parser: impl FnOnce(&'a [u8]) -> ParseResult<T>,
) -> Result<T> {
    let rest = meta
        .get(*at..)
        .ok_or_else(|| invalid(format!("box metadata ends before its {what}")))?;
    let (value, used) = parser(rest).map_err(|e| parse_error(what, e))?;
    *at += used;
    Ok(value)
}

fn parse_error(what: &str, error: ParseError) -> io::Error {
    invalid(format!("box archive {what}: {error}"))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// A version 0 box archive of `files`, each a path, its bytes and the
/// compression byte its record carries, with the directories the paths imply.
#[cfg(test)]
pub(crate) fn v0_archive(files: &[(&str, &[u8], u8)]) -> Vec<u8> {
    enum Built {
        File {
            name: String,
            offset: u64,
            len: u64,
            compression: u8,
        },
        Directory {
            name: String,
            entries: Vec<u64>,
        },
    }

    fn vlq(out: &mut Vec<u8>, n: u64) {
        assert!(n < 0x80, "the test writer encodes one-byte numbers only");
        out.push(0x80 | n as u8);
    }
    fn string(out: &mut Vec<u8>, s: &str) {
        vlq(out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }
    fn no_attrs(out: &mut Vec<u8>) {
        out.extend_from_slice(&1u64.to_le_bytes());
        vlq(out, 0);
    }

    let mut out = vec![0u8; 32];
    let mut records: Vec<Built> = Vec::new();
    let mut root: Vec<u64> = Vec::new();
    for (path, bytes, compression) in files {
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        let offset = out.len() as u64;
        out.extend_from_slice(bytes);

        let parts: Vec<&str> = path.split('/').collect();
        let mut parent: Option<usize> = None;
        for dir in &parts[..parts.len() - 1] {
            let entries = match parent {
                None => &root,
                Some(p) => match &records[p] {
                    Built::Directory { entries, .. } => entries,
                    Built::File { .. } => panic!("a file has no entries"),
                },
            };
            let found = entries
                .iter()
                .map(|&n| n as usize - 1)
                .find(|&i| matches!(&records[i], Built::Directory { name, .. } if name == dir));
            let at = match found {
                Some(at) => at,
                None => {
                    records.push(Built::Directory {
                        name: dir.to_string(),
                        entries: Vec::new(),
                    });
                    let number = records.len() as u64;
                    match parent {
                        None => root.push(number),
                        Some(p) => match &mut records[p] {
                            Built::Directory { entries, .. } => entries.push(number),
                            Built::File { .. } => panic!("a file has no entries"),
                        },
                    }
                    records.len() - 1
                }
            };
            parent = Some(at);
        }
        records.push(Built::File {
            name: parts[parts.len() - 1].to_string(),
            offset,
            len: bytes.len() as u64,
            compression: *compression,
        });
        let number = records.len() as u64;
        match parent {
            None => root.push(number),
            Some(p) => match &mut records[p] {
                Built::Directory { entries, .. } => entries.push(number),
                Built::File { .. } => panic!("a file has no entries"),
            },
        }
    }

    let trailer = out.len() as u64;
    vlq(&mut out, root.len() as u64);
    for n in &root {
        vlq(&mut out, *n);
    }
    vlq(&mut out, records.len() as u64);
    for record in &records {
        match record {
            Built::File {
                name,
                offset,
                len,
                compression,
            } => {
                out.push(0);
                out.push(*compression);
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&offset.to_le_bytes());
                string(&mut out, name);
                no_attrs(&mut out);
            }
            Built::Directory { name, entries } => {
                out.push(1);
                string(&mut out, name);
                vlq(&mut out, entries.len() as u64);
                for n in entries {
                    vlq(&mut out, *n);
                }
                no_attrs(&mut out);
            }
        }
    }
    vlq(&mut out, 0);
    no_attrs(&mut out);

    out[..4].copy_from_slice(b"\xffBOX");
    out[8..12].copy_from_slice(&8u32.to_le_bytes());
    out[16..24].copy_from_slice(&trailer.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(members: &[Member]) -> Vec<(String, Option<String>)> {
        let mut paths: Vec<_> = members
            .iter()
            .map(|m| (m.path.clone(), m.compression.clone()))
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn a_version_0_archive_lists_its_files_by_path() {
        let archive = v0_archive(&[
            ("meta.json", b"{}", 0),
            ("dir/a", b"aaa", 0),
            ("dir/sub/b", b"bb", 0),
            ("dir/z", b"zz", 0x10),
        ]);
        let members = members(&archive).expect("reads");
        assert_eq!(
            paths(&members),
            vec![
                ("dir/a".to_string(), None),
                ("dir/sub/b".to_string(), None),
                ("dir/z".to_string(), Some("zstd".to_string())),
                ("meta.json".to_string(), None),
            ]
        );
        let b = members.iter().find(|m| m.path == "dir/sub/b").expect("b");
        assert_eq!(&archive[b.offset..b.offset + b.len], b"bb");
    }

    #[test]
    fn a_version_1_archive_lists_its_files_by_path() {
        use box_format::{BoxPath, Compression, CompressionConfig, HashMap, sync::BoxWriter};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("v1.box");
        let mut writer = BoxWriter::create_with_alignment(&path, 8).expect("create");
        writer
            .mkdir(BoxPath::new("dir").expect("box path"), HashMap::new())
            .expect("mkdir");
        for (name, bytes) in [("meta.json", &b"{}"[..]), ("dir/a", &b"aaa"[..])] {
            writer
                .insert(
                    &CompressionConfig::new(Compression::Stored),
                    BoxPath::new(name).expect("box path"),
                    std::io::Cursor::new(bytes.to_vec()),
                    HashMap::new(),
                )
                .expect("insert");
        }
        writer.finish().expect("finish");

        let archive = std::fs::read(&path).expect("read");
        assert_eq!(archive[4], 1, "the writer writes version 1");
        let members = members(&archive).expect("reads");
        assert_eq!(
            paths(&members),
            vec![("dir/a".to_string(), None), ("meta.json".to_string(), None)]
        );
        let a = members.iter().find(|m| m.path == "dir/a").expect("a");
        assert_eq!(&archive[a.offset..a.offset + a.len], b"aaa");
    }

    #[test]
    fn a_damaged_archive_is_an_error_not_a_panic() {
        let archive = v0_archive(&[("dir/a", b"aaa", 0)]);
        // The archive ends with the last record's attributes (9 bytes), then
        // the archive's attribute keys and attributes (10), which the member
        // table does not read; 20 bytes from the end is inside the record.
        for cut in [0, 10, 31, archive.len() - 20] {
            assert!(members(&archive[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad_trailer = archive.clone();
        bad_trailer[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(members(&bad_trailer).is_err());
    }
}
