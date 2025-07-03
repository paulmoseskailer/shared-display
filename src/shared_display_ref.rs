use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    Pixel,
    draw_target::{DrawTarget, DrawTargetExt},
    prelude::{Dimensions, PixelColor},
    primitives::Rectangle,
};

pub struct SharedDisplayReference<D: DrawTarget + 'static> {
    pub display_ref: &'static Mutex<CriticalSectionRawMutex, Option<D>>,
    area: Rectangle,
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + 'static> SharedDisplayReference<D> {
    #[allow(dead_code)]
    pub fn from_rectangle(
        display: &'static Mutex<CriticalSectionRawMutex, Option<D>>,

        rect: Rectangle,
    ) -> Self {
        SharedDisplayReference {
            display_ref: display,
            area: rect,
        }
    }
}

impl<D: DrawTarget> Dimensions for SharedDisplayReference<D> {
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E>> DrawTarget
    for SharedDisplayReference<D>
{
    type Color = C;

    type Error = E;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let mut guard = self.display_ref.lock().await;
        let disp = guard.as_mut().unwrap();
        disp.clipped(&self.area)
            .translated(self.area.top_left)
            .draw_iter(pixels)
            .await
    }

    async fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let mut guard = self.display_ref.lock().await;
        let disp = guard.as_mut().unwrap();
        disp.clipped(&self.area)
            .translated(self.area.top_left)
            .fill_contiguous(area, colors)
            .await
    }
    async fn fill_solid(
        &mut self,
        area: &Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let mut guard = self.display_ref.lock().await;
        let disp = guard.as_mut().unwrap();
        disp.clipped(&self.area)
            .translated(self.area.top_left)
            .fill_solid(area, color)
            .await
    }

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let mut guard = self.display_ref.lock().await;
        let disp = guard.as_mut().unwrap();
        disp.clipped(&self.area)
            .translated(self.area.top_left)
            .clear(color)
            .await
    }
}
