use std::io::Read;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde_json::{Map, Value};

use crate::domain::documents::{DocumentInput, TemporalState};
use crate::{CommonplaceError, Result};

#[derive(Debug, Default)]
pub struct FileOptions {
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MetadataOverrides {
    pub title: Option<String>,
    pub source_type: String,
    pub temporal_state: TemporalState,
    pub occurred_at: Option<String>,
    pub metadata: Map<String, Value>,
}

pub struct FileItem {
    pub path: PathBuf,
    pub error: Option<CommonplaceError>,
}

pub fn enumerate(options: &FileOptions, maximum_documents: usize) -> Result<Vec<FileItem>> {
    if options.paths.is_empty() {
        return Err(CommonplaceError::InvalidInput(
            "at least one file or directory is required".into(),
        ));
    }
    let include = globs(&options.include)?;
    let exclude = globs(&options.exclude)?;
    let mut items = Vec::new();
    for root in &options.paths {
        let start = items.len();
        match std::fs::symlink_metadata(root) {
            Ok(metadata) if metadata.is_dir() => {
                let mut stack = Vec::new();
                match std::fs::read_dir(root) {
                    Ok(directory) => stack.push(directory),
                    Err(error) => push_item(
                        &mut items,
                        root.clone(),
                        Some(source_error(root, error)),
                        maximum_documents,
                    )?,
                }
                while let Some(directory) = stack.last_mut() {
                    let entry = match directory.next() {
                        Some(Ok(entry)) => entry,
                        Some(Err(error)) => {
                            push_item(
                                &mut items,
                                root.clone(),
                                Some(source_error(root, error)),
                                maximum_documents,
                            )?;
                            continue;
                        }
                        None => {
                            stack.pop();
                            continue;
                        }
                    };
                    let path = entry.path();
                    let kind = match entry.file_type() {
                        Ok(kind) => kind,
                        Err(error) => {
                            push_item(
                                &mut items,
                                path.clone(),
                                Some(source_error(&path, error)),
                                maximum_documents,
                            )?;
                            continue;
                        }
                    };
                    if kind.is_symlink() {
                        continue;
                    }
                    if kind.is_dir() && options.recursive {
                        match std::fs::read_dir(&path) {
                            Ok(directory) => stack.push(directory),
                            Err(error) => push_item(
                                &mut items,
                                path.clone(),
                                Some(source_error(&path, error)),
                                maximum_documents,
                            )?,
                        }
                    } else if kind.is_file()
                        && matches!(
                            path.extension().and_then(|extension| extension.to_str()),
                            Some("md" | "txt")
                        )
                    {
                        let relative = path
                            .strip_prefix(root)
                            .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
                        if (options.include.is_empty() || include.is_match(relative))
                            && !exclude.is_match(relative)
                        {
                            push_item(&mut items, path, None, maximum_documents)?;
                        }
                    }
                }
                items[start..].sort_by(|left, right| left.path.cmp(&right.path));
            }
            Ok(metadata) if metadata.is_file() => {
                push_item(&mut items, root.clone(), None, maximum_documents)?
            }
            Ok(_) => push_item(
                &mut items,
                root.clone(),
                Some(CommonplaceError::InvalidInput(format!(
                    "{} is not a regular file or directory; explicit symlinks are not followed",
                    root.display()
                ))),
                maximum_documents,
            )?,
            Err(error) => push_item(
                &mut items,
                root.clone(),
                Some(source_error(root, error)),
                maximum_documents,
            )?,
        }
    }
    Ok(items)
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|error| {
                CommonplaceError::InvalidInput(format!("invalid scan glob {pattern:?}: {error}"))
            })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| CommonplaceError::InvalidInput(format!("invalid scan globs: {error}")))
}

fn push_item(
    items: &mut Vec<FileItem>,
    path: PathBuf,
    error: Option<CommonplaceError>,
    maximum: usize,
) -> Result<()> {
    if items.len() >= maximum {
        return Err(CommonplaceError::LimitExceeded(format!(
            "input enumeration exceeds the {maximum}-document limit; narrow the paths or filters"
        )));
    }
    items.push(FileItem { path, error });
    Ok(())
}

pub fn read_file(
    path: &Path,
    source_key: String,
    metadata: &MetadataOverrides,
    maximum_bytes: usize,
) -> Result<DocumentInput> {
    let file_type = std::fs::symlink_metadata(path)
        .map_err(|error| source_error(path, error))?
        .file_type();
    if !file_type.is_file() {
        return Err(CommonplaceError::InvalidInput(format!(
            "{} is no longer a regular file",
            path.display()
        )));
    }
    let file = std::fs::File::open(path).map_err(|error| source_error(path, error))?;
    if !file
        .metadata()
        .map_err(|error| source_error(path, error))?
        .is_file()
    {
        return Err(CommonplaceError::InvalidInput(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let text = read_utf8(file, maximum_bytes).map_err(|error| match error {
        CommonplaceError::LimitExceeded(message) => {
            CommonplaceError::LimitExceeded(format!("{}: {message}", path.display()))
        }
        CommonplaceError::InvalidInput(message) => {
            CommonplaceError::InvalidInput(format!("{}: {message}", path.display()))
        }
        CommonplaceError::Io(error) => source_error(path, error),
        error => error,
    })?;
    let title = match &metadata.title {
        Some(title) => Some(title.clone()),
        None => Some(
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    CommonplaceError::InvalidInput(format!(
                        "{} has a non-UTF-8 filename; provide --title",
                        path.display()
                    ))
                })?
                .into(),
        ),
    };
    Ok(DocumentInput {
        source_key,
        text,
        title,
        source_type: metadata.source_type.clone(),
        temporal_state: metadata.temporal_state,
        occurred_at: metadata.occurred_at.clone(),
        metadata: metadata.metadata.clone(),
    })
}

pub(super) fn read_utf8(reader: impl Read, maximum_bytes: usize) -> Result<String> {
    let bound = u64::try_from(maximum_bytes)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| CommonplaceError::InvalidInput("source-byte limit is too large".into()))?;
    let mut bytes = Vec::new();
    reader.take(bound).read_to_end(&mut bytes)?;
    if bytes.len() > maximum_bytes {
        return Err(CommonplaceError::LimitExceeded(format!(
            "source exceeds the {maximum_bytes}-byte limit"
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        CommonplaceError::InvalidInput(format!("source is not valid UTF-8: {error}"))
    })
}

fn source_error(path: &Path, error: std::io::Error) -> CommonplaceError {
    CommonplaceError::InvalidInput(format!(
        "cannot read {}: {error}; check the path and permissions",
        path.display()
    ))
}

pub fn canonical_file_source_key(path: &Path) -> Result<String> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        CommonplaceError::InvalidInput(format!(
            "cannot resolve source path {}: {error}",
            path.display()
        ))
    })?;

    url::Url::from_file_path(&canonical)
        .map(String::from)
        .map_err(|()| {
            CommonplaceError::InvalidInput(format!(
                "cannot convert source path {} to a file URI",
                canonical.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_utf8_reads_include_the_limit_and_retain_bytes() {
        for text in ["", "\u{feff}\0\r\ne\u{301}🦀", "plain"] {
            assert_eq!(read_utf8(text.as_bytes(), text.len()).unwrap(), text);
            if !text.is_empty() {
                assert_eq!(
                    read_utf8(text.as_bytes(), text.len() - 1)
                        .unwrap_err()
                        .code(),
                    "limit_exceeded"
                );
            }
        }
        let mut reader = std::io::Cursor::new(vec![b'a'; 100]);
        assert_eq!(
            read_utf8(&mut reader, 16).unwrap_err().code(),
            "limit_exceeded"
        );
        assert_eq!(reader.position(), 17);
        assert_eq!(
            read_utf8([0xff].as_slice(), 10).unwrap_err().code(),
            "invalid_input"
        );
    }

    #[test]
    fn bounded_sorted_scans_respect_recursion_and_globs() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        for file in [
            "z.md",
            "a.txt",
            "ignore.json",
            "nested/b.md",
            "nested/c.txt",
        ] {
            std::fs::write(root.path().join(file), "").unwrap();
        }
        let mut options = FileOptions {
            paths: vec![root.path().into()],
            ..Default::default()
        };
        let names = |items: Vec<FileItem>| {
            items
                .into_iter()
                .map(|item| item.path.strip_prefix(root.path()).unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(enumerate(&options, 2).unwrap()),
            [PathBuf::from("a.txt"), PathBuf::from("z.md")]
        );
        assert_eq!(
            enumerate(&options, 1).err().unwrap().code(),
            "limit_exceeded"
        );
        options.recursive = true;
        options.include = vec!["**/*.md".into(), "a.txt".into()];
        options.exclude = vec!["nested/*".into()];
        assert_eq!(
            names(enumerate(&options, 2).unwrap()),
            [PathBuf::from("a.txt"), PathBuf::from("z.md")]
        );
        options.include = vec!["[".into()];
        assert_eq!(
            enumerate(&options, 100).err().unwrap().code(),
            "invalid_input"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_ingested_or_traversed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.md"), "").unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
        std::os::unix::fs::symlink(root.path().join("a.md"), root.path().join("b.md")).unwrap();
        let options = FileOptions {
            paths: vec![root.path().into()],
            recursive: true,
            ..Default::default()
        };
        assert_eq!(enumerate(&options, 10).unwrap().len(), 1);
        let explicit = FileOptions {
            paths: vec![root.path().join("b.md")],
            ..Default::default()
        };
        assert_eq!(
            enumerate(&explicit, 10).unwrap()[0]
                .error
                .as_ref()
                .unwrap()
                .code(),
            "invalid_input"
        );
    }

    #[test]
    fn direct_and_discovered_paths_have_the_same_identity() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file = directory.path().join("notes.md");
        std::fs::write(&file, "notes").expect("write source");

        let direct = canonical_file_source_key(&file).expect("direct key");
        let discovered =
            canonical_file_source_key(&directory.path().join("./notes.md")).expect("scan key");

        assert_eq!(direct, discovered);
        assert!(direct.starts_with("file:"));
    }
}
