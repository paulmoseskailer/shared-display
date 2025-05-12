#![allow(async_fn_in_trait)]

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::prelude::{ContainsPoint, PointsIter};
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{Dimensions, PixelColor, Size},
    primitives::Rectangle,
};
use std::time::Instant;

// TODO: this could be an associated const for SharableBufferedDisplay
// once that feature is stabilized
pub const MAX_NUM_PIXELS: usize = 128 * 96;

#[derive(Debug)]
pub enum PartitioningError {
    // cannot create partitions less than 8 pixels wide
    PartitionTooSmall,
    // display width must be divisible by both pixels as well as buffer elements
    BufferPixelMismatch,
    // a partition should have width divisible by 8
    PartitionBadWidth,
    OutsideParent,
    Overlaps,
    // when re-splitting partitions
    ExistingNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AreaToFlush {
    All,
    Some(Rectangle),
    None,
}

impl AreaToFlush {
    pub fn include(&mut self, other: &Rectangle) {
        match self {
            AreaToFlush::All => {}
            AreaToFlush::None => *self = AreaToFlush::Some(other.clone()),
            AreaToFlush::Some(rect) => {
                *self = AreaToFlush::Some(rect.envelope(other));
            }
        }
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement: Copy + PartialEq;

    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize;

    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>);

    fn get_compressed_buffer(
        &mut self,
    ) -> heapless::Vec<(Self::BufferElement, u8), MAX_NUM_PIXELS> {
        let start = Instant::now();
        let mut result = heapless::Vec::new();
        let mut running_sum: usize = 0;
        eprintln!("TODO: remove copy while compressing!");
        let mut source = self.get_buffer().iter();
        let len_uncompressed = source.len();
        if let Some(first) = source.next() {
            let mut prev_color = *first;
            let mut run_length = 1;
            for &color in source {
                if color == prev_color && run_length < u8::MAX {
                    run_length += 1;
                } else {
                    result
                        .push((prev_color, run_length))
                        .unwrap_or_else(|_| panic!("compressed_buffer too small!"));
                    running_sum += run_length as usize;
                    prev_color = color;
                    run_length = 1;
                }
            }
            result
                .push((prev_color, run_length))
                .unwrap_or_else(|_| panic!("compressed_buffer too small!"));
            running_sum += run_length as usize;
            assert_eq!(running_sum, len_uncompressed);
        }
        println!(
            "compressing took {}us, len {} -> {}",
            Instant::now().duration_since(start).as_micros(),
            len_uncompressed,
            result.len()
        );
        result
    }

    fn decompress_buffer(
        &mut self,
        compressed_buffer: &heapless::Vec<(Self::BufferElement, u8), MAX_NUM_PIXELS>,
    ) {
        let start = Instant::now();
        let destination = self.get_buffer();
        let mut i = 0;
        let mut src_iter = compressed_buffer.iter();
        while let Some((color, run_length)) = src_iter.next() {
            for offset in 0..*run_length {
                destination[i + offset as usize] = *color;
            }
            i += *run_length as usize;
        }
        println!(
            "decompressing took {}us",
            Instant::now().duration_since(start).as_micros()
        );
    }
}

pub struct CompressedDisplay<D: SharableBufferedDisplay> {
    pub display: D,
    pub compressed_buffer: heapless::Vec<(D::BufferElement, u8), MAX_NUM_PIXELS>,
}

impl<D: SharableBufferedDisplay> CompressedDisplay<D> {
    pub fn new(mut display: D) -> Self {
        let compressed_buffer = display.get_compressed_buffer();
        Self {
            display,
            compressed_buffer,
        }
    }

    pub fn new_partition(
        &mut self,
        area: Rectangle,
        draw_tracker: &'static DrawTracker,
    ) -> Result<DisplayPartition<D::BufferElement, D>, PartitioningError> {
        if area.size.width < 8 {
            return Err(PartitioningError::PartitionTooSmall);
        }

        let parent_size = self.bounding_box().size;
        let buffer_len = self.display.get_buffer().len();
        let pixels_per_buffer_el = (parent_size.width * parent_size.height) as usize / buffer_len;
        if pixels_per_buffer_el > 0 && parent_size.width % pixels_per_buffer_el as u32 != 0 {
            return Err(PartitioningError::BufferPixelMismatch);
        }

        if area.size.width % 8 != 0 {
            return Err(PartitioningError::PartitionBadWidth);
        }

        Ok(DisplayPartition::new(
            &mut self.compressed_buffer,
            parent_size,
            area,
            draw_tracker,
        ))
    }

    pub fn decompress_buffer(&mut self) {
        self.display.decompress_buffer(&self.compressed_buffer);
    }
}

impl<D: SharableBufferedDisplay> Dimensions for CompressedDisplay<D> {
    fn bounding_box(&self) -> Rectangle {
        self.display.bounding_box()
    }
}

pub struct DrawTracker {
    is_dirty: AtomicBool,
    pub dirty_area: Mutex<CriticalSectionRawMutex, AreaToFlush>,
}

impl DrawTracker {
    pub const fn new() -> Self {
        Self {
            is_dirty: AtomicBool::new(false),
            dirty_area: Mutex::new(AreaToFlush::None),
        }
    }

    pub async fn take_dirty_area(&self) -> AreaToFlush {
        if self.is_dirty.load(Ordering::Acquire) {
            let mut guard = self.dirty_area.lock().await;
            let area = guard.clone();
            *guard = AreaToFlush::None;
            self.is_dirty.store(false, Ordering::Release);
            area
        } else {
            AreaToFlush::None
        }
    }
}

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut (B, u8),
    buffer_len: usize,
    pub parent_size: Size,
    pub area: Rectangle,
    _display: core::marker::PhantomData<D>,

    draw_tracker: &'static DrawTracker,
}

impl<C, B, D> ContainsPoint for DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }
}

impl<C, B, D> DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
{
    pub fn new(
        buffer: &mut heapless::Vec<(B, u8), MAX_NUM_PIXELS>,
        parent_size: Size,
        area: Rectangle,
        draw_tracker: &'static DrawTracker,
    ) -> DisplayPartition<B, D> {
        DisplayPartition {
            buffer: buffer.as_mut_ptr(),
            parent_size,
            buffer_len: buffer.len(),
            area,
            _display: core::marker::PhantomData,
            draw_tracker,
        }
    }

    pub fn envelope(&mut self, other: &Rectangle) {
        self.area = self.area.envelope(other);
    }

    fn get_compressed_buffer_index(
        compressed_buffer: &mut [(B, u8)],
        target_index: usize,
    ) -> usize {
        let mut decompressed_index = 0;
        let mut compressed_index = 0;
        while decompressed_index < target_index {
            let (_color, run_length) = &compressed_buffer[compressed_index];
            if (decompressed_index + *run_length as usize) > target_index {
                break;
            }
            decompressed_index += *run_length as usize;
            compressed_index += 1;
        }
        compressed_index
    }

    // Internalized to add the extra parameter update_dirty_area
    async fn draw_iter_internal<I>(
        &mut self,
        pixels: I,
        update_dirty_area: bool,
    ) -> Result<(), D::Error>
    where
        I: ::core::iter::IntoIterator<Item = Pixel<D::Color>>,
    {
        let compressed_buffer: &mut [(B, u8)] =
            // Safety: we check that every index is within our owned slice
            unsafe { core::slice::from_raw_parts_mut(self.buffer, self.buffer_len) };
        let mut has_drawn = false;
        pixels
            .into_iter()
            .map(|pixel| Pixel(pixel.0 + self.area.top_left, pixel.1))
            .filter(|Pixel(pos, _color)| self.contains(*pos))
            .for_each(|p| {
                let buffer_index = D::calculate_buffer_index(p.0, self.parent_size);
                if self.contains(p.0) {
                    let compressed_buffer_index =
                        Self::get_compressed_buffer_index(compressed_buffer, buffer_index);
                    // TODO: set_pixel in compressed buffer correctly
                    // (this is the uncompressed set_pixel function directly)
                    D::set_pixel(&mut compressed_buffer[compressed_buffer_index].0, p);
                    has_drawn = true;
                }
            });
        if has_drawn {
            self.draw_tracker.is_dirty.store(true, Ordering::Relaxed);
            if update_dirty_area {
                *self.draw_tracker.dirty_area.lock().await = AreaToFlush::All;
            }
        }
        Ok(())
    }
}

impl<B, D> Dimensions for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}

impl<B, D> DrawTarget for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    type Color = D::Color;
    type Error = D::Error;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: ::core::iter::IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.draw_iter_internal(pixels, true).await
    }

    async fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        self.draw_tracker.dirty_area.lock().await.include(area);
        self.draw_iter_internal(
            area.points()
                .zip(colors)
                .map(|(pos, color)| Pixel(pos, color)),
            false,
        )
        .await
    }

    // Make sure to remove the offset from the Rectangle to be cleared,
    // draw_iter adds it again
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        *self.draw_tracker.dirty_area.lock().await = AreaToFlush::All;

        self.fill_solid(&(Rectangle::new(Point::new(0, 0), self.area.size)), color)
            .await
    }
}
