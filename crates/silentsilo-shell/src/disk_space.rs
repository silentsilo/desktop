//! How much room is left where the silo lives. A silo is an encrypted
//! copy, so everything entering it is written twice, and a disk that fills
//! up mid-import leaves a part-written blob: recoverable but alarming.
//!
//! Reported rather than enforced. The number is a snapshot, something else
//! can take the space between the check and the write, and a quota can make
//! it smaller than the volume suggests. Only "this will not fit" is worth
//! acting on.

use std::path::Path;

/// What a volume has left, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    /// Space this user may still use here, which is not always what the
    /// volume has: a quota lowers it.
    pub available: u64,
    pub total: u64,
}

/// `None` when the path cannot be asked about, which is the answer for a
/// drive that has been unplugged and for anything this build cannot query.
/// A missing number is treated as "do not warn": inventing a reassuring
/// figure would be worse, and inventing an alarming one would cry wolf.
pub fn space_at(path: &Path) -> Option<DiskSpace> {
    platform::space_at(path)
}

#[cfg(windows)]
mod platform {
    use super::DiskSpace;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    pub fn space_at(path: &Path) -> Option<DiskSpace> {
        // A path that does not exist yet has no volume to ask about. Walking
        // up to the nearest parent that does is what makes this usable while
        // a silo is being created, before its folder is there.
        let existing = existing_ancestor(path)?;
        let wide: Vec<u16> = existing
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut available = 0u64;
        let mut total = 0u64;
        // `available` rather than the volume's free bytes: on a disk with a
        // quota they differ, and the smaller one is what the write actually
        // has.
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut available),
                Some(&mut total),
                None,
            )
            .ok()?;
        }
        Some(DiskSpace { available, total })
    }

    fn existing_ancestor(path: &Path) -> Option<std::path::PathBuf> {
        path.ancestors()
            .find(|p| p.exists())
            .map(|p| p.to_path_buf())
    }
}

#[cfg(unix)]
mod platform {
    use super::DiskSpace;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// The field widths differ between macOS and Linux (`f_bavail` is 32
    /// bits on one and 64 on the other), so the widening goes through a
    /// generic bound rather than a cast that is redundant on one of them.
    fn wide(n: impl Into<u64>) -> u64 {
        n.into()
    }

    /// `statvfs` on the filesystem holding `path`. `f_bavail` is what an
    /// unprivileged process may still use, which is the honest number: the
    /// reserve APFS and ext4 keep for root is not ours to fill.
    pub fn space_at(path: &Path) -> Option<DiskSpace> {
        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `c_path` is NUL-terminated and outlives the call, and
        // `stats` is writable storage of exactly the type statvfs fills.
        let rc = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        // SAFETY: statvfs returned 0, so it initialised the struct.
        let stats = unsafe { stats.assume_init() };
        let fragment = wide(stats.f_frsize);
        Some(DiskSpace {
            available: wide(stats.f_bavail).saturating_mul(fragment),
            total: wide(stats.f_blocks).saturating_mul(fragment),
        })
    }
}

/// What a warning would be about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceVerdict {
    /// Room for what was asked, with the margin below still intact.
    Fine,
    /// It would fit, but leave the disk uncomfortably close to full.
    Tight,
    /// It would not fit.
    Insufficient,
}

/// How much room to insist on beyond whatever is being written.
///
/// A disk with nothing left is not merely full: Windows needs room for its
/// page file and temporary files, and SQLite needs room for a journal it
/// writes while committing. Filling a volume to the last byte breaks things
/// that have nothing to do with this app. Two gigabytes is small enough not
/// to nag on a modern disk and large enough to leave the machine working.
pub const HEADROOM_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Judges a write of `wanted` bytes against what is left.
///
/// `wanted` is the size on disk of what is about to be encrypted, which is
/// close enough to what the blob will weigh: encryption adds a header and a
/// tag per chunk, not a multiple.
pub fn verdict(space: DiskSpace, wanted: u64) -> SpaceVerdict {
    let needed = wanted.saturating_add(HEADROOM_BYTES);
    if space.available < wanted {
        SpaceVerdict::Insufficient
    } else if space.available < needed {
        SpaceVerdict::Tight
    } else {
        SpaceVerdict::Fine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(available: u64) -> DiskSpace {
        DiskSpace {
            available,
            total: 500 * 1024 * 1024 * 1024,
        }
    }

    #[test]
    fn a_write_larger_than_the_free_space_is_refused() {
        assert_eq!(
            verdict(space(1024), 4096),
            SpaceVerdict::Insufficient,
            "a write that cannot fit was not called out"
        );
    }

    #[test]
    fn a_write_that_fits_but_fills_the_disk_is_flagged() {
        // Room for the bytes and nothing else. The machine needs a page file
        // and SQLite needs a journal, so this is not success.
        assert_eq!(
            verdict(space(HEADROOM_BYTES), HEADROOM_BYTES),
            SpaceVerdict::Tight
        );
    }

    #[test]
    fn an_ordinary_write_on_an_ordinary_disk_says_nothing() {
        assert_eq!(
            verdict(space(50 * 1024 * 1024 * 1024), 100 * 1024 * 1024),
            SpaceVerdict::Fine
        );
    }

    #[test]
    fn a_huge_request_cannot_wrap_the_headroom_into_a_pass() {
        // Adding the headroom has to clamp rather than wrap. Wrapping would
        // turn a request the size of the address space into a small number
        // and wave it through on a disk with a kilobyte left.
        assert_eq!(
            verdict(space(1024), u64::MAX),
            SpaceVerdict::Insufficient,
            "an overflowing request was waved through"
        );
        assert_eq!(
            verdict(space(u64::MAX / 2), u64::MAX),
            SpaceVerdict::Insufficient
        );
    }

    #[test]
    fn a_path_on_this_machine_answers_with_something_plausible() {
        // Cheap smoke test for the platform call: the temporary directory
        // exists on every machine that runs this, and a volume with zero
        // total bytes would mean the query failed while reporting success.
        let dir = std::env::temp_dir();
        if let Some(space) = space_at(&dir) {
            assert!(space.total > 0, "a volume with no size at all");
            assert!(space.available <= space.total);
        }
    }
}
