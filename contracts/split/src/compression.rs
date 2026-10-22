//! Payload compression (issue #1021).
//!
//! Run-length encoding for byte payloads with explicit negotiation, raw
//! fallback and metrics. Heap free and `no_std` friendly: callers pass the
//! output buffer.
//!
//! Wire format: `[encoding byte][body...]`
//! - `0x00` raw: body is the original bytes.
//! - `0x01` RLE: body is `(run_length, value)` pairs, run length 1..=255.
//!
//! Negotiation: the caller advertises supported encodings as a bitmask
//! ([`ACCEPT_RAW`], [`ACCEPT_RLE`]). RLE is used only when it is accepted and
//! strictly smaller than raw; otherwise [`compress`] falls back to raw.

pub const ENCODING_RAW: u8 = 0;
pub const ENCODING_RLE: u8 = 1;

pub const ACCEPT_RAW: u8 = 0b01;
pub const ACCEPT_RLE: u8 = 0b10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionError {
    /// The output buffer is too small.
    BufferTooSmall,
    /// The payload is empty, truncated or uses an unknown encoding.
    InvalidPayload,
    /// The accepted-encodings mask has no supported encoding.
    NoAcceptableEncoding,
}

/// Pick the best encoding allowed by `accepted` for `input`.
pub fn negotiate(accepted: u8, input: &[u8]) -> Result<u8, CompressionError> {
    if accepted & ACCEPT_RLE != 0 && rle_size(input) < input.len() {
        Ok(ENCODING_RLE)
    } else if accepted & ACCEPT_RAW != 0 {
        Ok(ENCODING_RAW)
    } else if accepted & ACCEPT_RLE != 0 {
        // Only RLE is acceptable even though it is not smaller.
        Ok(ENCODING_RLE)
    } else {
        Err(CompressionError::NoAcceptableEncoding)
    }
}

fn rle_size(input: &[u8]) -> usize {
    let mut size = 0;
    let mut i = 0;
    while i < input.len() {
        let mut run = 1;
        while i + run < input.len() && input[i + run] == input[i] && run < 255 {
            run += 1;
        }
        size += 2;
        i += run;
    }
    size
}

/// Compress `input` into `out`, returning the number of bytes written.
pub fn compress(accepted: u8, input: &[u8], out: &mut [u8]) -> Result<usize, CompressionError> {
    let encoding = negotiate(accepted, input)?;
    if out.is_empty() {
        return Err(CompressionError::BufferTooSmall);
    }
    out[0] = encoding;

    if encoding == ENCODING_RAW {
        if out.len() < 1 + input.len() {
            return Err(CompressionError::BufferTooSmall);
        }
        out[1..1 + input.len()].copy_from_slice(input);
        return Ok(1 + input.len());
    }

    if out.len() < 1 + rle_size(input) {
        return Err(CompressionError::BufferTooSmall);
    }
    let mut w = 1;
    let mut i = 0;
    while i < input.len() {
        let mut run = 1;
        while i + run < input.len() && input[i + run] == input[i] && run < 255 {
            run += 1;
        }
        out[w] = run as u8;
        out[w + 1] = input[i];
        w += 2;
        i += run;
    }
    Ok(w)
}

/// Decompress `input` into `out`, returning the number of bytes written.
pub fn decompress(input: &[u8], out: &mut [u8]) -> Result<usize, CompressionError> {
    let (&encoding, body) = input.split_first().ok_or(CompressionError::InvalidPayload)?;
    match encoding {
        ENCODING_RAW => {
            if out.len() < body.len() {
                return Err(CompressionError::BufferTooSmall);
            }
            out[..body.len()].copy_from_slice(body);
            Ok(body.len())
        }
        ENCODING_RLE => {
            if body.len() % 2 != 0 {
                return Err(CompressionError::InvalidPayload);
            }
            let mut w = 0;
            for pair in body.chunks(2) {
                let run = pair[0] as usize;
                if run == 0 {
                    return Err(CompressionError::InvalidPayload);
                }
                if out.len() < w + run {
                    return Err(CompressionError::BufferTooSmall);
                }
                for b in &mut out[w..w + run] {
                    *b = pair[1];
                }
                w += run;
            }
            Ok(w)
        }
        _ => Err(CompressionError::InvalidPayload),
    }
}

/// Cumulative compression counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompressionMetrics {
    pub payloads: u32,
    pub rle_payloads: u32,
    pub raw_fallbacks: u32,
    pub input_bytes: u64,
    pub output_bytes: u64,
}

impl CompressionMetrics {
    /// Compress and record the result.
    pub fn compress(
        &mut self,
        accepted: u8,
        input: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CompressionError> {
        let written = compress(accepted, input, out)?;
        self.payloads += 1;
        if out[0] == ENCODING_RLE {
            self.rle_payloads += 1;
        } else {
            self.raw_fallbacks += 1;
        }
        self.input_bytes += input.len() as u64;
        self.output_bytes += written as u64;
        Ok(written)
    }

    /// Size reduction in basis points (10_000 = 100%). `0` when nothing was saved.
    pub fn savings_bps(&self) -> u32 {
        if self.input_bytes == 0 || self.output_bytes >= self.input_bytes {
            return 0;
        }
        ((self.input_bytes - self.output_bytes) * 10_000 / self.input_bytes) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(accepted: u8, input: &[u8]) -> (usize, u8) {
        let mut packed = [0u8; 600];
        let n = compress(accepted, input, &mut packed).unwrap();
        let mut restored = [0u8; 600];
        let m = decompress(&packed[..n], &mut restored).unwrap();
        assert_eq!(&restored[..m], input);
        (n, packed[0])
    }

    #[test]
    fn repetitive_data_shrinks_by_more_than_70_percent() {
        let input = [7u8; 300];
        let (n, enc) = roundtrip(ACCEPT_RAW | ACCEPT_RLE, &input);
        assert_eq!(enc, ENCODING_RLE);
        assert!(n * 100 <= input.len() * 30, "compressed {n} of {}", input.len());
    }

    #[test]
    fn incompressible_data_falls_back_to_raw() {
        let mut input = [0u8; 64];
        for (i, b) in input.iter_mut().enumerate() {
            *b = i as u8;
        }
        let (n, enc) = roundtrip(ACCEPT_RAW | ACCEPT_RLE, &input);
        assert_eq!(enc, ENCODING_RAW);
        assert_eq!(n, input.len() + 1);
    }

    #[test]
    fn negotiation_respects_accepted_encodings() {
        let repetitive = [1u8; 50];
        assert_eq!(negotiate(ACCEPT_RAW, &repetitive), Ok(ENCODING_RAW));
        assert_eq!(negotiate(ACCEPT_RLE, &repetitive), Ok(ENCODING_RLE));
        assert_eq!(negotiate(ACCEPT_RAW | ACCEPT_RLE, &repetitive), Ok(ENCODING_RLE));
        assert_eq!(negotiate(0, &repetitive), Err(CompressionError::NoAcceptableEncoding));
        // RLE-only peers still get RLE even when it is larger.
        assert_eq!(negotiate(ACCEPT_RLE, &[1, 2, 3]), Ok(ENCODING_RLE));
    }

    #[test]
    fn runs_longer_than_255_are_split() {
        roundtrip(ACCEPT_RLE, &[9u8; 520]);
    }

    #[test]
    fn empty_input_roundtrips() {
        let (n, _) = roundtrip(ACCEPT_RAW | ACCEPT_RLE, &[]);
        assert_eq!(n, 1);
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        let mut out = [0u8; 16];
        assert_eq!(decompress(&[], &mut out), Err(CompressionError::InvalidPayload));
        assert_eq!(decompress(&[9, 1], &mut out), Err(CompressionError::InvalidPayload));
        assert_eq!(decompress(&[ENCODING_RLE, 3], &mut out), Err(CompressionError::InvalidPayload));
        assert_eq!(decompress(&[ENCODING_RLE, 0, 5], &mut out), Err(CompressionError::InvalidPayload));
        assert_eq!(decompress(&[ENCODING_RLE, 200, 5], &mut out), Err(CompressionError::BufferTooSmall));
    }

    #[test]
    fn small_output_buffer_is_reported() {
        let mut tiny = [0u8; 1];
        assert_eq!(compress(ACCEPT_RAW, &[1, 2, 3], &mut tiny), Err(CompressionError::BufferTooSmall));
    }

    #[test]
    fn metrics_track_savings_and_fallbacks() {
        let mut m = CompressionMetrics::default();
        let mut out = [0u8; 600];
        m.compress(ACCEPT_RAW | ACCEPT_RLE, &[0u8; 200], &mut out).unwrap();
        m.compress(ACCEPT_RAW | ACCEPT_RLE, &[1, 2, 3, 4], &mut out).unwrap();
        assert_eq!(m.payloads, 2);
        assert_eq!(m.rle_payloads, 1);
        assert_eq!(m.raw_fallbacks, 1);
        assert_eq!(m.input_bytes, 204);
        assert_eq!(m.output_bytes, 3 + 5);
        assert!(m.savings_bps() > 9_000);
        assert_eq!(CompressionMetrics::default().savings_bps(), 0);
    }
}
