//! What Peep keeps on disk between launches, so the conversation list shows at once.

use std::{
    fs::{self, File},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::chat::Snapshot;

const FILE: &str = "snapshot.json";
/// The file names the people you talk to: readable by you alone.
const PRIVATE_FILE: u32 = 0o600;
const PRIVATE_FOLDER: u32 = 0o700;

/// Peep's folder for what it keeps, or `None` if the home folder is unknown.
pub fn folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support/Peep"))
}

/// The snapshot an earlier launch kept, or an empty one.
pub fn read(folder: &Path) -> Snapshot {
    // A missing or unreadable snapshot only means starting without one.
    fs::read(folder.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Replaces the kept snapshot. The file is swapped in whole, so a crash never leaves half
/// of one.
pub fn write(folder: &Path, snapshot: &Snapshot) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    fs::set_permissions(folder, fs::Permissions::from_mode(PRIVATE_FOLDER))?;
    let staged = folder.join(format!("{FILE}.new"));
    let mut file = File::options()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(PRIVATE_FILE)
        .open(&staged)?;
    file.write_all(&serde_json::to_vec(snapshot)?)?;
    file.sync_all()?;
    fs::rename(staged, folder.join(FILE))
}

/// Forgets the kept snapshot, when the person signs out.
pub fn clear(folder: &Path) -> io::Result<()> {
    match fs::remove_file(folder.join(FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Token;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("peep-cache-{name}-{}", std::process::id()));
        fs::remove_dir_all(&folder).ok();
        folder
    }

    #[test]
    fn a_snapshot_is_kept_private_and_read_back() {
        let folder = scratch("round-trip");
        let snapshot = Snapshot {
            token: Some(Token {
                value: "token".into(),
                issued: 1.,
            }),
            ..Default::default()
        };

        assert_eq!(read(&folder), Snapshot::default(), "nothing kept yet");
        write(&folder, &snapshot).unwrap();

        assert_eq!(read(&folder), snapshot);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder.join(FILE)), PRIVATE_FILE);
        assert_eq!(mode(&folder), PRIVATE_FOLDER);

        clear(&folder).unwrap();
        clear(&folder).unwrap();
        assert_eq!(read(&folder), Snapshot::default());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_damaged_snapshot_reads_as_empty() {
        let folder = scratch("damaged");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(FILE), b"{ not json").unwrap();

        assert_eq!(read(&folder), Snapshot::default());
        fs::remove_dir_all(&folder).unwrap();
    }
}
