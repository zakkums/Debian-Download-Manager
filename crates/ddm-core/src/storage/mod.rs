//! Disk I/O and file lifecycle.
//!
//! Preallocates temp files (fallocate on Linux when available, else set_len),
//! supports concurrent offset writes (pwrite), fsync policy, and atomic
//! finalize (rename from `.part` to final name).

mod builder;
mod writer;

pub use builder::StorageWriterBuilder;
pub use writer::StorageWriter;

/// Temporary file suffix used before atomic rename.
pub const TEMP_SUFFIX: &str = ".part";

/// Path for the temp file: appends `.part` to the final path (e.g. `file.iso` → `file.iso.part`).
pub fn temp_path(final_path: &std::path::Path) -> std::path::PathBuf {
    let mut o = final_path.as_os_str().to_owned();
    o.push(".part");
    std::path::PathBuf::from(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::Path;

    #[test]
    fn temp_path_appends_part() {
        let p = temp_path(Path::new("file.iso"));
        assert_eq!(p.to_string_lossy(), "file.iso.part");
        let p2 = temp_path(Path::new("/tmp/archive.zip"));
        assert_eq!(p2.to_string_lossy(), "/tmp/archive.zip.part");
    }

    #[test]
    fn create_preallocate_write_finalize() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("output.bin");
        let tp = temp_path(&final_path);

        let mut builder = StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(100).unwrap();
        let writer = builder.build();

        writer.write_at(0, b"hello").unwrap();
        writer.write_at(50, b"world").unwrap();
        writer.write_at(95, b"xy").unwrap();
        writer.sync().unwrap();
        writer.finalize(&final_path).unwrap();

        assert!(!tp.exists());
        assert!(final_path.exists());
        let mut f = std::fs::File::open(&final_path).unwrap();
        let mut buf = vec![0u8; 100];
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[0..5], b"hello");
        assert_eq!(&buf[50..55], b"world");
        assert_eq!(&buf[95..97], b"xy");
    }

    #[test]
    fn write_at_concurrent_style() {
        let dir = tempfile::tempdir().unwrap();
        let tp = dir.path().join("out.part");
        let mut builder = StorageWriterBuilder::create(&tp).unwrap();
        builder.preallocate(20).unwrap();
        let writer = builder.build();
        let w2 = writer.clone();
        writer.write_at(0, b"aaaa").unwrap();
        w2.write_at(10, b"bbbb").unwrap();
        writer.write_at(4, b"cccc").unwrap();
        writer.sync().unwrap();
        let final_p = dir.path().join("out.bin");
        writer.finalize(&final_p).unwrap();
        let mut f = std::fs::File::open(&final_p).unwrap();
        let mut buf = vec![0u8; 20];
        f.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[0..4], b"aaaa");
        assert_eq!(&buf[4..8], b"cccc");
        assert_eq!(&buf[10..14], b"bbbb");
    }
}
