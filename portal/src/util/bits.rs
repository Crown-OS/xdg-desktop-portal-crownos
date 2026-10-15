/// The two 32-bit halves protocols carry a 64-bit value in, high first.
pub const fn split_u64(value: u64) -> [u32; 2] {
    [(value >> 32) as u32, value as u32]
}

pub const fn join_u32(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_trip() {
        let value = 0x0123_4567_89ab_cdef;
        let [hi, lo] = split_u64(value);
        assert_eq!(join_u32(hi, lo), value);
    }
}
