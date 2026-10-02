//! Restrict local credentials, history and media to the current Windows user.
use std::{
    ffi::c_void,
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
        SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: These allocations are returned by the LocalAlloc-based SID/SDDL APIs.
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(crate) struct SecurityDescriptor(LocalAllocation);
impl SecurityDescriptor {
    pub(crate) fn current_user() -> io::Result<Self> {
        // SAFETY: All API outputs have initialized storage, handles and allocations
        // remain owned until after their last use, and the token buffer is aligned.
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = OwnedHandle::from_raw_handle(token);
            let mut size = 0;
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut size,
            );
            if (size as usize) < size_of::<TOKEN_USER>() {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut sid = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
                return Err(io::Error::last_os_error());
            }
            let _sid_allocation = LocalAllocation(sid.cast());
            let mut length = 0;
            while *sid.add(length) != 0 {
                length += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid, length));
            // Protected DACL: only this user, inherited by newly created children.
            let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(LocalAllocation(descriptor)))
        }
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.0.0
    }
}

pub(super) fn restrict(path: &Path) -> io::Result<()> {
    // Junctions and other reparse points must not redirect permission changes.
    if std::fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Local data cannot be a reparse point",
        ));
    }
    let descriptor = SecurityDescriptor::current_user()?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: The path is NUL terminated and the descriptor outlives this call.
    if unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.as_ptr(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE,
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
        EqualSid, GetAce, GetSecurityDescriptorControl, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

    #[test]
    fn local_data_has_a_protected_acl_granting_only_the_current_user() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("private");
        super::super::private_dir(&directory).unwrap();
        let file = directory.join("credentials.sqlite3");
        drop(super::super::private_file(&file).unwrap());
        // Elevated processes can create files owned by Administrators. Query
        // the actual token user independently of the SDDL construction instead.
        let mut token = ptr::null_mut();
        unsafe {
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                0
            );
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut size = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut size,
            );
        }
        assert!((size as usize) >= size_of::<TOKEN_USER>());
        let mut user_buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        unsafe {
            assert_ne!(
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenUser,
                    user_buffer.as_mut_ptr().cast(),
                    size,
                    &mut size
                ),
                0
            );
        }
        let user = unsafe { &*user_buffer.as_ptr().cast::<TOKEN_USER>() };
        for path in [directory, file] {
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: The allocated descriptor owns all returned owner/ACL/ACE
            // pointers and remains alive until after the assertions.
            unsafe {
                let mut acl = ptr::null_mut();
                let mut descriptor = ptr::null_mut();
                assert_eq!(
                    GetNamedSecurityInfoW(
                        path.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut acl,
                        ptr::null_mut(),
                        &mut descriptor
                    ),
                    0
                );
                let _allocation = LocalAllocation(descriptor);
                let mut control = 0;
                let mut revision = 0;
                assert_ne!(
                    GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
                    0
                );
                assert_ne!(control & SE_DACL_PROTECTED, 0);
                assert!(!acl.is_null());
                assert_eq!((*acl).AceCount, 1);
                let mut ace = ptr::null_mut();
                assert_ne!(GetAce(acl, 0, &mut ace), 0);
                let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                assert_ne!(
                    EqualSid(
                        user.User.Sid,
                        (&ace.SidStart as *const u32).cast_mut().cast()
                    ),
                    0
                );
                assert_eq!(ace.Mask, FILE_ALL_ACCESS);
            }
        }
    }
}
