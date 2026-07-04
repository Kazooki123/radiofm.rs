//! `rodio::Decoder<R>` requires `R: Read + Seek` on the whole type, even
//! for formats like MP3 that don't structurally need to seek backward --
//! the bound exists because `Decoder` is one type shared across all
//! container formats, some of which (WAV, FLAC) do need it. A live HTTP
//! stream can't seek at all, so we can't hand it a `reqwest::blocking::Response`
//! directly.
//!
//! `CachingReader` bridges the gap: every byte read from the underlying
//! stream is kept in memory, so seeking *backward* into already-read data
//! is just a pointer move, and seeking *forward* transparently pulls (and
//! caches) more bytes from the network. In practice, format probing only
//! touches the first few KB, so this stays cheap.
//!
//! **TRADE-OFF** worth knowing: because every read is retained, memory
//! grows for the lifetime of playback (roughly 1MB per ~64s at 128kbps).
//! It's usually fine for a normal listening session, but if you want an unattended multi-hour
//! playback, the next improvement i'll be adding here is evicting bytes that the decoder has
//! moved well past, instead of keeping the whole history.

use std::io::{self, Read, Seek, SeekFrom};

pub struct CachingReader<R: Read> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl<R: Read> CachingReader<R> {
    pub fn new(inner: R) -> Self {
        CachingReader {
            inner,
            buf: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    /// Pulls from the underlying stream until we've cached at least
    /// `target` bytes, or the stream ends.
    fn fill_to(&mut self, target: usize) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        while self.buf.len() < target && !self.eof {
            let n = self.inner.read(&mut chunk)?;
            if n == 0 {
                self.eof = true;
                break;
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
        Ok(())
    }
}

impl<R: Read> Read for CachingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.fill_to(self.pos + out.len())?;
        let available = &self.buf[self.pos..];
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.pos += n;
        Ok(n)
    }
}

impl<R: Read> Seek for CachingReader<R> {
    fn seek(&mut self, seek_from: SeekFrom) -> io::Result<u64> {
        let target: i64 = match seek_from {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "cannot seek from the end of a live, unbounded stream",
                ));
            }
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start of the stream",
            ));
        }
        let target = target as usize;
        self.fill_to(target)?;
        self.pos = target.min(self.buf.len());
        Ok(self.pos as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_sequentially() {
        let mut r = CachingReader::new(Cursor::new(b"hello world".to_vec()));
        let mut out = [0u8; 5];
        r.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"hello");
    }

    #[test]
    fn seek_backward_then_forward() {
        let mut r = CachingReader::new(Cursor::new(b"0123456789".to_vec()));
        let mut out = [0u8; 4];
        r.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"0123");

        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"0123");

        r.seek(SeekFrom::Start(6)).unwrap();
        r.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"6789");
    }

    #[test]
    fn seek_end_is_rejected() {
        let mut r = CachingReader::new(Cursor::new(b"abc".to_vec()));
        assert!(r.seek(SeekFrom::End(0)).is_err());
    }
}
