use std::io;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use servo_base::generic_channel;
use servo_media::player::{PlayerEvent, SeekLock, SeekLockMsg};
use symphonia::core::io::MediaSource;

use super::shared::{lock, wait, Shared};

/// The blocking byte source symphonia reads the resource through. Absence is
/// resolved at `read`: block if the live fetch will deliver it, otherwise run the
/// `SeekData` handshake. `io::Seek` only moves the cursor.
pub(super) struct ByteReader {
    pub(super) shared: Arc<Shared>,
    /// `StreamType::Seekable`: refetching at an offset is possible at all.
    pub(super) seekable: bool,
}

impl ByteReader {
    /// Asks the element for a fetch delivering bytes from `offset`. Blocks until
    /// the element acknowledges through the `SeekLock`. `clear` drops the region
    /// (a jump); a resume at the head keeps it and stays contiguous.
    fn request_range(&self, offset: u64, clear: bool) -> io::Result<()> {
        {
            let mut stream = lock(&self.shared.stream);
            if clear {
                stream.base = offset;
                stream.data.clear();
            }
            stream.stalled = false;
            // The new fetch delivers a fresh body with its own end.
            stream.eos = false;
        }

        let (sender, receiver) = generic_channel::channel::<SeekLockMsg>()
            .ok_or_else(|| io::Error::other("seek-lock channel failed"))?;
        self.shared.events.send(PlayerEvent::SeekData(
            offset,
            SeekLock {
                lock_channel: sender,
            },
        ));
        let (ok, ack) = receiver
            .recv()
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        let _ = ack.send(());
        if !ok {
            return Err(io::Error::other("element refused the refetch"));
        }
        // The new fetch context starts locked; NeedData is what unlocks it.
        self.shared.events.send(PlayerEvent::NeedData);
        Ok(())
    }
}

impl io::Read for ByteReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut stream = lock(&self.shared.stream);
        loop {
            // EOF, not an error: the decoder loop tells quit and seek apart from
            // a real end by checking the flags before treating this as EOS.
            if self.shared.quit.load(Ordering::SeqCst) || self.shared.seek_pending() {
                return Ok(0);
            }

            let head = stream.head();
            if stream.read_pos >= stream.base && stream.read_pos < head {
                let start = (stream.read_pos - stream.base) as usize;
                let n = buf.len().min(stream.data.len() - start);
                buf[..n].copy_from_slice(&stream.data[start..start + n]);
                stream.read_pos += n as u64;
                stream.evict();
                return Ok(n);
            }

            if stream.read_pos == head {
                if stream.eos {
                    return Ok(0);
                }
                if !stream.stalled {
                    stream = wait(&self.shared.bytes_cv, stream);
                    continue;
                }
            }

            // A jump outside the region, or a dry head whose fetch we stalled.
            if !self.seekable {
                return Err(io::Error::from(io::ErrorKind::Unsupported));
            }
            let (offset, clear) = if stream.read_pos == head {
                (head, false)
            } else {
                (stream.read_pos, true)
            };
            drop(stream);
            self.request_range(offset, clear)?;
            stream = lock(&self.shared.stream);
        }
    }
}

impl io::Seek for ByteReader {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let mut stream = lock(&self.shared.stream);
        let new = match pos {
            io::SeekFrom::Start(p) => p as i128,
            io::SeekFrom::Current(d) => stream.read_pos as i128 + d as i128,
            io::SeekFrom::End(d) => match stream.total_len {
                Some(len) => len as i128 + d as i128,
                None => return Err(io::Error::from(io::ErrorKind::Unsupported)),
            },
        };
        if new < 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        stream.read_pos = new as u64;
        Ok(stream.read_pos)
    }
}

impl MediaSource for ByteReader {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        lock(&self.shared.stream).total_len
    }
}
