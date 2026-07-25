//! Windows opened-handle ownership and DACL validation for secret-bearing files.

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetTokenInformation, IsValidAcl,
    IsValidSid, IsWellKnownSid, TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Validate the owner and effective DACL of an already-open file or directory.
///
/// The owner must be the current process identity. Every access-allow ACE,
/// including inherited entries, must target that identity, LocalSystem, or the
/// built-in Administrators group. A null DACL and unsupported ACE forms fail
/// closed.
pub fn validate_private_file_handle(file: &std::fs::File, label: &str) -> io::Result<()> {
    let token = current_process_token()?;
    let token_user_buffer = token_user_buffer(&token)?;
    let token_user = unsafe { &*token_user_buffer.as_ptr().cast::<TOKEN_USER>() };
    let current_sid = token_user.User.Sid;
    if current_sid.is_null() || unsafe { IsValidSid(current_sid) } == 0 {
        return Err(invalid_acl(label, "process token has an invalid user SID"));
    }

    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "failed to inspect {label} Windows security descriptor: {}",
                io::Error::from_raw_os_error(result as i32)
            ),
        ));
    }
    let _descriptor = OwnedSecurityDescriptor(descriptor);

    if owner.is_null()
        || unsafe { IsValidSid(owner) } == 0
        || unsafe { EqualSid(owner, current_sid) } == 0
    {
        return Err(invalid_acl(
            label,
            "owner does not match the current process identity",
        ));
    }
    if dacl.is_null() || unsafe { IsValidAcl(dacl) } == 0 {
        return Err(invalid_acl(label, "DACL is null or invalid"));
    }

    validate_dacl(dacl, current_sid, label)
}

fn current_process_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 || token.is_null() {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "failed to open the current Windows process token: {}",
                io::Error::last_os_error()
            ),
        ))
    } else {
        Ok(OwnedHandle(token))
    }
}

fn token_user_buffer(token: &OwnedHandle) -> io::Result<Vec<usize>> {
    let mut required = 0u32;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
    }
    if required < size_of::<TOKEN_USER>() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows process token returned an invalid user record length",
        ));
    }

    let word_size = size_of::<usize>();
    let words = (required as usize).div_ceil(word_size);
    let mut buffer = vec![0usize; words];
    let loaded = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    if loaded == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "failed to read the current Windows process identity: {}",
                io::Error::last_os_error()
            ),
        ));
    }
    Ok(buffer)
}

fn validate_dacl(dacl: *mut ACL, current_sid: PSID, label: &str) -> io::Result<()> {
    let mut info: ACL_SIZE_INFORMATION = unsafe { zeroed() };
    let loaded = unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    if loaded == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "failed to enumerate {label} Windows DACL: {}",
                io::Error::last_os_error()
            ),
        ));
    }

    let mut owner_has_access = false;
    for index in 0..info.AceCount {
        let mut raw_ace: *mut c_void = null_mut();
        if unsafe { GetAce(dacl, index, &mut raw_ace) } == 0 || raw_ace.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "failed to read {label} Windows DACL entry {index}: {}",
                    io::Error::last_os_error()
                ),
            ));
        }
        let header = unsafe { &*(raw_ace as *const ACE_HEADER) };
        match header.AceType {
            ACCESS_ALLOWED_ACE_TYPE => {
                if usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>() {
                    return Err(invalid_acl(label, "contains a truncated access-allow ACE"));
                }
                let ace = unsafe { &*(raw_ace as *const ACCESS_ALLOWED_ACE) };
                if ace.Mask == 0 {
                    continue;
                }
                let sid = (&ace.SidStart as *const u32).cast_mut().cast::<c_void>();
                if unsafe { IsValidSid(sid) } == 0 {
                    return Err(invalid_acl(label, "contains an invalid access SID"));
                }
                if unsafe { EqualSid(sid, current_sid) } != 0 {
                    owner_has_access = true;
                } else if unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } == 0
                    && unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) } == 0
                {
                    return Err(invalid_acl(
                        label,
                        "grants access to a principal other than the owner, LocalSystem, or built-in Administrators",
                    ));
                }
            }
            ACCESS_DENIED_ACE_TYPE => {}
            _ => return Err(invalid_acl(label, "contains an unsupported ACE type")),
        }
    }

    if !owner_has_access {
        return Err(invalid_acl(
            label,
            "does not grant access to the owning process identity",
        ));
    }
    Ok(())
}

fn invalid_acl(label: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{label} Windows ACL validation failed: {reason}"),
    )
}
