use std::path::Path;

pub const MOP_IPC_GROUP: &str = "mop-ipc";

/// Check if a system group exists and return its GID
#[cfg(unix)]
pub fn get_group_gid(group_name: &str) -> Option<u32> {
    use std::ffi::CString;
    let c_name = CString::new(group_name).ok()?;
    unsafe {
        let grp = libc::getgrnam(c_name.as_ptr());
        if grp.is_null() {
            None
        } else {
            Some((*grp).gr_gid)
        }
    }
}

#[cfg(not(unix))]
pub fn get_group_gid(_group_name: &str) -> Option<u32> {
    None
}

/// Set group ownership (if group exists and caller has permission) and permissions
#[cfg(unix)]
pub fn ensure_group_and_permissions(path: &Path, group_name: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    if let Some(gid) = get_group_gid(group_name) {
        let _ = std::os::unix::fs::chown(path, None, Some(gid));
    }

    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
pub fn ensure_group_and_permissions(_path: &Path, _group_name: &str, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ensure_permissions_on_tempdir() {
        let dir = tempdir().unwrap();
        let test_file = dir.path().join("test.sock");
        std::fs::write(&test_file, b"test").unwrap();

        // Should not panic even if group does not exist
        ensure_group_and_permissions(&test_file, "nonexistent-mop-group-xyz", 0o660);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&test_file).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o660);
        }
    }
}
