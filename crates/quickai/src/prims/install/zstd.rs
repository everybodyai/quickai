//! Reading a `.tar.zst`: Zstandard frames one after another (decision C28 of the plan).
//!
//! `ruzstd` decodes one frame at a time and leaves three things to its caller, which this reader
//! does so that the installer can say what it means: it reads *every* frame of a file (a
//! skippable frame, which carries metadata, is skipped; frames written one after another are read
//! through), it compares the checksum a frame carries with the one it computed (`ruzstd` reads the
//! four bytes and does not compare them), and it tells an archive that stops short from one that
//! is damaged. Anything after the last frame that is not a frame is an error: an archive is a
//! whole file, and bytes that are not part of it are not to be installed around.

use std::error::Error;
use std::io::{self, BufRead, BufReader, Read};

use ruzstd::decoding::errors::{FrameDecoderError, ReadFrameHeaderError};
use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};

/// What a person is told when the data ends before the archive does.
pub const INCOMPLETE: &str =
    "the archive is incomplete: the file ends before its data does (the download was cut short)";

pub struct ZstdReader<R: Read> {
    source: BufReader<R>,
    frame: FrameDecoder,
    /// A frame has been started and is not yet drained.
    in_frame: bool,
    /// Frames that held data and were read to their end.
    frames: u32,
    finished: bool,
}

impl<R: Read> ZstdReader<R> {
    pub fn new(source: R) -> Self {
        ZstdReader {
            source: BufReader::with_capacity(256 * 1024, source),
            frame: FrameDecoder::new(),
            in_frame: false,
            frames: 0,
            finished: false,
        }
    }

    /// Starts the next frame, skipping skippable ones. `Ok(false)` at the clean end of the file.
    fn next_frame(&mut self) -> io::Result<bool> {
        loop {
            if self.source.fill_buf()?.is_empty() {
                return if self.frames == 0 {
                    Err(incomplete())
                } else {
                    Ok(false)
                };
            }
            match self.frame.init(&mut self.source) {
                Ok(()) => return Ok(true),
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame {
                    length,
                    ..
                })) => {
                    let mut left = u64::from(length);
                    while left > 0 {
                        let buffered = self.source.fill_buf()?.len() as u64;
                        if buffered == 0 {
                            return Err(incomplete());
                        }
                        let step = buffered.min(left);
                        self.source.consume(step as usize);
                        left -= step;
                    }
                }
                Err(FrameDecoderError::ReadFrameHeaderError(
                    ReadFrameHeaderError::BadMagicNumber(_),
                )) if self.frames > 0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the archive is damaged: what follows its last frame is not zstd data",
                    ));
                }
                Err(e) => return Err(describe(&e)),
            }
        }
    }

    fn check_checksum(&self) -> io::Result<()> {
        if let Some(stored) = self.frame.get_checksum_from_data() {
            if self.frame.get_calculated_checksum() != Some(stored) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the archive is damaged: its checksum does not match its data",
                ));
            }
        }
        Ok(())
    }
}

impl<R: Read> Read for ZstdReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.finished {
                return Ok(0);
            }
            if !self.in_frame {
                if !self.next_frame()? {
                    self.finished = true;
                    return Ok(0);
                }
                self.in_frame = true;
            }
            // decode far enough ahead that a read can be answered, as `StreamingDecoder` does
            while self.frame.can_collect() < out.len() && !self.frame.is_finished() {
                let wanted = out.len() - self.frame.can_collect();
                self.frame
                    .decode_blocks(&mut self.source, BlockDecodingStrategy::UptoBytes(wanted))
                    .map_err(|e| describe(&e))?;
            }
            let n = self.frame.read(out)?;
            if n > 0 {
                return Ok(n);
            }
            if self.frame.is_finished() && self.frame.can_collect() == 0 {
                self.check_checksum()?;
                self.in_frame = false;
                self.frames += 1;
            }
        }
    }
}

fn incomplete() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, INCOMPLETE)
}

/// A decoder error as a person should read it: the file ends too soon, or the data is damaged.
fn describe(error: &FrameDecoderError) -> io::Error {
    if ends_too_soon(error) {
        incomplete()
    } else {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the archive is damaged: {error}"),
        )
    }
}

/// Whether `error` comes from running out of data: an `UnexpectedEof` anywhere in its chain of
/// causes, or the sentence the standard library gives it.
fn ends_too_soon(error: &(dyn Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(io) = error.downcast_ref::<io::Error>() {
            if io.kind() == io::ErrorKind::UnexpectedEof {
                return true;
            }
        }
        cause = error.source();
    }
    let text = error.to_string();
    text.contains("failed to fill whole buffer") || text.contains("unexpected end of file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::archives::{fixture, zst_frame, zst_skippable};

    fn decode(bytes: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        ZstdReader::new(bytes).read_to_end(&mut out)?;
        Ok(out)
    }

    fn message(result: io::Result<Vec<u8>>) -> String {
        result.expect_err("an error was expected").to_string()
    }

    #[test]
    fn one_frame_reads_back_what_was_put_in() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        assert_eq!(decode(&zst_frame(&data)).unwrap(), data);
        assert_eq!(decode(&zst_frame(b"")).unwrap(), b"");
    }

    #[test]
    fn frames_one_after_another_are_read_through_and_skippable_ones_are_skipped() {
        let mut file = zst_skippable(b"metadata the reader must skip");
        file.extend(zst_frame(b"first, "));
        file.extend(zst_skippable(&[0u8; 1000]));
        file.extend(zst_frame(b"second"));
        file.extend(zst_skippable(b""));
        assert_eq!(decode(&file).unwrap(), b"first, second");
    }

    #[test]
    fn a_file_with_nothing_in_it_is_incomplete_not_empty_success() {
        assert_eq!(message(decode(b"")), INCOMPLETE);
        // only skippable frames hold no data either
        assert_eq!(message(decode(&zst_skippable(b"x"))), INCOMPLETE);
    }

    #[test]
    fn a_file_cut_anywhere_says_it_is_incomplete() {
        let data = vec![7u8; 200_000];
        let whole = zst_frame(&data);
        for cut in [1, 3, 4, 5, 6, 7, 8, 100, whole.len() / 2, whole.len() - 1] {
            assert_eq!(
                message(decode(&whole[..cut])),
                INCOMPLETE,
                "cut at {cut} of {}",
                whole.len()
            );
        }
        // a skippable frame that stops short too
        let mut skipping = zst_skippable(&[0u8; 100]);
        skipping.truncate(50);
        assert_eq!(message(decode(&skipping)), INCOMPLETE);
    }

    #[test]
    fn bytes_after_the_last_frame_are_refused() {
        let mut file = zst_frame(b"data");
        file.extend(b"this is not a zstd frame");
        let error = message(decode(&file));
        assert!(error.contains("not zstd data"), "{error}");
    }

    #[test]
    fn what_a_real_zstd_wrote_is_read_in_every_framing() {
        let plain = decode(&fixture("mini-ollama-linux.tar.zst")).unwrap();
        // a tar archive: the first entry's name is at the start of the first header
        assert!(plain.starts_with(b"bin/ollama"), "{:?}", &plain[..32]);
        assert_eq!(
            decode(&fixture("mini-ollama-linux-nocheck.tar.zst")).unwrap(),
            plain,
            "the same data without a checksum"
        );
        assert_eq!(
            decode(&fixture("mini-ollama-linux-multiframe.tar.zst")).unwrap(),
            plain,
            "the same data in two frames"
        );
    }

    #[test]
    fn the_checksum_a_frame_carries_is_checked() {
        let good = fixture("mini-ollama-linux.tar.zst");
        // the last four bytes are the XXH64 checksum: the data is intact, the checksum is not
        let mut wrong_checksum = good.clone();
        *wrong_checksum.last_mut().unwrap() ^= 0xFF;
        assert!(message(decode(&wrong_checksum)).contains("checksum"));
        // a flipped byte in the data is found by the decoder or by the checksum, and never passes
        for at in [good.len() / 3, good.len() / 2, good.len() - 6] {
            let mut bad = good.clone();
            bad[at] ^= 0x55;
            assert!(decode(&bad).is_err(), "byte {at} flipped and still read");
        }
    }

    #[test]
    fn small_reads_give_the_same_bytes_as_large_ones() {
        let data: Vec<u8> = (0..70_000u32).map(|i| (i * 7 % 256) as u8).collect();
        let mut file = zst_frame(&data[..30_000]);
        file.extend(zst_frame(&data[30_000..]));
        let mut reader = ZstdReader::new(&file[..]);
        let mut got = Vec::new();
        let mut chunk = [0u8; 13];
        loop {
            let n = reader.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..n]);
        }
        assert_eq!(got, data);
    }
}
