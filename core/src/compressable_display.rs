use core::cmp::PartialEq;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Point, prelude::*, primitives::Rectangle,
};

// requires embedded-alloc for no_std
extern crate alloc;
use alloc::vec::Vec;

use crate::{MAX_APPS_PER_SCREEN, NewPartitionError, compressed_buffer::*, flush_lock::FlushLock};

/// A buffered [`DrawTarget`] that can be compressed and shared among multiple apps.
pub trait CompressableDisplay: DrawTarget {
    /// The type of elements saved to the buffer - may differ from [`DrawTarget::Color`].
    type BufferElement: Copy + PartialEq + Default;

    /// Specify how `Color` maps to  `BufferElement`.
    fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement;

    /// Calculate the buffer position of a [`Point`].
    fn calculate_buffer_index(point: Point, buffer_area_size: Size) -> usize;

    /// Flushes a given chunk. Called once per chunk for every flush.
    async fn flush_chunk(&mut self, chunk: &[Self::BufferElement], chunk_area: Rectangle);
}

/// A partition of a [`CompressableDisplay`].
pub struct CompressedDisplayPartition<D: CompressableDisplay> {
    id: u8,
    buffer: CompressedBuffer<D::BufferElement>,
    /// Size of the parent display.
    pub parent_size: Size,
    /// Size of the partition itself.
    pub area: Rectangle,

    _display: core::marker::PhantomData<D>,
    flush_request_channel: &'static Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN>,
}

impl<D: CompressableDisplay> ContainsPoint for CompressedDisplayPartition<D> {
    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }
}

impl<D: CompressableDisplay> Dimensions for CompressedDisplayPartition<D> {
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}

impl<B, D> CompressedDisplayPartition<D>
where
    B: Default + Copy + PartialEq,
    D: CompressableDisplay<BufferElement = B>,
{
    /// Creates a new partition.
    pub fn new(
        id: u8,
        parent_size: Size,
        area: Rectangle,
        flush_request_channel: &'static Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN>,
    ) -> Result<CompressedDisplayPartition<D>, NewPartitionError> {
        if area.size.width < 8 {
            return Err(NewPartitionError::TooSmall);
        }
        if area.size.width % 8 != 0 {
            return Err(NewPartitionError::BadWidth);
        }

        Ok(CompressedDisplayPartition {
            id,
            buffer: CompressedBuffer::new(area.size, B::default()),
            parent_size,
            area,
            _display: core::marker::PhantomData,
            flush_request_channel,
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

    /// Request to flush this partition.
    pub async fn request_flush(&mut self) {
        self.flush_request_channel.send(self.id).await;
    }
}

impl<B, D> DrawTarget for CompressedDisplayPartition<D>
where
    D: CompressableDisplay<BufferElement = B>,
    B: Copy + PartialEq,
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
                Ok(())
            })
            .await
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
