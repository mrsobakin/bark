pub trait CallbackResult: Sized {
    fn ok() -> Self;
    fn err(self) -> Option<Self>;
}

impl<E> CallbackResult for Result<(), E> {
    fn ok() -> Self {
        Ok(())
    }
    fn err(self) -> Option<Self> {
        self.is_err().then_some(self)
    }
}

impl CallbackResult for () {
    fn ok() -> Self {}
    fn err(self) -> Option<Self> {
        None
    }
}

#[derive(Clone)]
pub struct Chunker<T: Default + Copy, const N: usize> {
    buf: [T; N],
    len: usize,
}

impl<T: Default + Copy, const N: usize> Default for Chunker<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default + Copy, const N: usize> Chunker<T, N> {
    pub fn new() -> Self {
        Self {
            buf: [T::default(); N],
            len: 0,
        }
    }

    /// Process input data, calling `f` once for every complete frame of `N` elements.
    pub fn feed<R: CallbackResult>(
        &mut self,
        mut data: &[T],
        mut f: impl FnMut(&[T; N]) -> R,
    ) -> R {
        // 1. Complete any pending partial frame.
        if self.len > 0 {
            let take = usize::min(N - self.len, data.len());

            self.buf[self.len..self.len + take].copy_from_slice(&data[..take]);
            self.len += take;
            data = &data[take..];

            if self.len == N {
                self.len = 0;
                if let Some(e) = f(&self.buf).err() {
                    return e;
                };
            } else {
                return R::ok();
            }
        }

        // 2. Process whole frames directly from input.
        while let Some((frame, tail)) = data.split_first_chunk::<N>() {
            if let Some(e) = f(frame).err() {
                return e;
            };
            data = tail;
        }

        // 3. Stash remainder for next call.
        self.len = data.len();
        if self.len > 0 {
            self.buf[..self.len].copy_from_slice(data);
        }

        R::ok()
    }

    /// If there is a pending partial frame, pad it with `T::default()` and call `f` on it.
    pub fn finish<R>(&mut self, f: impl FnOnce(&[T; N]) -> R) -> R
    where
        R: CallbackResult,
    {
        if self.len > 0 {
            self.buf[self.len..N].fill(T::default());
            self.len = 0;
            f(&self.buf)
        } else {
            R::ok()
        }
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }
}
