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
use alloc::vec;
use alloc::vec::Vec;

use crate::{SharableBufferedDisplay, flush_lock::FlushLock};

pub trait CompressableDisplay:
    SharableBufferedDisplay<BufferElement: Copy + PartialEq + Default>
{
    // TODO: this drop does not seem necessary?
    fn drop_buffer(&mut self);
}

pub struct CompressedDisplayPartition<B: core::cmp::PartialEq + Copy, D: ?Sized> {
    pub area: Rectangle,
    pub buffer: Vec<(B, u8)>,
    pub parent_size: Size,
    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> ContainsPoint for CompressedDisplayPartition<B, D>
where
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }
}

impl<C, B, D> Dimensions for CompressedDisplayPartition<B, D>
where
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}

pub fn get_compressed_display_with_value<B: Copy>(area: Rectangle, value: B) -> Vec<(B, u8)> {
    let num_pixels = area.size.width * area.size.height;
    let num_runs = match num_pixels % 255 {
        0 => num_pixels / 255,
        _ => num_pixels / 255 + 1,
    };
    vec![(value, 255); num_runs as usize]
}

impl<C, B, D> CompressedDisplayPartition<B, D>
where
    C: PixelColor,
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    pub fn new(parent_size: Size, area: Rectangle) -> CompressedDisplayPartition<B, D> {
        CompressedDisplayPartition {
            buffer: get_compressed_display_with_value(area, B::default()),
            parent_size,
            area,
            _display: core::marker::PhantomData,
        }
    }

    pub fn envelope(&mut self, other: &Rectangle) {
        self.area = self.area.envelope(other);
        todo!("enveloping compressed partitions not yet implemented");
    }

    fn set_pixel(&mut self, pixel: Pixel<C>) {
        let target_index = D::calculate_buffer_index(pixel.0, self.parent_size);
        let new_value = pixel.1;

        let mut current_index = D::calculate_buffer_index(self.area.top_left, self.parent_size);
        let mut run_index = 0;
        let mut iter = self.buffer.iter();
        while let Some((_color, run_length)) = iter.next() {
            if current_index + *run_length as usize > target_index {
                break;
            }
            current_index += *run_length as usize;
            run_index += 1;
        }
        let (buffer_before_ref, run_len_before) = &self.buffer[run_index];
        if D::map_to_buffer_element(new_value) == *buffer_before_ref {
            return;
        }
        let (buffer_before, run_len_before) = (*buffer_before_ref, *run_len_before);

        let run_before_len = target_index - current_index;
        let run_after_len = (current_index + run_len_before as usize) - (target_index + 1);
        let have_run_before = run_before_len > 0;
        // new pixel
        self.buffer[run_index] = (D::map_to_buffer_element(new_value), 1);
        if have_run_before {
            self.buffer.insert(
                run_index,
                (buffer_before, run_before_len.try_into().unwrap()),
            );
        }
        if run_after_len > 0 {
            let index = run_index + 1 + have_run_before as usize;
            self.buffer
                .insert(index, (buffer_before, run_after_len.try_into().unwrap()));
        }
    }
}

impl<B, D> DrawTarget for CompressedDisplayPartition<B, D>
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
                        self.set_pixel(p);
                    });
            })
            .await;
        Ok(())
    }

    // TODO: implement fill_contiguous, fill_solid efficiently

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        FlushLock::new()
            .protect_write(|| {
                self.buffer.clear();
                self.buffer =
                    get_compressed_display_with_value(self.area, D::map_to_buffer_element(color));
            })
            .await;
        Ok(())
    }
}
