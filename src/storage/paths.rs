use super::StoreError;
use fs2::FileExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
#[cfg(windows)]
pub(crate) mod windows;
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

pub fn private_dir(path: &Path) -> Result<(), StoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)?;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(StoreError::InvalidData);
    }
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    #[cfg(windows)]
    windows::restrict(path)?;
    Ok(())
}
pub fn private_file(path: &Path) -> Result<File, StoreError> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err(StoreError::InvalidData);
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options.mode(0o600);
    #[cfg(windows)]
    if path.exists() {
        windows::restrict(path)?;
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    #[cfg(windows)]
    windows::restrict(path)?;
    Ok(file)
}
pub struct DataDirGuard {
    _lock: File,
}
pub(crate) fn lock_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}
impl DataDirGuard {
    pub fn acquire(path: &Path) -> Result<Self, StoreError> {
        private_dir(path)?;
        let lock = private_file(&path.join("instance.lock"))?;
        lock.try_lock_exclusive().map_err(|e| {
            if lock_contended(&e) {
                StoreError::Locked
            } else {
                StoreError::Io(e)
            }
        })?;
        Ok(Self { _lock: lock })
    }
}
