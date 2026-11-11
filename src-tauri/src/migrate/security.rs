use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;
use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows::Win32::Security::{
    AclSizeInformation, EqualSid, GetAclInformation, GetSecurityDescriptorControl, ACL,
    ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SE_DACL_AUTO_INHERITED, SE_DACL_AUTO_INHERIT_REQ, SE_DACL_PRESENT,
    SE_DACL_PROTECTED,
};

struct DirectorySecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    owner: PSID,
    dacl: *mut ACL,
}

impl Drop for DirectorySecurity {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(HLOCAL(self.descriptor.0));
        }
    }
}

impl DirectorySecurity {
    fn read(file: &File) -> io::Result<Self> {
        let mut result = Self {
            descriptor: PSECURITY_DESCRIPTOR::default(),
            owner: PSID::default(),
            dacl: std::ptr::null_mut(),
        };
        unsafe {
            GetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut result.owner),
                None,
                Some(&mut result.dacl),
                None,
                Some(&mut result.descriptor),
            )
        }
        .ok()
        .map_err(io::Error::other)?;
        Ok(result)
    }

    fn inheritance(&self) -> io::Result<u16> {
        let mut control = 0;
        let mut revision = 0;
        unsafe { GetSecurityDescriptorControl(self.descriptor, &mut control, &mut revision) }
            .map_err(io::Error::other)?;
        let mask =
            SE_DACL_PRESENT | SE_DACL_PROTECTED | SE_DACL_AUTO_INHERITED | SE_DACL_AUTO_INHERIT_REQ;
        Ok(control & mask.0)
    }

    fn aces(&self) -> io::Result<Option<(u8, &[u8])>> {
        if self.dacl.is_null() {
            return Ok(None);
        }
        let mut info = ACL_SIZE_INFORMATION::default();
        unsafe {
            GetAclInformation(
                self.dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                AclSizeInformation,
            )
        }
        .map_err(io::Error::other)?;
        let header_size = std::mem::size_of::<ACL>();
        let ace_bytes = (info.AclBytesInUse as usize)
            .checked_sub(header_size)
            .ok_or_else(|| io::Error::other("invalid directory DACL"))?;
        // The returned ACL lives inside this descriptor. Ignore unused
        // allocation bytes, keeping ACE order and inherited flags intact.
        unsafe {
            let start = self.dacl.cast::<u8>().add(header_size);
            Ok(Some((
                (*self.dacl).AclRevision,
                std::slice::from_raw_parts(start, ace_bytes),
            )))
        }
    }
}

pub(super) fn directories_equal(source: &File, target: &File) -> io::Result<bool> {
    let source = DirectorySecurity::read(source)?;
    let target = DirectorySecurity::read(target)?;
    Ok(unsafe { EqualSid(source.owner, target.owner) }.is_ok()
        && source.inheritance()? == target.inheritance()?
        && source.aces()? == target.aces()?)
}
