use std::{ffi::CStr, fmt};

#[cfg_attr(test, derive(PartialEq))]
pub struct FixedStr<const N: usize> {
    bytes: [u8; N],
    ind: usize,
}

impl<const N: usize> FixedStr<N> {
    #[inline]
    pub fn new() -> Self {
        Self {
            bytes: [0u8; N],
            ind: 0,
        }
    }

    pub fn from_slice(buf: &[u8]) -> Option<Self> {
        if buf.len() > N {
            return None;
        }

        let mut bytes = [0u8; N];
        bytes[..buf.len()].copy_from_slice(buf);

        Some(Self {
            bytes,
            ind: buf.len(),
        })
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ind = 0;
        // Really only needed for CStr version, but whatever
        self.bytes.fill(0);
    }
}

#[cfg(test)]
impl<const N: usize> fmt::Debug for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match std::str::from_utf8(&self.bytes[..self.ind]) {
            Ok(s) => write!(f, "'{}'", s),
            Err(_) => f.write_str("non utf-8 string"),
        }
    }
}

impl<const N: usize> AsRef<str> for FixedStr<N> {
    #[inline]
    fn as_ref(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.ind]) }
    }
}

impl<const N: usize> fmt::Write for FixedStr<N> {
    fn write_str(&mut self, s: &str) -> Result<(), fmt::Error> {
        if self.ind + s.len() > N {
            return Err(fmt::Error);
        }

        self.bytes[self.ind..self.ind + s.len()].copy_from_slice(s.as_bytes());
        self.ind += s.len();
        Ok(())
    }
}

pub struct FixedCStr<const N: usize> {
    inner: FixedStr<N>,
}

impl<const N: usize> FixedCStr<N> {
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: FixedStr::new(),
        }
    }

    pub fn from_ptr(ptr: *const libc::c_char) -> Option<Self> {
        unsafe {
            if ptr.is_null() {
                return None;
            }

            let str_len = libc::strlen(ptr);

            if str_len >= N {
                return None;
            }

            let slice = std::slice::from_raw_parts(ptr.cast::<u8>(), str_len);

            let mut inner = FixedStr::new();
            inner.bytes[..str_len].copy_from_slice(slice);
            inner.ind = str_len;

            Some(Self { inner })
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<const N: usize> AsRef<CStr> for FixedCStr<N> {
    #[inline]
    fn as_ref(&self) -> &CStr {
        unsafe { CStr::from_bytes_with_nul_unchecked(&self.inner.bytes[..self.inner.ind + 1]) }
    }
}

impl<const N: usize> fmt::Write for FixedCStr<N> {
    fn write_str(&mut self, s: &str) -> Result<(), fmt::Error> {
        if self.inner.ind + s.len() + 1 > N {
            return Err(fmt::Error);
        }

        self.inner.write_str(s)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn simple() {
        let mut fstr = FixedStr::<32>::new();
        write!(&mut fstr, "/proc/{}/task", 35234).unwrap();
        assert_eq!(fstr.as_ref(), "/proc/35234/task");

        let mut fcstr = FixedCStr::<32>::new();
        write!(&mut fcstr, "/proc/{}/task", 35234).unwrap();
        assert_eq!(
            fcstr.as_ref(),
            CStr::from_bytes_with_nul(b"/proc/35234/task\0").unwrap()
        );
    }

    #[test]
    fn too_long() {
        let mut fstr = FixedStr::<15>::new();
        assert!(write!(&mut fstr, "/proc/{}/task", 35234).is_err());
        assert_eq!(fstr.as_ref(), "/proc/35234");

        let mut fcstr = FixedCStr::<16>::new();
        assert!(write!(&mut fcstr, "/proc/{}/task", 35234).is_err());
        assert_eq!(
            fcstr.as_ref(),
            CStr::from_bytes_with_nul(b"/proc/35234\0").unwrap()
        );
    }
}
