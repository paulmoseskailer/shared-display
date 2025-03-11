use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::PixelColor,
    prelude::{OriginDimensions, Size},
    Pixel,
};

pub struct DisplayPartition<'a, D: ?Sized, C, E> {
    pub buffer: &'a mut [u8],
    parent: &'a D,
    _color: core::marker::PhantomData<C>,
    _error: core::marker::PhantomData<E>,
}

pub trait SharableBufferedDisplay: DrawTarget {
    fn split_display_buffer(
        &mut self, /* add option to split vertically here */
    ) -> (
        DisplayPartition<Self, Self::Color, Self::Error>,
        DisplayPartition<Self, Self::Color, Self::Error>,
    );

    async fn draw_iter_to_buffer<I>(
        &self,
        partition: &mut DisplayPartition<Self, Self::Color, Self::Error>,
        pixels: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>;
}

impl<D, C, E> OriginDimensions for DisplayPartition<'_, D, C, E>
where
    D: SharableBufferedDisplay<Color = C, Error = E>,
    C: PixelColor,
{
    fn size(&self) -> Size {
        Size::new(0, 0)
    }
}

impl<D, C, E> DrawTarget for DisplayPartition<'_, D, C, E>
where
    D: SharableBufferedDisplay<Color = C, Error = E>,
    C: PixelColor,
{
    type Color = C;
    type Error = E;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), E>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.parent.draw_iter_to_buffer(self, pixels).await
    }
}
