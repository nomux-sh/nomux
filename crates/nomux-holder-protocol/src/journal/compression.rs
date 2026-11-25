//! The compressed payloads of the journal. Each payload is one LZ4 block
//! that needs no other block, thus a reader can start at each record.
//!
//! The `lz4_flex` crate is the LZ4 implementation, with its safe encoder and
//! its safe decoder. `docs/holder.md` has the measurements that selected
//! LZ4: it keeps the holder below its size, memory and throughput targets,
//! and zstd does not.

use std::io;

use lz4_flex::block::{compress_into, decompress_into, get_maximum_output_size};

/// Appends to `output` one LZ4 block that has `raw`.
pub(super) fn compress(raw: &[u8], output: &mut Vec<u8>) {
    let start = output.len();
    output.resize(start + get_maximum_output_size(raw.len()), 0);
    // The space is the largest size that a block of `raw` can have, thus
    // the encoder has no error. A block of that size is never stored: it is
    // longer than `raw`.
    let length = compress_into(raw, &mut output[start..]).unwrap_or(output.len() - start);
    output.truncate(start + length);
}

/// Decodes one block that must have `raw_length` bytes. The decoder writes
/// only into that number of bytes, thus a block that says more cannot make
/// the reader allocate more.
pub(super) fn decompress(stored: &[u8], raw_length: usize) -> io::Result<Vec<u8>> {
    let mut raw = vec![0; raw_length];
    let length = decompress_into(stored, &mut raw)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if length != raw_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the block does not have the length of its record",
        ));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_that_does_not_have_the_length_of_its_record_is_refused() {
        let raw: Vec<u8> = (0..100_000u32)
            .flat_map(|number| (number / 7).to_le_bytes())
            .collect();
        let mut block = Vec::new();
        compress(&raw, &mut block);

        assert!(block.len() < raw.len());
        assert_eq!(decompress(&block, raw.len()).unwrap(), raw);
        assert!(decompress(&block, raw.len() - 1).is_err());
        assert!(decompress(&block, raw.len() + 1).is_err());
        assert!(decompress(&block[..block.len() / 2], raw.len()).is_err());
    }
}
