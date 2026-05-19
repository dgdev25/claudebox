use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// File magic for ClaudeBox appliance files.
pub const CLBX_MAGIC: &[u8; 4] = b"CLBX";
/// Format version stored in the header.
pub const CLBX_VERSION: u8 = 1;

/// Segment tag constants — 8-byte ASCII, null-padded.
pub mod tag {
    pub const MANIFEST: &[u8; 8] = b"MANIFEST";
    pub const KERNEL: &[u8; 8] = b"KERNEL\x00\x00";
    pub const EBPF: &[u8; 8] = b"EBPF\x00\x00\x00\x00";
    pub const WITNESS: &[u8; 8] = b"WITNESS\x00";
    pub const META: &[u8; 8] = b"META\x00\x00\x00\x00";
    pub const CRYPTO: &[u8; 8] = b"CRYPTO\x00\x00";
}

/// Writes a ClaudeBox `.rvf` appliance file segment-by-segment.
///
/// Call [`write_segment`] for each segment in order, then
/// [`finalize`] to patch the segment count into the header and flush.
pub struct RvfWriter<W: Write + Seek> {
    inner: W,
    seg_count: u32,
}

impl<W: Write + Seek> RvfWriter<W> {
    /// Write the file header and return an `RvfWriter` ready for segments.
    pub fn new(mut inner: W) -> std::io::Result<Self> {
        // magic(4) + version(1) + flags(1) + reserved(2) + seg_count(4) = 12 bytes
        inner.write_all(CLBX_MAGIC)?;
        inner.write_all(&[CLBX_VERSION, 0x00, 0x00, 0x00])?;
        inner.write_all(&0u32.to_le_bytes())?; // placeholder seg_count
        Ok(Self { inner, seg_count: 0 })
    }

    /// Append one segment. `tag` must be exactly 8 bytes.
    pub fn write_segment(&mut self, tag: &[u8; 8], payload: &[u8]) -> std::io::Result<()> {
        self.inner.write_all(tag)?;
        self.inner.write_all(&0u32.to_le_bytes())?; // flags
        self.inner.write_all(&(payload.len() as u64).to_le_bytes())?;
        self.inner.write_all(payload)?;
        self.seg_count += 1;
        Ok(())
    }

    /// Patch the segment count into the header and flush.
    pub fn finalize(mut self) -> std::io::Result<()> {
        // seg_count is at offset 8 (after magic+version+flags+reserved)
        self.inner.seek(SeekFrom::Start(8))?;
        self.inner.write_all(&self.seg_count.to_le_bytes())?;
        self.inner.flush()
    }
}

/// One parsed segment from a `.rvf` file.
pub struct Segment {
    /// 8-byte tag.
    pub tag: [u8; 8],
    /// Segment payload bytes.
    pub payload: Vec<u8>,
}

/// Parsed ClaudeBox appliance header + segments.
pub struct RvfFile {
    pub segments: Vec<Segment>,
}

impl RvfFile {
    /// Read a `.rvf` file from disk.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut f = std::fs::File::open(path)
            .map_err(|e| anyhow::anyhow!("cannot open {}: {e}", path.display()))?;
        Self::read_from(&mut f)
    }

    fn read_from<R: Read>(r: &mut R) -> anyhow::Result<Self> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)
            .map_err(|e| anyhow::anyhow!("failed to read magic: {e}"))?;
        anyhow::ensure!(
            &magic == CLBX_MAGIC,
            "invalid magic {:?}; expected CLBX",
            magic
        );

        let mut hdr = [0u8; 8]; // version(1)+flags(1)+reserved(2)+seg_count(4)
        r.read_exact(&mut hdr)?;
        let version = hdr[0];
        anyhow::ensure!(
            version == CLBX_VERSION,
            "unsupported RVF version {version}; expected {CLBX_VERSION}"
        );
        let seg_count = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);

        let mut segments = Vec::with_capacity(seg_count as usize);
        for _ in 0..seg_count {
            let mut tag = [0u8; 8];
            r.read_exact(&mut tag)?;
            let mut flags_and_len = [0u8; 12]; // flags(4) + length(8)
            r.read_exact(&mut flags_and_len)?;
            let length = u64::from_le_bytes([
                flags_and_len[4],
                flags_and_len[5],
                flags_and_len[6],
                flags_and_len[7],
                flags_and_len[8],
                flags_and_len[9],
                flags_and_len[10],
                flags_and_len[11],
            ]);
            let mut payload = vec![0u8; length as usize];
            r.read_exact(&mut payload)?;
            segments.push(Segment { tag, payload });
        }

        Ok(RvfFile { segments })
    }

    /// Find the first segment with the given tag. Returns `None` if absent.
    pub fn find(&self, tag: &[u8; 8]) -> Option<&[u8]> {
        self.segments
            .iter()
            .find(|s| &s.tag == tag)
            .map(|s| s.payload.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_roundtrip_single_segment() {
        let mut buf = Cursor::new(Vec::new());
        let mut writer = RvfWriter::new(&mut buf).unwrap();
        writer.write_segment(tag::MANIFEST, b"hello manifest").unwrap();
        writer.finalize().unwrap();

        buf.set_position(0);
        let rvf = RvfFile::read_from(&mut buf).unwrap();
        assert_eq!(rvf.segments.len(), 1);
        assert_eq!(rvf.find(tag::MANIFEST).unwrap(), b"hello manifest");
    }

    #[test]
    fn test_roundtrip_multiple_segments() {
        let mut buf = Cursor::new(Vec::new());
        let mut writer = RvfWriter::new(&mut buf).unwrap();
        writer.write_segment(tag::MANIFEST, b"manifest payload").unwrap();
        writer.write_segment(tag::CRYPTO, b"crypto payload").unwrap();
        writer.write_segment(tag::KERNEL, b"kernel bytes").unwrap();
        writer.finalize().unwrap();

        buf.set_position(0);
        let rvf = RvfFile::read_from(&mut buf).unwrap();
        assert_eq!(rvf.segments.len(), 3);
        assert_eq!(rvf.find(tag::MANIFEST).unwrap(), b"manifest payload");
        assert_eq!(rvf.find(tag::CRYPTO).unwrap(), b"crypto payload");
        assert_eq!(rvf.find(tag::KERNEL).unwrap(), b"kernel bytes");
    }

    #[test]
    fn test_find_absent_tag_returns_none() {
        let mut buf = Cursor::new(Vec::new());
        let mut writer = RvfWriter::new(&mut buf).unwrap();
        writer.write_segment(tag::MANIFEST, b"data").unwrap();
        writer.finalize().unwrap();

        buf.set_position(0);
        let rvf = RvfFile::read_from(&mut buf).unwrap();
        assert!(rvf.find(tag::KERNEL).is_none());
    }

    #[test]
    fn test_bad_magic_returns_error() {
        let bad = b"JUNK\x01\x00\x00\x00\x00\x00\x00\x00";
        let mut cur = Cursor::new(&bad[..]);
        assert!(RvfFile::read_from(&mut cur).is_err());
    }
}
