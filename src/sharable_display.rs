use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{Dimensions, OriginDimensions, PixelColor, Size},
    primitives::Rectangle,
    Pixel,
};

pub struct DisplayPartition<B, D: ?Sized> {
    pub buffer: *mut B,
    buffer_len: usize,
    pub parent_size: Size,
    pub partition: Rectangle,
    _display: core::marker::PhantomData<D>,
}

impl<C, B, D> DisplayPartition<B, D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
{
    pub fn new(
        buffer: &mut [B],
        parent_size: Size,
        partition: Rectangle,
    ) -> DisplayPartition<B, D> {
        DisplayPartition {
            buffer: buffer.as_mut_ptr(),
            parent_size,
            buffer_len: buffer.len(),
            partition,
            _display: core::marker::PhantomData,
        }
    }

    fn contains(&self, p: Point) -> bool {
        self.partition.contains(p)
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement;

    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize;

    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>);

    fn split_buffer_vertically(
        &mut self,
    ) -> (
        DisplayPartition<Self::BufferElement, Self>,
        DisplayPartition<Self::BufferElement, Self>,
    ) {
        let parent_size = self.bounding_box().size;
        assert!(
            parent_size.width > 8,
            "Error: can't split a display that's only 8 pixels wide"
        );

        let buffer_len = self.get_buffer().len();
        let pixels_per_buffer_el = (parent_size.width * parent_size.height) as usize / buffer_len;
        if pixels_per_buffer_el > 1 {
            assert_eq!(
                parent_size.width % pixels_per_buffer_el as u32,
                0,
                "A buffer element would have to span multiple rows! Have {} pixels per buffer element and display width {} pixels. Adjust screen size or buffer element type!",
                pixels_per_buffer_el, parent_size.width
            );
        }

        // ensure no bytes are split in half by rounding to a split of width multiple of 8
        let left_partition_width = (parent_size.width / 2) + 7 & !7;
        let left_partition = Rectangle::new(
            self.bounding_box().top_left,
            Size::new(left_partition_width, parent_size.height),
        );
        let right_partition = Rectangle::new(
            self.bounding_box().top_left + Point::new(left_partition_width.try_into().unwrap(), 0),
            Size::new(parent_size.width - left_partition_width, parent_size.height),
        );
        (
            DisplayPartition::new(self.get_buffer(), parent_size, left_partition),
            DisplayPartition::new(self.get_buffer(), parent_size, right_partition),
        )
    }
}

impl<B, D> Dimensions for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    fn bounding_box(&self) -> Rectangle {
        self.partition
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
                let buffer_index = D::calculate_buffer_index(p.0, self.parent_size);
                if self.contains(p.0) {
                    D::set_pixel(&mut whole_buffer[buffer_index], p);
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

impl<B, D> SharableBufferedDisplay for DisplayPartition<B, D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    type BufferElement = B;
    fn get_buffer(&mut self) -> &mut [Self::BufferElement] {
        unsafe { core::slice::from_raw_parts_mut(self.buffer, self.buffer_len) }
    }

    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>) {
        D::set_pixel(buffer, pixel)
    }

    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        D::calculate_buffer_index(point, parent_size)
    }
}
