use std::io::Read;

use super::FixedStr;

pub struct LineReader<R, const N: usize> {
    inner: R,
    buf: [u8; N],
    /// Read location
    cursor: usize,
    /// Filled end
    filled: usize,
    eof: bool,
}

impl<R: Read, const N: usize> LineReader<R, N> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: reader,
            buf: [0u8; N],
            cursor: 0,
            filled: 0,
            eof: false,
        }
    }
}

impl<R: Read, const N: usize> Iterator for LineReader<R, N> {
    type Item = FixedStr<N>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.eof {
            return None;
        }

        loop {
            if self.eof {
                if dbg!(self.cursor < self.filled) {
                    let ret = Self::Item::from_slice(&self.buf[self.cursor..self.filled]);
                    self.cursor = self.filled;
                    return ret;
                } else {
                    return None;
                }
            }

            for i in self.cursor..self.filled {
                let c = self.buf[i];
                if c == b'\n' {
                    let ret = Self::Item::from_slice(&self.buf[dbg!(self.cursor)..dbg!(i)]);
                    self.cursor = i + 1;
                    return ret;
                }
            }

            // Move any partial lines to the beginning and fill with
            // more data
            if dbg!(dbg!(self.cursor) < dbg!(self.filled)) {
                // A single line is too long to fit
                if self.cursor == 0 && self.filled == N {
                    return None;
                }

                let remaining = self.filled - self.cursor;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.buf.as_ptr().offset(self.cursor as isize),
                        self.buf.as_mut_ptr(),
                        remaining,
                    );
                }

                self.filled = dbg!(remaining);
            } else {
                self.filled = 0;
            }

            self.cursor = 0;

            match self.inner.read(&mut self.buf[self.filled..]) {
                Ok(read) => {
                    // EOF
                    if read == 0 {
                        self.eof = true;
                        continue;
                    }

                    self.filled += read;
                }
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn empty() {
        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(&[]));
        assert!(lr.next().is_none());
    }

    #[test]
    fn one_line_terminated() {
        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(b"line\n"));
        assert_eq!("line", lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn one_line_eof() {
        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(b"line"));
        assert_eq!("line", lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn two_lines_terminated() {
        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(b"one\ntwo\n"));
        assert_eq!("one", lr.next().unwrap().as_ref());
        assert_eq!("two", lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn two_lines_eof() {
        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(b"one\ntwo"));
        assert_eq!("one", lr.next().unwrap().as_ref());
        assert_eq!("two", lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn large_lines_eof() {
        let mut large_lines = [b'a'; 1024];

        large_lines[200] = b'\n';
        large_lines[401] = b'\n';
        large_lines[602] = b'\n';
        large_lines[803] = b'\n';
        large_lines[1004] = b'\n';

        let single_line = [b'a'; 200];
        let single_line = std::str::from_utf8(&single_line).unwrap();

        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(large_lines));

        for _ in 0..5 {
            assert_eq!(single_line, lr.next().unwrap().as_ref());
        }

        assert_eq!(
            std::str::from_utf8(&[b'a'; 19]).unwrap(),
            lr.next().unwrap().as_ref()
        );
        assert!(lr.next().is_none());
    }

    #[test]
    fn max_length_line_terminated() {
        let mut max = [b'1'; 512];
        max[511] = b'\n';
        let max_str = std::str::from_utf8(&max[..511]).unwrap();

        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(&max));
        assert_eq!(max_str, lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn max_length_line_eof() {
        let max = [b'1'; 511];
        let max_str = std::str::from_utf8(&max).unwrap();

        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(&max));
        assert_eq!(max_str, lr.next().unwrap().as_ref());
        assert!(lr.next().is_none());
    }

    #[test]
    fn too_long() {
        let too_long = [b'f'; 513];

        let mut lr = LineReader::<_, 512>::new(std::io::Cursor::new(&too_long));
        assert!(lr.next().is_none());
    }
}
