#![no_std]
#![allow(async_fn_in_trait)]

pub mod compressed;

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

pub const MAX_APPS_PER_SCREEN: usize = 8;

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

    async fn draw_iter_internal<I>(
        &mut self,
        pixels: I,
        update_dirty_area: bool,
    ) -> Result<(), D::Error>
    where
        I: ::core::iter::IntoIterator<Item = Pixel<D::Color>>,
    {
        let whole_buffer: &mut [B] =
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
                    D::set_pixel(&mut whole_buffer[buffer_index], p);
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
