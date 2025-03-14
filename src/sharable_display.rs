use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{OriginDimensions, PixelColor, Size},
    primitives::Rectangle,
    Pixel,
};

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut B,
    buffer_len: usize,
    pub display_width: usize,
    pub partition: Rectangle,
    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C>,
{
    pub fn new(
        buffer: &mut [B],
        display_width: usize,
        partition: Rectangle,
    ) -> DisplayPartition<B, D> {
        DisplayPartition {
            buffer: buffer.as_mut_ptr(),
            display_width,
            buffer_len: buffer.len(),
            partition,
            _display: core::marker::PhantomData,
        }
    }

    fn contains(&self, p: Point) -> bool {
        self.partition.contains(p)
    }

    fn owns_index(&self, index: usize) -> bool {
        let i: u32 = index.try_into().unwrap();
        let screen_width = 2 * self.partition.size.width;
        let x = i % screen_width;
        // check if the x-coordinate of the index is within the owned partition
        // TODO adapt
        self.contains(Point::new(x.try_into().unwrap(), 0))
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement;

    fn split_display_buffer(
        &mut self, /* add option to split vertically here later */
    ) -> (
        DisplayPartition<Self::BufferElement, Self>,
        DisplayPartition<Self::BufferElement, Self>,
    );

    fn get_buffer_offset(pixel: Pixel<Self::Color>, display_width: usize) -> usize;

    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>);
}

impl<B, D> OriginDimensions for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    fn size(&self) -> Size {
        self.partition.size
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
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let whole_buffer: &mut [B] =
            // Safety: we check that every index is within our owned slice
            unsafe { core::slice::from_raw_parts_mut(self.buffer, self.buffer_len) };
        pixels
            .into_iter()
            .map(|pixel| Pixel(pixel.0 + self.partition.top_left, pixel.1))
            .for_each(|p| {
                let index = D::get_buffer_offset(p, self.display_width);
                if self.owns_index(index) {
                    D::set_pixel(&mut whole_buffer[index], p);
                }
            });
        Ok(())
    }

    // Make sure to remove the offset from the Rectangle to be cleared,
    // draw_iter adds it again
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_solid(
            &(Rectangle::new(Point::new(0, 0), self.partition.size)),
            color,
        )
        .await
    }
}

// TODO impl SharableBufferedDisplay for DisplayPartition
