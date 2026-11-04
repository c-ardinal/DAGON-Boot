//! The owned, bounded form of payload fields (little-endian).

use dagon_format::version::Version;

/// A payload field type: its owned form is `SIZE` bytes. A field type of
/// another crate implements it with the compact form it wants recorded (a
/// length, an id, a digest prefix).
pub trait Compact: Copy {
    const SIZE: usize;
    /// Writes the owned form; `out.len() == SIZE`.
    fn put(&self, out: &mut [u8]);
}

macro_rules! int {
    ($($t:ty),*) => {$(
        impl Compact for $t {
            const SIZE: usize = core::mem::size_of::<$t>();
            fn put(&self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

int!(u8, u16, u32, u64, i8, i16, i32, i64);

impl Compact for bool {
    const SIZE: usize = 1;
    fn put(&self, out: &mut [u8]) {
        out[0] = u8::from(*self);
    }
}

impl<const N: usize> Compact for [u8; N] {
    const SIZE: usize = N;
    fn put(&self, out: &mut [u8]) {
        out.copy_from_slice(self);
    }
}

impl Compact for Version {
    const SIZE: usize = 4;
    fn put(&self, out: &mut [u8]) {
        self.raw().put(out);
    }
}

impl<T: Compact> Compact for Option<T> {
    /// A presence byte, then the value (zeros when absent).
    const SIZE: usize = 1 + T::SIZE;
    fn put(&self, out: &mut [u8]) {
        out.fill(0);
        if let Some(v) = self {
            out[0] = 1;
            v.put(&mut out[1..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_little_endian_and_options_carry_presence() {
        let mut b = [0xFFu8; 5];
        Some(0x0102_0304u32).put(&mut b);
        assert_eq!(b, [1, 4, 3, 2, 1]);
        None::<u32>.put(&mut b);
        assert_eq!(b, [0; 5]);
        let mut v = [0u8; 4];
        Version::new(1, 2, 3).put(&mut v);
        assert_eq!(u32::from_le_bytes(v), Version::new(1, 2, 3).raw());
    }
}
