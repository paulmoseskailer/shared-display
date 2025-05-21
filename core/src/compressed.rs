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
    pub buffer: Box<Vec<(B, u8)>>,
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
    let full_runs = num_pixels / 255;
    let mut result = vec![(value, 255); full_runs as usize];
    let remainder = num_pixels - (full_runs * 255);
    if remainder > 0 {
        result.push((value, remainder.try_into().unwrap()));
    }
    result
}

impl<C, B, D> CompressedDisplayPartition<B, D>
where
    C: PixelColor,
    B: Copy + core::cmp::PartialEq,
    D: CompressableDisplay<BufferElement = B, Color = C> + ?Sized,
{
    pub fn new(parent_size: Size, area: Rectangle) -> CompressedDisplayPartition<B, D> {
        CompressedDisplayPartition {
            buffer: Box::new(get_compressed_display_with_value(area, B::default())),
            parent_size,
            area,
            _display: core::marker::PhantomData,
        }
    }

    pub fn envelope(&mut self, other: &Rectangle) {
        self.area = self.area.envelope(other);
        todo!("enveloping compressed partitions not yet implemented");
    }

    fn check_rle(&self) -> Result<(), ()> {
        let resolution = self.area.size.width * self.area.size.height;
        let decompressed_buffer_len = self
            .buffer
            .iter()
            .fold(0_u64, |before, (_color, run_len)| before + *run_len as u64);
        if decompressed_buffer_len == resolution as u64 {
            return Ok(());
        }
        println!(
            "RLE ({} runs) encodes {decompressed_buffer_len} pixels != resolution {resolution}",
            self.buffer.len()
        );
        return Err(());
    }

    fn set_pixel(&mut self, pixel: Pixel<C>) {
        let target_index = D::calculate_buffer_index(pixel.0, self.area.size);
        let new_value = pixel.1;

        let mut current_index = 0;
        let mut run_index = 0;
        let mut iter = self.buffer.iter();
        while let Some((_color, run_length)) = iter.next() {
            if current_index + *run_length as usize > target_index {
                break;
            }
            current_index += *run_length as usize;
            run_index += 1;
        }
        if run_index == self.buffer.len() {
            panic!("set_pixel: did not find run to break up");
        }

        // TODO: merge runs if possible
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

        if self.check_rle().is_err() {
            panic!("set_pixel({:?}) check rle failed", pixel.0);
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
                if self.check_rle().is_err() {
                    panic!("after draw_iter check rle failed");
                }
            })
            .await;
        Ok(())
    }

    // TODO: implement clear, fill_contiguous, fill_solid efficiently
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        // empty vector
        self.buffer.clear();
        // re-fill vector
        let new_value = D::map_to_buffer_element(color);
        let num_pixels = self.area.size.width * self.area.size.height;
        let full_runs = num_pixels / 255;
        for _ in 0..full_runs {
            self.buffer.push((new_value, 255));
        }
        let remainder = num_pixels - (full_runs * 255);
        if remainder > 0 {
            self.buffer.push((new_value, remainder.try_into().unwrap()));
        }
        Ok(())
    }
}
