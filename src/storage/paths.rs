use super::StoreError;
use fs2::FileExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

pub fn private_dir(path: &Path) -> Result<(), StoreError> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(StoreError::InvalidData);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn private_file(path: &Path) -> Result<File, StoreError> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err(StoreError::InvalidData);
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
pub struct DataDirGuard {
    _lock: File,
}
impl DataDirGuard {
    pub fn acquire(path: &Path) -> Result<Self, StoreError> {
        private_dir(path)?;
        let lock = private_file(&path.join("instance.lock"))?;
        lock.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                StoreError::Locked
            } else {
                StoreError::Io(e)
            }
        })?;
        Ok(Self { _lock: lock })
    }
}
