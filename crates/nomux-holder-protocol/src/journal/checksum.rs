//! CRC-32 with the polynomial of IEEE 802.3, as in zlib and in PNG.

const POLYNOMIAL: u32 = 0xEDB8_8320;

const TABLE: [u32; 256] = table();

const fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index: u32 = 0;
    while index < 256 {
        let mut value = index;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ POLYNOMIAL
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index as usize] = value;
        index += 1;
    }
    table
}

pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut value = u32::MAX;
    for byte in bytes {
        let index = (value ^ u32::from(*byte)) & 0xff;
        value = (value >> 8) ^ TABLE[index as usize];
    }
    !value
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn the_checksum_is_the_one_of_zlib() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
