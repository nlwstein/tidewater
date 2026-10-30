//! The archive decoders' bit reader: they read their bits from the low end of each byte up.

pub(super) struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    count: u32,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            count: 0,
        }
    }

    /// Tops the buffer up with whole bytes while they fit.
    fn refill(&mut self) {
        while self.count <= 56 {
            let Some(&b) = self.data.get(self.pos) else {
                return;
            };
            self.buf |= u64::from(b) << self.count;
            self.pos += 1;
            self.count += 8;
        }
    }

    /// The next `n` bits (at most 32), zero past the end of the data, with how many of them are real.
    pub(super) fn peek(&mut self, n: u32) -> (usize, u32) {
        if self.count < n {
            self.refill();
        }
        ((self.buf & ((1 << n) - 1)) as usize, self.count.min(n))
    }

    /// Drops `n` bits that [`Self::peek`] said are real.
    pub(super) fn skip(&mut self, n: u32) {
        self.buf >>= n;
        self.count -= n;
    }

    /// The next `n` bits (at most 32); `None` past the end of the data.
    pub(super) fn take(&mut self, n: u32) -> Option<u32> {
        let (v, real) = self.peek(n);
        if real < n {
            return None;
        }
        self.skip(n);
        Some(v as u32)
    }
}
