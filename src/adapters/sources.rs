use std::path::Path;

use crate::{CommonplaceError, Result};

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
    use super::canonical_file_source_key;

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
