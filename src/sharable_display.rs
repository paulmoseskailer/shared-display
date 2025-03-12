use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{OriginDimensions, Size},
    primitives::Rectangle,
    Pixel,
};

pub struct DisplayPartition<Display: ?Sized, BufferType: Sized> {
    pub buffer: *mut BufferType,
    pub partition: Rectangle,
    _display: core::marker::PhantomData<Display>,
}

impl<D, BufferType> DisplayPartition<D, BufferType>
where
    D: SharableBufferedDisplay,
    BufferType: Sized,
{
    pub fn new(buffer: &mut [BufferType], partition: Rectangle) -> DisplayPartition<D, BufferType> {
        DisplayPartition {
            buffer: buffer.as_mut_ptr(),
            partition,
            _display: core::marker::PhantomData,
        }
    }

    fn contains(&self, p: Point) -> bool {
        self.partition.contains(p)
    }

    fn set_pixel_checked(&mut self, dest: Point, value: BufferType) {
        if self.contains(dest) {
            // TODO this two should equal the number of partitions
            let offset: usize = (dest.x + self.partition.size.width as i32 * 2 * dest.y)
                .try_into()
                .unwrap();
            unsafe {
                *self.buffer.add(offset) = value;
            }
        }
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferType;

    fn split_display_buffer(
        &mut self, /* add option to split vertically here later */
    ) -> (
        DisplayPartition<Self, Self::BufferType>,
        DisplayPartition<Self, Self::BufferType>,
    );

    /// What value should be written into the buffer at the pixel's position?
    fn get_pixel_value(pixel: Pixel<Self::Color>) -> Self::BufferType;
}

impl<D, B> OriginDimensions for DisplayPartition<D, B>
where
    D: SharableBufferedDisplay,
{
    fn size(&self) -> Size {
        self.partition.size
    }
}

impl<D, B> DrawTarget for DisplayPartition<D, B>
where
    D: SharableBufferedDisplay<BufferType = B>,
{
    type Color = D::Color;
    type Error = D::Error;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels.into_iter().try_for_each(|p| {
            self.set_pixel_checked(p.0, D::get_pixel_value(p));
            Ok(())
        })
    }
}
