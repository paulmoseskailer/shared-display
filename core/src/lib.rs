#![no_std]
#![allow(async_fn_in_trait)]

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

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut B,
    buffer_len: usize,
    pub parent_size: Size,
    pub area: Rectangle,
    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
{
    pub fn new(buffer: &mut [B], parent_size: Size, area: Rectangle) -> DisplayPartition<B, D> {
        DisplayPartition {
            buffer: buffer.as_mut_ptr(),
            parent_size,
            buffer_len: buffer.len(),
            area,
            _display: core::marker::PhantomData,
        }
    }

    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
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
            ),
            DisplayPartition::new(
                unsafe {
                    // SAFETY: self.buffer and self.buffer_len are initialized from slice in new
                    core::slice::from_raw_parts_mut(self.buffer, self.buffer_len)
                },
                self.parent_size,
                right_area,
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

pub struct PixelInBuffer {
    pub start_index: usize,
    pub width_in_buffer_elements: usize,
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement;

    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    fn calculate_buffer_index(point: Point, parent_size: Size) -> PixelInBuffer;

    fn set_ith_buffer_element_for_pixel(
        buffer: &mut Self::BufferElement,
        pixel: Pixel<Self::Color>,
        i: usize,
    );

    fn new_partition(
        &mut self,
        area: Rectangle,
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

        Ok(DisplayPartition::new(self.get_buffer(), parent_size, area))
    }

    fn split_buffer_vertically(
        &mut self,
    ) -> Result<
        (
            DisplayPartition<Self::BufferElement, Self>,
            DisplayPartition<Self::BufferElement, Self>,
        ),
        PartitioningError,
    > {
        let parent_size = self.bounding_box().size;

        // ensure no bytes are split in half by rounding to a split of width multiple of 8
        let left_area_width = (parent_size.width / 2) + 7 & !7;
        let left_area = Rectangle::new(
            self.bounding_box().top_left,
            Size::new(left_area_width, parent_size.height),
        );
        let right_area = Rectangle::new(
            self.bounding_box().top_left + Point::new(left_area_width.try_into().unwrap(), 0),
            Size::new(parent_size.width - left_area_width, parent_size.height),
        );

        let left = self.new_partition(left_area)?;
        let right = self.new_partition(right_area)?;
        Ok((left, right))
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
                    for offset in 0..(buffer_index.width_in_buffer_elements) {
                        let index = buffer_index.start_index + offset;
                        // TODO maybe panic if index out of range? seems like a severe error
                        if index < self.buffer_len {
                            D::set_ith_buffer_element_for_pixel(
                                &mut whole_buffer[index],
                                p,
                                offset,
                            );
                        }
                    }
                }
            });
        Ok(())
    }

    // Make sure to remove the offset from the Rectangle to be cleared,
    // draw_iter adds it again
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_solid(&(Rectangle::new(Point::new(0, 0), self.area.size)), color)
            .await
    }
}
