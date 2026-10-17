//! Mounted file systems as storage volumes.
use super::{c_chars, fs};
use std::ffi::CString;

/// Mount flag for removable media; the system headers define it, the libc crate does not.
const MNT_REMOVABLE: u32 = 0x0000_0200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Removable,
    Fixed,
    Network,
    Optical,
}

impl Kind {
    /// Win32 `DriveType`.
    pub fn drive_type(self) -> u32 {
        match self {
            Kind::Removable => 2,
            Kind::Fixed => 3,
            Kind::Network => 4,
            Kind::Optical => 5,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Kind::Removable => "Removable Disk",
            Kind::Fixed => "Local Fixed Disk",
            Kind::Network => "Network Connection",
            Kind::Optical => "Compact Disc",
        }
    }

    /// Win32 `MediaType` of logical disks; network drives have none.
    pub fn media_type(self) -> Option<u32> {
        match self {
            Kind::Fixed => Some(12),
            Kind::Removable | Kind::Optical => Some(11),
            Kind::Network => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mount {
    pub mount_point: String,
    pub device: String,
    /// File system type name as the kernel reports it, lower case.
    pub fs_type: String,
    pub flags: u32,
    pub blocks: u64,
    pub block_size: u64,
    pub available_blocks: u64,
    pub fsid: u32,
}

impl Mount {
    pub fn kind(&self) -> Kind {
        match self.fs_type.as_str() {
            "smbfs" | "nfs" | "afpfs" | "webdav" | "cifs" => Kind::Network,
            "cd9660" | "udf" => Kind::Optical,
            _ if self.flags & MNT_REMOVABLE != 0 => Kind::Removable,
            _ => Kind::Fixed,
        }
    }

    pub fn size(&self) -> u64 {
        self.blocks.saturating_mul(self.block_size)
    }

    pub fn free_space(&self) -> u64 {
        self.available_blocks.saturating_mul(self.block_size)
    }

    /// Upper-case type name; FAT variants all report as `FAT32`.
    pub fn file_system(&self) -> String {
        match self.fs_type.as_str() {
            "msdos" => "FAT32".into(),
            "hfs" => "HFS+".into(),
            other => other.to_ascii_uppercase(),
        }
    }

    pub fn hidden_from_browsing(&self) -> bool {
        self.flags & libc::MNT_DONTBROWSE as u32 != 0
    }

    pub fn is_root(&self) -> bool {
        self.mount_point == "/"
    }

    /// Volume name, or the last path component of the mount point.
    pub fn label(&self) -> String {
        volume_name(&self.mount_point).unwrap_or_else(|| self.fallback_label())
    }

    pub fn fallback_label(&self) -> String {
        if self.is_root() {
            return "/".into();
        }
        self.mount_point
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(&self.mount_point)
            .to_owned()
    }
}

/// Every mounted file system except `devfs` and `autofs`.
pub fn mounts() -> Vec<Mount> {
    fs::mounts()
        .iter()
        .map(|m| Mount {
            mount_point: c_chars(&m.f_mntonname),
            device: c_chars(&m.f_mntfromname),
            fs_type: c_chars(&m.f_fstypename).to_ascii_lowercase(),
            flags: m.f_flags,
            blocks: m.f_blocks,
            block_size: u64::from(m.f_bsize),
            available_blocks: m.f_bavail,
            fsid: first_fsid_word(m.f_fsid),
        })
        .filter(|m| !matches!(m.fs_type.as_str(), "devfs" | "autofs"))
        .collect()
}

/// Longest file name component the file system accepts.
pub fn max_name_length(path: &str) -> Option<u32> {
    let path = CString::new(path).ok()?;
    let value = unsafe { libc::pathconf(path.as_ptr(), libc::_PC_NAME_MAX) };
    u32::try_from(value).ok().filter(|v| *v > 0)
}

/// Volume name of the file system mounted at the path.
pub fn volume_name(path: &str) -> Option<String> {
    /// Reply layout for `ATTR_VOL_NAME`: total length, then a reference to the name bytes.
    const SIZE: usize = 1024;
    let path = CString::new(path).ok()?;
    let mut request = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_NAME,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut reply = [0u32; SIZE / 4];
    let status = unsafe {
        libc::getattrlist(
            path.as_ptr(),
            std::ptr::from_mut(&mut request).cast(),
            reply.as_mut_ptr().cast(),
            SIZE,
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let bytes: Vec<u8> = reply.iter().flat_map(|word| word.to_ne_bytes()).collect();
    let word = |at: usize| u32::from_ne_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    let total = (word(0) as usize).min(SIZE);
    // The reference sits at offset 4; its data offset is relative to the reference itself.
    let start = 4usize.checked_add_signed(word(4) as i32 as isize)?;
    let end = start.checked_add(word(8) as usize)?;
    if end > total {
        return None;
    }
    let name = super::iokit::nul_terminated(&bytes[start..end]);
    (!name.is_empty()).then_some(name)
}

/// First 32-bit word of a file system id; the libc type keeps its words private.
fn first_fsid_word(id: libc::fsid_t) -> u32 {
    let words = unsafe { std::mem::transmute::<libc::fsid_t, [i32; 2]>(id) };
    words[0] as u32
}
