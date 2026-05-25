use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use tempfile::NamedTempFile;

pub fn atomic_write_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let mut temp = NamedTempFile::new_in(parent)?;
    temp.as_file_mut().write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|err| err.error)?;

    sync_directory(parent)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::atomic_write_bytes;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn creates_missing_parents_and_replaces_existing_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");

        atomic_write_bytes(&path, br#"{"first":true}"#).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"first":true}"#);

        atomic_write_bytes(&path, br#"{"second":true}"#).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"second":true}"#);
    }
}
