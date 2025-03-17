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
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
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
        let x: i32 = (i as usize % self.display_width).try_into().unwrap();
        let y: i32 = (i as usize / self.display_width).try_into().unwrap();
        // check if the x-coordinate of the index is within the owned partition
        // TODO adapt
        self.contains(Point::new(x, y))
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    type BufferElement;

    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    fn calculate_buffer_index(point: Point, display_width: usize) -> usize;

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

        // ensure no bytes are split in half by rounding to a split of width multiple of 8
        let left_partition_width = (parent_size.width / 2) + 7 & !7;
        let left_partition = Rectangle::new(
            Point::new(0, 0),
            Size::new(left_partition_width, parent_size.height),
        );
        let right_partition = Rectangle::new(
            Point::new(left_partition_width.try_into().unwrap(), 0),
            Size::new(parent_size.width - left_partition_width, parent_size.height),
        );
        (
            DisplayPartition::new(
                self.get_buffer(),
                parent_size.width as usize,
                left_partition,
            ),
            DisplayPartition::new(
                self.get_buffer(),
                parent_size.width as usize,
                right_partition,
            ),
        )
    }
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
                let index = D::calculate_buffer_index(p.0, self.display_width);
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
