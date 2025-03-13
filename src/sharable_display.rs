use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{OriginDimensions, PixelColor, Size},
    primitives::Rectangle,
    Pixel,
};

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut B,
    pub display_width: usize,
    buffer_len: usize,
    pub partition: Rectangle,
    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferType = B, Color = C>,
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
    type BufferType;

    fn split_display_buffer(
        &mut self, /* add option to split vertically here later */
    ) -> (
        DisplayPartition<Self::BufferType, Self>,
        DisplayPartition<Self::BufferType, Self>,
    );

    fn get_buffer_offset(pixel: Pixel<Self::Color>, display_width: usize) -> usize;

    fn set_pixel(buffer: &mut Self::BufferType, pixel: Pixel<Self::Color>);
}

impl<B, D> OriginDimensions for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferType = B>,
{
    fn size(&self) -> Size {
        self.partition.size
    }
}

impl<B, D> DrawTarget for DisplayPartition<B, D>
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
            let index = D::get_buffer_offset(p, self.display_width);
            if self.owns_index(index) {
                // Safety: we checked that index is within our owned slice
                unsafe {
                    let whole_buffer: &mut [B] =
                        core::slice::from_raw_parts_mut(self.buffer, self.buffer_len);
                    D::set_pixel(&mut whole_buffer[index], p);
                }
            }
            Ok(())
        })
    }

    // Make sure to clear the partition. The default clear method uses self.bounding_box()
    // which assumes the display has top_left (0,0)
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        // TODO can we avoid this clone?
        self.fill_solid(&self.partition.clone(), color).await
    }
}
