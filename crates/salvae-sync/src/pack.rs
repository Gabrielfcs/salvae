//! Deterministic folder <-> blob packing.
//!
//! A save folder is serialized into one byte blob with files in sorted path
//! order and no timestamps, so identical folder *content* always produces
//! identical bytes (the vault's dedupe/conflict logic relies on this).
//!
//! Format: `b"SVPK1\n"` || u32 file count || for each file:
//! u32 path-len || path (utf8, `/`-separated) || u64 data-len || data.

use std::path::{Path, PathBuf};

use crate::SyncError;

const MAGIC: &[u8] = b"SVPK1\n";

/// Pack all files under `root` (recursively) into one deterministic blob.
pub fn pack_folder(root: &Path) -> Result<Vec<u8>, SyncError> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (rel, abs) in &files {
        let data = std::fs::read(abs).map_err(|e| SyncError::Io(e.to_string()))?;
        let rel_bytes = rel.as_bytes();
        out.extend_from_slice(&(rel_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(rel_bytes);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&data);
    }
    Ok(out)
}

fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), SyncError> {
    for entry in std::fs::read_dir(dir).map_err(|e| SyncError::Io(e.to_string()))? {
        let entry = entry.map_err(|e| SyncError::Io(e.to_string()))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if ft.is_dir() {
            collect_files(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| SyncError::Pack(e.to_string()))?;
            out.push((rel.to_string_lossy().replace('\\', "/"), path));
        }
    }
    Ok(())
}

/// Unpack a blob produced by [`pack_folder`] into `dest`, recreating files.
/// Rejects entries with absolute paths or `..` components (path traversal).
pub fn unpack_folder(data: &[u8], dest: &Path) -> Result<(), SyncError> {
    if data.len() < MAGIC.len() || &data[..MAGIC.len()] != MAGIC {
        return Err(SyncError::Pack("bad magic header".into()));
    }
    let mut cur = MAGIC.len();
    let count = read_u32(data, &mut cur)?;
    for _ in 0..count {
        let path_len = read_u32(data, &mut cur)? as usize;
        let rel = read_slice(data, &mut cur, path_len)?;
        let rel_str = std::str::from_utf8(rel).map_err(|e| SyncError::Pack(e.to_string()))?;
        let data_len = read_u64(data, &mut cur)? as usize;
        let file_data = read_slice(data, &mut cur, data_len)?;

        let rel_path = Path::new(rel_str);
        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(SyncError::Pack(format!(
                "unsafe path in archive: {rel_str:?}"
            )));
        }
        let target = dest.join(rel_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SyncError::Io(e.to_string()))?;
        }
        std::fs::write(&target, file_data).map_err(|e| SyncError::Io(e.to_string()))?;
    }
    Ok(())
}

fn read_u32(data: &[u8], cur: &mut usize) -> Result<u32, SyncError> {
    let bytes = read_slice(data, cur, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u64(data: &[u8], cur: &mut usize) -> Result<u64, SyncError> {
    let bytes = read_slice(data, cur, 8)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_slice<'a>(data: &'a [u8], cur: &mut usize, len: usize) -> Result<&'a [u8], SyncError> {
    let end = cur
        .checked_add(len)
        .ok_or_else(|| SyncError::Pack("length overflow".into()))?;
    if end > data.len() {
        return Err(SyncError::Pack("unexpected end of archive".into()));
    }
    let slice = &data[*cur..end];
    *cur = end;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn pack_then_unpack_reproduces_files() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "world.db", b"main save");
        write(src.path(), "meta/info.txt", b"v=3");

        let blob = pack_folder(src.path()).unwrap();

        let dst = tempfile::tempdir().unwrap();
        unpack_folder(&blob, dst.path()).unwrap();
        assert_eq!(
            std::fs::read(dst.path().join("world.db")).unwrap(),
            b"main save"
        );
        assert_eq!(
            std::fs::read(dst.path().join("meta/info.txt")).unwrap(),
            b"v=3"
        );
    }

    #[test]
    fn packing_is_deterministic() {
        let src = tempfile::tempdir().unwrap();
        write(src.path(), "b.txt", b"bbb");
        write(src.path(), "a.txt", b"aaa");
        write(src.path(), "sub/c.txt", b"ccc");
        let first = pack_folder(src.path()).unwrap();
        let second = pack_folder(src.path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let dst = tempfile::tempdir().unwrap();
        assert!(matches!(
            unpack_folder(b"NOPE", dst.path()),
            Err(SyncError::Pack(_))
        ));
    }

    #[test]
    fn path_traversal_is_rejected() {
        // Hand-craft a blob with one entry whose path escapes the destination.
        let mut blob = Vec::new();
        blob.extend_from_slice(MAGIC);
        blob.extend_from_slice(&1u32.to_le_bytes());
        let evil = b"../escape.txt";
        blob.extend_from_slice(&(evil.len() as u32).to_le_bytes());
        blob.extend_from_slice(evil);
        blob.extend_from_slice(&3u64.to_le_bytes());
        blob.extend_from_slice(b"bad");

        let dst = tempfile::tempdir().unwrap();
        assert!(matches!(
            unpack_folder(&blob, dst.path()),
            Err(SyncError::Pack(_))
        ));
        assert!(!dst.path().parent().unwrap().join("escape.txt").exists());
    }
}
