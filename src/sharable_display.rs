use embedded_graphics::{
    draw_target::DrawTarget,
    prelude::{OriginDimensions, Size},
    Pixel,
};

pub struct DisplayPartition<'a, D: ?Sized> {
    pub buffer: &'a mut [u8],
    _display: core::marker::PhantomData<D>,
}

impl<D> DisplayPartition<'_, D>
where
    D: SharableBufferedDisplay,
{
    pub fn new(buffer: &mut [u8]) -> DisplayPartition<D> {
        DisplayPartition {
            buffer,
            _display: core::marker::PhantomData,
        }
    }
}

pub trait SharableBufferedDisplay: DrawTarget {
    fn split_display_buffer(
        &mut self, /* add option to split vertically here later */
    ) -> (DisplayPartition<Self>, DisplayPartition<Self>);

    fn set_pixel(
        partition: &mut DisplayPartition<Self>,
        pixel: Pixel<Self::Color>,
    ) -> Result<(), Self::Error>;
}

impl<D> OriginDimensions for DisplayPartition<'_, D>
where
    D: SharableBufferedDisplay,
{
    fn size(&self) -> Size {
        Size::new(0, 0)
    }
}

impl<D> DrawTarget for DisplayPartition<'_, D>
where
    D: SharableBufferedDisplay,
{
    type Color = D::Color;
    type Error = D::Error;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels.into_iter().try_for_each(|p| D::set_pixel(self, p))
    }
}
