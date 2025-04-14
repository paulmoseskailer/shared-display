#![no_std]
#![allow(async_fn_in_trait)]

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::prelude::ContainsPoint;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{Dimensions, PixelColor, Size},
    primitives::Rectangle,
};

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

pub struct DrawTracker {
    is_dirty: AtomicBool,
    pub dirty_area: Mutex<CriticalSectionRawMutex, Option<Rectangle>>,
}

impl DrawTracker {
    pub const fn new() -> Self {
        Self {
            is_dirty: AtomicBool::new(false),
            dirty_area: Mutex::new(None),
        }
    }

    pub async fn take_dirty_area(&self) -> Option<Rectangle> {
        if self.is_dirty.swap(false, Ordering::Acquire) {
            let mut guard = self.dirty_area.lock().await;
            let result = guard.clone().unwrap();
            *guard = None;
            Some(result)
        } else {
            None
        }
    }
}

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut B,
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
        buffer: &mut [B],
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

    pub fn split_vertically(
        &mut self,
    ) -> Result<(DisplayPartition<B, D>, DisplayPartition<B, D>), PartitioningError> {
        let size = self.area.size;

        // ensure no bytes are split in half by rounding to a split of width multiple of 8
        let left_area_width = (size.width / 2) + 7 & !7;
        let left_area = Rectangle::new(self.area.top_left, Size::new(left_area_width, size.height));
        let right_area = Rectangle::new(
            self.area.top_left + Point::new(left_area_width.try_into().unwrap(), 0),
            Size::new(size.width - left_area_width, size.height),
        );

        if left_area_width < 8 || size.width - left_area_width < 8 {
            return Err(PartitioningError::PartitionTooSmall);
        }

        let pixels_per_buffer_el =
            (self.parent_size.width * self.parent_size.height) as usize / self.buffer_len;
        if pixels_per_buffer_el > 0 && self.parent_size.width % pixels_per_buffer_el as u32 != 0 {
            return Err(PartitioningError::BufferPixelMismatch);
        }

        Ok((
            DisplayPartition::new(
                unsafe {
                    // SAFETY: self.buffer and self.buffer_len are initialized from slice in new
                    core::slice::from_raw_parts_mut(self.buffer, self.buffer_len)
                },
                self.parent_size,
                left_area,
                self.draw_tracker,
            ),
            DisplayPartition::new(
                unsafe {
                    // SAFETY: self.buffer and self.buffer_len are initialized from slice in new
                    core::slice::from_raw_parts_mut(self.buffer, self.buffer_len)
                },
                self.parent_size,
                right_area,
                self.draw_tracker,
            ),
        ))
    }

    pub fn envelope(&mut self, other: &Rectangle) {
        self.area = self.area.envelope(other);
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

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement;

    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize;

    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>);

    fn new_partition(
        &mut self,
        area: Rectangle,
        draw_tracker: &'static DrawTracker,
    ) -> Result<DisplayPartition<Self::BufferElement, Self>, PartitioningError> {
        if area.size.width < 8 {
            return Err(PartitioningError::PartitionTooSmall);
        }

        let parent_size = self.bounding_box().size;
        let buffer_len = self.get_buffer().len();
        let pixels_per_buffer_el = (parent_size.width * parent_size.height) as usize / buffer_len;
        if pixels_per_buffer_el > 0 && parent_size.width % pixels_per_buffer_el as u32 != 0 {
            return Err(PartitioningError::BufferPixelMismatch);
        }

        if area.size.width % 8 != 0 {
            return Err(PartitioningError::PartitionBadWidth);
        }

        Ok(DisplayPartition::new(
            self.get_buffer(),
            parent_size,
            area,
            draw_tracker,
        ))
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
        let mut dirty_area: Option<Rectangle> = self.draw_tracker.dirty_area.lock().await.clone();
        let whole_buffer: &mut [B] =
            // Safety: we check that every index is within our owned slice
            unsafe { core::slice::from_raw_parts_mut(self.buffer, self.buffer_len) };
        pixels
            .into_iter()
            .map(|pixel| Pixel(pixel.0 + self.area.top_left, pixel.1))
            .filter(|Pixel(pos, _color)| self.contains(*pos))
            .for_each(|p| {
                let buffer_index = D::calculate_buffer_index(p.0, self.parent_size);
                if self.contains(p.0) {
                    D::set_pixel(&mut whole_buffer[buffer_index], p);

                    dirty_area = match dirty_area {
                        None => Some(Rectangle::with_center(p.0, Size::default())),
                        Some(previous_area) => Some(
                            previous_area.envelope(&Rectangle::with_center(p.0, Size::default())),
                        ),
                    };
                }
            });
        if let Some(dirty_area) = dirty_area {
            self.draw_tracker.is_dirty.store(true, Ordering::Relaxed);
            let mut guard = self.draw_tracker.dirty_area.lock().await;
            *guard = Some(dirty_area);
        }
        Ok(())
    }

    // Make sure to remove the offset from the Rectangle to be cleared,
    // draw_iter adds it again
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.draw_tracker.is_dirty.store(true, Ordering::Relaxed);
        *self.draw_tracker.dirty_area.lock().await = Some(self.area);

        self.fill_solid(&(Rectangle::new(Point::new(0, 0), self.area.size)), color)
            .await
    }
}
