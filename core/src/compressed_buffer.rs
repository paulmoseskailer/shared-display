use core::cmp::PartialEq;
use embedded_graphics::prelude::*;

// requires embedded-alloc for no_std
extern crate alloc;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

/// An RLE-encoded framebuffer.
#[allow(clippy::box_collection)]
pub struct CompressedBuffer<B: Copy + PartialEq> {
    pub(crate) inner: Box<Vec<(B, u8)>>,
    decompressed_size: Size,
}

impl<B: Copy + PartialEq> CompressedBuffer<B> {
    /// Creates a new compressed buffer with a start value.
    pub fn new(decompressed_size: Size, start_value: B) -> Self {
        let num_pixels = decompressed_size.width * decompressed_size.height;
        let full_runs = num_pixels / 255;
        let mut buffer = vec![(start_value, 255); full_runs as usize];
        let remainder = num_pixels - (full_runs * 255);
        if remainder > 0 {
            buffer.push((start_value, remainder.try_into().unwrap()));
        }
        Self {
            inner: Box::new(buffer),
            decompressed_size,
        }
    }

    /// Returns a raw pointer to the inner buffer.
    pub fn get_ptr_to_inner(&self) -> *const Vec<(B, u8)> {
        &*self.inner
    }

    /// Check whether the buffer still encodes as many elements as it should.
    pub fn check_integrity(&self) -> Result<(), ()> {
        let decompressed_buffer_len = self.decompressed_size.width * self.decompressed_size.height;
        let actual_len = self
            .inner
            .iter()
            .fold(0_u64, |before, (_color, run_len)| before + *run_len as u64);
        if actual_len == decompressed_buffer_len as u64 {
            return Ok(());
        }
        Err(())
    }

    pub(crate) fn set_at_index(&mut self, target_index: usize, new_value: B) {
        let mut current_index = 0;
        let mut run_index = 0;
        for (_color, run_length) in self.inner.iter() {
            if current_index + *run_length as usize > target_index {
                break;
            }
            current_index += *run_length as usize;
            run_index += 1;
        }
        if run_index == self.inner.len() {
            panic!("set_pixel: did not find run to break up");
        }

        let (buffer_value_previously, run_len_previously) = &self.inner[run_index];
        if new_value == *buffer_value_previously {
            // nothing to do, color already set
            return;
        }
        let (buffer_previously, run_len_previously) =
            (*buffer_value_previously, *run_len_previously);

        let run_before_len = target_index - current_index;
        let run_after_len = (current_index + run_len_previously as usize) - (target_index + 1);

        let have_run_before = run_before_len > 0;
        let have_run_after = run_after_len > 0;
        // check if we can merge with previous run
        if !have_run_before && run_index > 0 {
            let (color_before, run_len_before) = &self.inner[run_index - 1];
            if *color_before == new_value && *run_len_before < 255 {
                // add current pixel to previous run
                self.inner[run_index - 1].1 += 1;
                self.inner[run_index].1 -= 1;
                if self.inner[run_index].1 == 0 {
                    // remove run
                    self.inner.remove(run_index);
                    // possibly merge run after
                    if run_index < self.inner.len() {
                        let (color_after, run_len_after) = &self.inner[run_index];
                        let combined_len =
                            self.inner[run_index - 1].1.saturating_add(*run_len_after);
                        if combined_len < 255 && *color_after == new_value {
                            self.inner[run_index - 1].1 = combined_len;
                            self.inner.remove(run_index);
                        }
                    }
                }
                return;
            }
        }

        // check if we can merge with next run
        if !have_run_after && run_index < (self.inner.len() - 1) {
            let (color_after, run_len_after) = &self.inner[run_index + 1];
            if *color_after == new_value && *run_len_after < 255 {
                self.inner[run_index + 1].1 += 1;
                self.inner[run_index].1 -= 1;
                if self.inner[run_index].1 == 0 {
                    self.inner.remove(run_index);
                }
                return;
            }
        }

        // new pixel
        self.inner[run_index] = (new_value, 1);
        if have_run_before {
            self.inner.insert(
                run_index,
                (buffer_previously, run_before_len.try_into().unwrap()),
            );
        }
        if run_after_len > 0 {
            let index = run_index + 1 + have_run_before as usize;
            self.inner.insert(
                index,
                (buffer_previously, run_after_len.try_into().unwrap()),
            );
        }

        if self.check_integrity().is_err() {
            panic!(
                "after set_at_index({}) check_integrity failed",
                target_index
            );
        }
    }

    /// Empty the buffer and refill it with a new value.
    pub fn clear_and_refill(&mut self, new_value: B) {
        // empty first
        self.inner.clear();
        // then re-fill
        let num_pixels = self.decompressed_size.width * self.decompressed_size.height;
        let full_runs = num_pixels / 255;
        for _ in 0..full_runs {
            self.inner.push((new_value, 255));
        }
        let remainder = num_pixels - (full_runs * 255);
        if remainder > 0 {
            self.inner.push((new_value, remainder.try_into().unwrap()));
        }
    }
}

#[derive(Clone)]
pub struct DecompressingIter<'a, B: Copy + PartialEq + Default> {
    current_run: Option<(B, u8)>,
    compressed_buffer_iter: core::slice::Iter<'a, (B, u8)>,
    decompressed_index: usize,
}

impl<'a, B: Copy + PartialEq + Default> DecompressingIter<'a, B> {
    pub fn new(buffer: &'a Vec<(B, u8)>) -> Self {
        let mut compressed_buffer_iter = buffer.iter();
        let current_run = compressed_buffer_iter.next().map(|&r| r);
        Self {
            current_run,
            compressed_buffer_iter,
            decompressed_index: 0,
        }
    }
}

impl<'a, B: Copy + PartialEq + Default> Iterator for DecompressingIter<'a, B> {
    type Item = B;

    fn next(&mut self) -> Option<Self::Item> {
        let (current_value, items_left_in_run) = self.current_run?;
        if items_left_in_run > 1 {
            self.current_run?.1 -= 1;
        } else {
            // consuming last element of current_run
            let &(next_value, next_run_len) = self.compressed_buffer_iter.next()?;
            assert_ne!(next_run_len, 0, "run with length 0 found");

            self.current_run = Some((next_value, next_run_len));
        }
        self.decompressed_index += 1;
        Some(current_value)
    }

    fn nth(&mut self, n: usize) -> Option<B> {
        if n == 0 {
            self.next();
        }

        let (_current_value, items_left_in_run) = self.current_run?;
        if n < (items_left_in_run as usize) {
            // nth item is in current run
            self.current_run?.1 -= <usize as TryInto<u8>>::try_into(n).unwrap();
            self.decompressed_index += n;

            self.next()
        } else {
            // not enough items in current run, skip to next run
            let remaining_n = n - items_left_in_run as usize;
            self.decompressed_index += items_left_in_run as usize;

            let &(next_value, next_run_len) = self.compressed_buffer_iter.next()?;
            assert_ne!(next_run_len, 0, "run with length 0 found");
            self.current_run = Some((next_value, next_run_len));

            self.nth(remaining_n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clear() {
        let size = Size::new(128, 4); // 512 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 45);
        buffer.check_integrity().unwrap();

        buffer.clear_and_refill(255);
        assert_eq!(
            buffer.inner,
            Box::new(vec![(255, 255), (255, 255), (255, 2)])
        );
    }

    #[test]
    fn test_merge_before() {
        let size = Size::new(4, 4); // 16 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 30);
        buffer.check_integrity().unwrap();

        buffer.set_at_index(2, 52);
        assert_eq!(buffer.inner, Box::new(vec![(30, 2), (52, 1), (30, 13)]));

        buffer.set_at_index(3, 52);
        assert_eq!(buffer.inner, Box::new(vec![(30, 2), (52, 2), (30, 12)]));
    }

    #[test]
    fn test_merge_after() {
        let size = Size::new(4, 4); // 16 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 30);
        buffer.check_integrity().unwrap();

        buffer.set_at_index(2, 52);
        assert_eq!(buffer.inner, Box::new(vec![(30, 2), (52, 1), (30, 13)]));

        buffer.set_at_index(1, 52);
        assert_eq!(buffer.inner, Box::new(vec![(30, 1), (52, 2), (30, 13)]));
    }

    #[test]
    fn test_merge_before_and_after() {
        let size = Size::new(128, 2); // 256 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity().unwrap();
        assert_eq!(buffer.inner, Box::new(vec![(0, 255), (0, 1)]));

        buffer.set_at_index(0, 27);
        assert_eq!(buffer.inner, Box::new(vec![(27, 1), (0, 254), (0, 1)]));

        buffer.set_at_index(2, 27);
        assert_eq!(
            buffer.inner,
            Box::new(vec![(27, 1), (0, 1), (27, 1), (0, 252), (0, 1)])
        );

        buffer.set_at_index(1, 27);
        assert_eq!(buffer.inner, Box::new(vec![(27, 3), (0, 252), (0, 1)]));
    }

    #[test]
    fn test_no_merge_over_255() {
        let size = Size::new(257, 1);
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity().unwrap();
        assert_eq!(buffer.inner, Box::new(vec![(0, 255), (0, 2)]));
        buffer.set_at_index(254, 3);

        assert_eq!(buffer.inner, Box::new(vec![(0, 254), (3, 1), (0, 2)]));
        buffer.set_at_index(254, 0);
        assert_eq!(buffer.inner, Box::new(vec![(0, 255), (0, 2)]));
    }
}
