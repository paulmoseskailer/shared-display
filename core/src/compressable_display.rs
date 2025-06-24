use core::cmp::PartialEq;
use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Point, prelude::*, primitives::Rectangle,
};

// requires embedded-alloc for no_std
extern crate alloc;
use alloc::vec::Vec;

use crate::{
    DisplaySidePartitioningError, SharableBufferedDisplay, compressed_buffer::*,
    flush_lock::FlushLock,
};

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
                            .set_at_index(target_index, D::map_to_buffer_element(p.1))
                            .unwrap();
                    });
                if self.buffer.check_integrity().is_err() {
                    panic!("after draw_iter check rle failed");
                }
            })
            .await;
        Ok(())
    }

    async fn fill_solid(
        &mut self,
        area: &Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let buffer_element = D::map_to_buffer_element(color);

        // fill row-by-row
        let row_starts = core::iter::repeat(area.top_left)
            .take(area.size.height as usize)
            .enumerate()
            .map(|(i, p)| p + Point::new(0, i as i32));
        for row_start in row_starts {
            let target_index = D::calculate_buffer_index(row_start, self.area.size);
            self.buffer
                .set_at_index_contiguous(target_index, buffer_element, area.size.width as usize)
                .unwrap();
        }
        Ok(())
    }

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.buffer
            .clear_and_refill(D::map_to_buffer_element(color));
        Ok(())
    }
}
