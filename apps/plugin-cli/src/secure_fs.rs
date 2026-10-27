use std::ffi::OsString;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use cap_fs_ext::MetadataExt;
use cap_fs_ext::{
    FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt, OpenOptionsSyncExt,
};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, OpenOptions};

use crate::PackageError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileId {
    device: u64,
    inode: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HandleInfo {
    id: FileId,
    len: u64,
    links: u64,
    is_file: bool,
    is_dir: bool,
    is_reparse: bool,
}

pub(crate) struct BoundInput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) canonical_path: PathBuf,
}

pub(crate) struct SourceTree {
    root: Dir,
    root_info: HandleInfo,
}

pub(crate) struct OutputTarget {
    parent: Dir,
    leaf: OsString,
}

impl SourceTree {
    pub(crate) fn open(path: &Path) -> Result<Self, PackageError> {
        let file = open_ambient(path, true)?;
        let info = handle_info(&file)?;
        ensure_directory(info)?;
        Ok(Self {
            root: Dir::from_std_file(file.into_std()),
            root_info: info,
        })
    }

    pub(crate) fn prepare_output(&self, output: &Path) -> Result<OutputTarget, PackageError> {
        let leaf = output
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or(PackageError::OutputHasNoParent)?
            .to_os_string();
        if output
            .components()
            .next_back()
            .is_none_or(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(PackageError::InvalidOutputPath);
        }
        let parent_path = output.parent().unwrap_or_else(|| Path::new("."));
        let parent_file = open_ambient(parent_path, true)?;
        let parent_info = handle_info(&parent_file)?;
        ensure_directory(parent_info)?;
        let parent = Dir::from_std_file(parent_file.into_std());
        reject_source_ancestor(&self.root, &parent, self.root_info.id)?;
        match parent.symlink_metadata(&leaf) {
            Ok(_) => return Err(PackageError::OutputExists),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(PackageError::Io(error)),
        }
        Ok(OutputTarget { parent, leaf })
    }

    pub(crate) fn read_root_file(&self, name: &str, limit: usize) -> Result<Vec<u8>, PackageError> {
        if name.contains('/') || name.contains('\\') || name.is_empty() {
            return Err(PackageError::InvalidPath);
        }
        let file = self
            .root
            .open_with(name, &read_options(false))
            .map_err(map_open_error)?;
        read_open_file(file, limit, self.root_info.id.device)
    }

    pub(crate) fn read_optional_root_file(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, PackageError> {
        if name.contains('/') || name.contains('\\') || name.is_empty() {
            return Err(PackageError::InvalidPath);
        }
        let file = match self.root.open_with(name, &read_options(false)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_open_error(error)),
        };
        read_open_file(file, limit, self.root_info.id.device).map(Some)
    }

    pub(crate) fn collect_assets(
        &self,
        entries: &mut Vec<(String, Vec<u8>)>,
        max_entries: usize,
        max_entry_bytes: usize,
    ) -> Result<(), PackageError> {
        let assets_file = match self.root.open_with("assets", &read_options(true)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(map_open_error(error)),
        };
        let assets_info = handle_info(&assets_file)?;
        ensure_child_directory(assets_info, self.root_info.id.device)?;
        let assets = Dir::from_std_file(assets_file.into_std());
        collect_directory(
            &assets,
            "assets",
            self.root_info.id.device,
            entries,
            max_entries,
            max_entry_bytes,
        )
    }
}

impl OutputTarget {
    pub(crate) fn write_atomic(self, bytes: &[u8]) -> Result<(), PackageError> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let mut temporary_name = None;
        let mut temporary_file = None;
        for _ in 0..128 {
            let candidate = OsString::from(format!(
                ".info-panel-plugin.{}.{}.tmp",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match self.parent.open_with(&candidate, &write_options()) {
                Ok(file) => {
                    temporary_name = Some(candidate);
                    temporary_file = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(PackageError::Io(error)),
            }
        }
        let temporary_name = temporary_name.ok_or(PackageError::TemporaryNameExhausted)?;
        let mut cleanup = TemporaryOutput {
            parent: &self.parent,
            name: temporary_name,
            active: true,
        };
        let mut file = temporary_file.expect("temporary name and file are set together");
        let info = handle_info(&file)?;
        ensure_regular_file(info)?;
        file.write_all(bytes).map_err(PackageError::Io)?;
        file.sync_all().map_err(PackageError::Io)?;
        drop(file);
        self.parent
            .hard_link(&cleanup.name, &self.parent, &self.leaf)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    PackageError::OutputExists
                } else {
                    PackageError::Io(error)
                }
            })?;
        self.parent
            .remove_file(&cleanup.name)
            .map_err(PackageError::Io)?;
        cleanup.active = false;
        sync_directory(&self.parent)?;
        Ok(())
    }
}

struct TemporaryOutput<'a> {
    parent: &'a Dir,
    name: OsString,
    active: bool,
}

impl Drop for TemporaryOutput<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.parent.remove_file(&self.name);
        }
    }
}

pub(crate) fn read_bound_file(path: &Path, limit: usize) -> Result<BoundInput, PackageError> {
    let mut file = open_ambient(path, false)?;
    let before = handle_info(&file)?;
    ensure_regular_file(before)?;
    let bytes = read_stable_contents(&mut file, before, limit)?;
    let canonical_path = bound_canonical_path(path, before.id)?;
    Ok(BoundInput {
        bytes,
        canonical_path,
    })
}

pub(crate) fn read_bound_file_with_error(
    path: &Path,
    limit: usize,
    too_large: PackageError,
) -> Result<BoundInput, PackageError> {
    match read_bound_file(path, limit) {
        Err(PackageError::EntryTooLarge) => Err(too_large),
        result => result,
    }
}

fn collect_directory(
    directory: &Dir,
    prefix: &str,
    root_device: u64,
    entries: &mut Vec<(String, Vec<u8>)>,
    max_entries: usize,
    max_entry_bytes: usize,
) -> Result<(), PackageError> {
    let mut children = directory
        .entries()
        .map_err(PackageError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(PackageError::Io)?;
    children.sort_by_key(cap_std::fs::DirEntry::file_name);
    for child in children {
        if entries.len() >= max_entries {
            return Err(PackageError::TooManyEntries);
        }
        let name = child
            .file_name()
            .into_string()
            .map_err(|_| PackageError::InvalidPath)?;
        let package_name = format!("{prefix}/{name}");
        crate::validate_archive_path(package_name.as_bytes(), crate::MAX_PATH_BYTES)?;
        let file_type = child.file_type().map_err(PackageError::Io)?;
        if file_type.is_symlink() {
            return Err(PackageError::LinksNotAllowed);
        }
        if file_type.is_dir() {
            let file = child
                .open_with(&read_options(true))
                .map_err(map_open_error)?;
            let info = handle_info(&file)?;
            ensure_child_directory(info, root_device)?;
            let child_dir = Dir::from_std_file(file.into_std());
            collect_directory(
                &child_dir,
                &package_name,
                root_device,
                entries,
                max_entries,
                max_entry_bytes,
            )?;
        } else {
            let file = child
                .open_with(&read_options(false))
                .map_err(map_open_error)?;
            let bytes = read_open_file(file, max_entry_bytes, root_device)?;
            entries.push((package_name, bytes));
        }
    }
    Ok(())
}

fn read_open_file(mut file: File, limit: usize, root_device: u64) -> Result<Vec<u8>, PackageError> {
    let before = handle_info(&file)?;
    ensure_regular_file(before)?;
    if before.id.device != root_device {
        return Err(PackageError::MountPointNotAllowed);
    }
    read_stable_contents(&mut file, before, limit)
}

fn read_stable_contents(
    file: &mut File,
    before: HandleInfo,
    limit: usize,
) -> Result<Vec<u8>, PackageError> {
    if before.len > limit as u64 {
        return Err(PackageError::EntryTooLarge);
    }
    let mut first = Vec::with_capacity((before.len as usize).min(64 * 1024));
    (&mut *file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut first)
        .map_err(PackageError::Io)?;
    let middle = handle_info(file)?;
    ensure_regular_file(middle)?;
    if middle != before || first.len() > limit || first.len() as u64 != before.len {
        return Err(PackageError::FileChangedOrTooLarge);
    }

    file.seek(SeekFrom::Start(0)).map_err(PackageError::Io)?;
    let mut second = Vec::with_capacity(first.len());
    (&mut *file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut second)
        .map_err(PackageError::Io)?;
    let after = handle_info(file)?;
    ensure_regular_file(after)?;
    if after != before
        || second.len() > limit
        || second.len() as u64 != before.len
        || second != first
    {
        return Err(PackageError::FileChangedOrTooLarge);
    }
    Ok(first)
}

fn open_ambient(path: &Path, maybe_dir: bool) -> Result<File, PackageError> {
    File::open_ambient_with(path, &read_options(maybe_dir), ambient_authority())
        .map_err(map_open_error)
}

fn read_options(maybe_dir: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    options.follow(FollowSymlinks::No);
    options.maybe_dir(maybe_dir);
    options.nonblock(true);
    options
}

fn write_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn map_open_error(error: io::Error) -> PackageError {
    match error.raw_os_error() {
        #[cfg(unix)]
        Some(code) if code == libc::ELOOP => PackageError::LinksNotAllowed,
        _ => PackageError::Io(error),
    }
}

fn ensure_regular_file(info: HandleInfo) -> Result<(), PackageError> {
    if info.is_reparse {
        return Err(PackageError::LinksNotAllowed);
    }
    if !info.is_file || info.is_dir {
        return Err(PackageError::SpecialFileNotAllowed);
    }
    if info.links != 1 {
        return Err(PackageError::LinksNotAllowed);
    }
    Ok(())
}

fn ensure_directory(info: HandleInfo) -> Result<(), PackageError> {
    if info.is_reparse {
        return Err(PackageError::LinksNotAllowed);
    }
    if !info.is_dir || info.is_file {
        return Err(PackageError::SourceNotDirectory);
    }
    Ok(())
}

fn ensure_child_directory(info: HandleInfo, root_device: u64) -> Result<(), PackageError> {
    if info.is_reparse {
        return Err(PackageError::LinksNotAllowed);
    }
    if !info.is_dir || info.is_file {
        return Err(PackageError::SpecialFileNotAllowed);
    }
    if info.id.device != root_device {
        return Err(PackageError::MountPointNotAllowed);
    }
    Ok(())
}

fn reject_source_ancestor(
    source: &Dir,
    output_parent: &Dir,
    source_id: FileId,
) -> Result<(), PackageError> {
    let source_ancestors = collect_known_ancestors(source)?;
    let mut output_visited = Vec::new();
    let mut current_dir = output_parent.try_clone().map_err(PackageError::Io)?;
    loop {
        let current = directory_info(&current_dir)?;
        output_visited.push(current.id);
        match classify_ancestry(source_id, &source_ancestors, &output_visited, false) {
            AncestryDecision::InsideSource => return Err(PackageError::OutputInsideSource),
            AncestryDecision::ProvenOutside => return Ok(()),
            AncestryDecision::Continue => {}
        }
        let next = match open_parent(&current_dir) {
            Ok(next) => next,
            Err(PackageError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied => {
                return prove_with_bound_directory_paths(source, output_parent);
            }
            Err(error) => return Err(error),
        };
        let next_info = directory_info(&next)?;
        if next_info.id == current.id {
            return finish_ancestry_proof(classify_ancestry(
                source_id,
                &source_ancestors,
                &output_visited,
                true,
            ));
        }
        current_dir = next;
    }
}

fn collect_known_ancestors(start: &Dir) -> Result<Vec<FileId>, PackageError> {
    let mut ancestors = Vec::new();
    let mut current_dir = start.try_clone().map_err(PackageError::Io)?;
    loop {
        let current = directory_info(&current_dir)?;
        ancestors.push(current.id);
        let next = match open_parent(&current_dir) {
            Ok(next) => next,
            Err(PackageError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied => {
                return Ok(ancestors);
            }
            Err(error) => return Err(error),
        };
        let next_info = directory_info(&next)?;
        if next_info.id == current.id {
            return Ok(ancestors);
        }
        current_dir = next;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AncestryDecision {
    InsideSource,
    ProvenOutside,
    Continue,
}

fn classify_ancestry(
    source_id: FileId,
    source_ancestors: &[FileId],
    output_visited: &[FileId],
    reached_root: bool,
) -> AncestryDecision {
    if output_visited.contains(&source_id) {
        return AncestryDecision::InsideSource;
    }
    if output_visited.iter().any(|candidate| {
        source_ancestors
            .iter()
            .skip(1)
            .any(|known| known == candidate)
    }) || reached_root
    {
        AncestryDecision::ProvenOutside
    } else {
        AncestryDecision::Continue
    }
}

fn finish_ancestry_proof(decision: AncestryDecision) -> Result<(), PackageError> {
    match decision {
        AncestryDecision::InsideSource => Err(PackageError::OutputInsideSource),
        AncestryDecision::ProvenOutside => Ok(()),
        AncestryDecision::Continue => Err(PackageError::OutputAncestryUnverifiable),
    }
}

fn prove_with_bound_directory_paths(source: &Dir, output: &Dir) -> Result<(), PackageError> {
    let source_path = bound_directory_path(source)?;
    let output_path = bound_directory_path(output)?;
    if output_path.starts_with(&source_path) {
        Err(PackageError::OutputInsideSource)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn bound_directory_path(directory: &Dir) -> Result<PathBuf, PackageError> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let mut buffer = vec![0_u8; libc::PATH_MAX as usize];
    // SAFETY: the directory fd remains live, F_GETPATH writes at most PATH_MAX bytes to
    // the valid mutable buffer, and success guarantees a NUL-terminated path.
    if unsafe {
        libc::fcntl(
            directory.as_raw_fd(),
            libc::F_GETPATH,
            buffer.as_mut_ptr().cast::<libc::c_char>(),
        )
    } < 0
    {
        return Err(PackageError::OutputAncestryUnverifiable);
    }
    let path = CStr::from_bytes_until_nul(&buffer)
        .map_err(|_| PackageError::OutputAncestryUnverifiable)?;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn bound_directory_path(directory: &Dir) -> Result<PathBuf, PackageError> {
    use std::os::fd::AsRawFd;

    let path = std::fs::read_link(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .map_err(|_| PackageError::OutputAncestryUnverifiable)?;
    if path.to_string_lossy().ends_with(" (deleted)") {
        return Err(PackageError::OutputAncestryUnverifiable);
    }
    Ok(path)
}

#[cfg(windows)]
fn bound_directory_path(directory: &Dir) -> Result<PathBuf, PackageError> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let file = directory
        .try_clone()
        .map_err(PackageError::Io)?
        .into_std_file();
    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    let required = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
    if required == 0 {
        return Err(PackageError::OutputAncestryUnverifiable);
    }
    let mut buffer = vec![0_u16; required as usize];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(PackageError::OutputAncestryUnverifiable);
    }
    buffer.truncate(written as usize);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

fn directory_info(directory: &Dir) -> Result<HandleInfo, PackageError> {
    let clone = directory.try_clone().map_err(PackageError::Io)?;
    let file = File::from_std(clone.into_std_file());
    let info = handle_info(&file)?;
    ensure_directory(info)?;
    Ok(info)
}

fn sync_directory(directory: &Dir) -> Result<(), PackageError> {
    directory
        .try_clone()
        .map_err(PackageError::Io)?
        .into_std_file()
        .sync_all()
        .map_err(PackageError::Io)
}

fn bound_canonical_path(path: &Path, expected: FileId) -> Result<PathBuf, PackageError> {
    let canonical = std::fs::canonicalize(path).map_err(PackageError::Io)?;
    let check = open_ambient(&canonical, false)?;
    let info = handle_info(&check)?;
    ensure_regular_file(info)?;
    if info.id != expected {
        return Err(PackageError::ProvenancePathChanged);
    }
    Ok(canonical)
}

#[cfg(unix)]
fn open_parent(directory: &Dir) -> Result<Dir, PackageError> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    // SAFETY: the directory fd is live for the call, the path is a static NUL-terminated
    // ".." component, and a successful owned fd is transferred exactly once to File.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), c"..".as_ptr(), flags) };
    if fd < 0 {
        return Err(PackageError::Io(io::Error::last_os_error()));
    }
    // SAFETY: openat returned a new owned file descriptor on success.
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    Ok(Dir::from_std_file(file))
}

#[cfg(windows)]
fn open_parent(directory: &Dir) -> Result<Dir, PackageError> {
    directory
        .open_parent_dir(ambient_authority())
        .map_err(PackageError::Io)
}

#[cfg(unix)]
fn handle_info(file: &File) -> Result<HandleInfo, PackageError> {
    let metadata = file.metadata().map_err(PackageError::Io)?;
    Ok(HandleInfo {
        id: FileId {
            device: metadata.dev(),
            inode: {
                let mut identifier = [0_u8; 16];
                identifier[..8].copy_from_slice(&metadata.ino().to_le_bytes());
                identifier
            },
        },
        len: metadata.len(),
        links: metadata.nlink(),
        is_file: metadata.is_file(),
        is_dir: metadata.is_dir(),
        is_reparse: metadata.is_symlink(),
    })
}

#[cfg(windows)]
fn handle_info(file: &File) -> Result<HandleInfo, PackageError> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ID_INFO, FILE_TYPE_DISK, FileIdInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetFileType,
    };

    let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(PackageError::SpecialFileNotAllowed);
    }
    let mut raw = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    if unsafe { GetFileInformationByHandle(handle, raw.as_mut_ptr()) } == 0 {
        return Err(PackageError::Io(io::Error::last_os_error()));
    }
    let raw = unsafe { raw.assume_init() };
    let mut file_id = MaybeUninit::<FILE_ID_INFO>::uninit();
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            file_id.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(PackageError::Io(io::Error::last_os_error()));
    }
    let file_id = unsafe { file_id.assume_init() };
    let is_dir = raw.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    Ok(HandleInfo {
        id: FileId {
            device: file_id.VolumeSerialNumber,
            inode: file_id.FileId.Identifier,
        },
        len: (u64::from(raw.nFileSizeHigh) << 32) | u64::from(raw.nFileSizeLow),
        links: u64::from(raw.nNumberOfLinks),
        is_file: !is_dir,
        is_dir,
        is_reparse: raw.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn atomic_output_uses_private_mode_and_syncs() {
        use std::os::unix::fs::PermissionsExt;

        let source_path = tempfile::tempdir().unwrap();
        let output_path = tempfile::tempdir().unwrap();
        let source = SourceTree::open(source_path.path()).unwrap();
        let output = output_path.path().join("plugin.info-panel-plugin");
        source
            .prepare_output(&output)
            .unwrap()
            .write_atomic(b"package")
            .unwrap();
        assert_eq!(
            std::fs::metadata(output).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_hardlinked_and_symlinked_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let hardlink = directory.path().join("hardlink");
        let symlink = directory.path().join("symlink");
        std::fs::write(&original, b"input").unwrap();
        std::fs::hard_link(&original, &hardlink).unwrap();
        std::os::unix::fs::symlink(&original, &symlink).unwrap();
        assert!(matches!(
            read_bound_file(&hardlink, 64),
            Err(PackageError::LinksNotAllowed)
        ));
        assert!(matches!(
            read_bound_file(&symlink, 64),
            Err(PackageError::LinksNotAllowed)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir().unwrap();
        let fifo = directory.path().join("input.fifo");
        let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: path is a valid NUL-terminated filesystem path and mode is bounded.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(read_bound_file(&fifo, 64).is_err());
    }

    #[test]
    fn atomic_output_never_clobbers_target_created_after_prepare() {
        let source_path = tempfile::tempdir().unwrap();
        let output_path = tempfile::tempdir().unwrap();
        let source = SourceTree::open(source_path.path()).unwrap();
        let output = output_path.path().join("plugin.info-panel-plugin");
        let prepared = source.prepare_output(&output).unwrap();
        std::fs::write(&output, b"racing writer").unwrap();
        assert!(matches!(
            prepared.write_atomic(b"new package"),
            Err(PackageError::OutputExists)
        ));
        assert_eq!(std::fs::read(output).unwrap(), b"racing writer");
    }

    #[test]
    fn incomplete_ancestor_walk_is_not_proof_of_safety() {
        let source = FileId {
            device: 1,
            inode: [1; 16],
        };
        let known_parent = FileId {
            device: 1,
            inode: [2; 16],
        };
        let output = FileId {
            device: 1,
            inode: [3; 16],
        };
        let decision = classify_ancestry(source, &[source, known_parent], &[output], false);
        assert_eq!(decision, AncestryDecision::Continue);
        assert!(matches!(
            finish_ancestry_proof(decision),
            Err(PackageError::OutputAncestryUnverifiable)
        ));
    }
}
