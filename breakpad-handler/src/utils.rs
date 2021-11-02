mod fixed_str;
pub mod fs;
mod line_reader;

pub use fixed_str::{FixedCStr, FixedStr};
pub use line_reader::LineReader;

#[inline]
pub fn to_byte_array<T: Sized>(item: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((item as *const T).cast::<u8>(), std::mem::size_of::<T>()) }
}
