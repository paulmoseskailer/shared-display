use core::cmp::PartialEq;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    prelude::*,
    prelude::{Dimensions, PixelColor, Size},
    primitives::Rectangle,
};

// requires embedded-alloc for no_std
extern crate alloc;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::{DisplaySidePartitioningError, SharableBufferedDisplay, flush_lock::FlushLock};

/// A [`SharableBufferedDisplay`] that can compressed.
pub trait CompressableDisplay:
    SharableBufferedDisplay<BufferElement: Copy + PartialEq + Default>
{
    /// Flushes a given chunk. Called once per chunk for every flush.
    async fn flush_chunk(&mut self, chunk: Vec<Self::BufferElement>, chunk_area: Rectangle);

    /// Drops the original buffer if one exists. [`CompressedDisplayPartition`]s assign their
    /// own buffers.
    // TODO: reduce buffer to chunk size instead
    fn drop_buffer(&mut self);
}

/// A partition of a [`CompressableDisplay`].
pub struct CompressedDisplayPartition<D: SharableBufferedDisplay + ?Sized>
where
    D::BufferElement: core::cmp::PartialEq + Copy,
{
    buffer: CompressedBuffer<D::BufferElement>,
    /// Size of the parent display.
    pub parent_size: Size,
    /// Size of the partition itself.
    pub area: Rectangle,

    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> ContainsPoint for CompressedDisplayPartition<D>
where
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }
}

impl<C, B, D> Dimensions for CompressedDisplayPartition<D>
where
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}

impl<C, B, D> CompressedDisplayPartition<D>
where
    C: PixelColor,
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    /// Creates a new partition.
    pub fn new(
        parent_size: Size,
        area: Rectangle,
    ) -> Result<CompressedDisplayPartition<D>, DisplaySidePartitioningError> {
        if area.size.width < 8 {
            return Err(DisplaySidePartitioningError::PartitionTooSmall);
        }
        if area.size.width % 8 != 0 {
            return Err(DisplaySidePartitioningError::PartitionBadWidth);
        }

        Ok(CompressedDisplayPartition {
            buffer: CompressedBuffer::new(area.size, B::default()),
            parent_size,
            area,
            _display: core::marker::PhantomData,
        })
    }

    /// Increase this partition's size.
    pub fn envelope(&mut self, other: &Rectangle) {
        self.area = self.area.envelope(other);
        todo!("enveloping compressed partitions not yet implemented");
    }

    /// Provide a raw pointer to the compressed buffer.
    pub fn get_ptr_to_buffer(&self) -> *const Vec<(B, u8)> {
        self.buffer.get_ptr_to_inner()
    }
}

impl<B, D> DrawTarget for CompressedDisplayPartition<D>
where
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B>,
{
    type Color = D::Color;
    type Error = D::Error;
    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        FlushLock::new()
            .protect_write(|| {
                let self_area = self.area;
                let self_offset = self_area.top_left;
                pixels
                    .into_iter()
                    .filter(|Pixel(pos, _color)| self_area.contains(*pos + self_offset))
                    .for_each(|p| {
                        let target_index = D::calculate_buffer_index(p.0, self.area.size);
                        self.buffer
                            .set_at_index(target_index, D::map_to_buffer_element(p.1));
                    });
                if self.buffer.check_integrity().is_err() {
                    panic!("after draw_iter check rle failed");
                }
            })
            .await;
        Ok(())
    }

    // TODO: implement fill_contiguous, fill_solid efficiently
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.buffer
            .clear_and_refill(D::map_to_buffer_element(color));
        Ok(())
    }
}

/// An RLE-encoded framebuffer.
struct CompressedBuffer<B: Copy + PartialEq> {
    inner: Box<Vec<(B, u8)>>,
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
        return Err(());
    }

    fn set_at_index(&mut self, target_index: usize, new_value: B) {
        let mut current_index = 0;
        let mut run_index = 0;
        let mut iter = self.inner.iter();
        while let Some((_color, run_length)) = iter.next() {
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
