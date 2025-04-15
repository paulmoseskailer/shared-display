use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use embassy_sync::mutex::Mutex;

use embedded_graphics::{
    Pixel,
    draw_target::{DrawTarget, DrawTargetExt},
    geometry::{OriginDimensions, Point},
    prelude::{PixelColor, Size},
    primitives::Rectangle,
};

pub struct SharedDisplayReference<D: DrawTarget + OriginDimensions + 'static> {
    display_ref: &'static Mutex<CriticalSectionRawMutex, Option<D>>,
    area: Rectangle,
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + OriginDimensions + 'static>
    SharedDisplayReference<D>
{
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

impl<D: DrawTarget + OriginDimensions> OriginDimensions for SharedDisplayReference<D> {
    fn size(&self) -> Size {
        self.area.size
    }
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + OriginDimensions> DrawTarget
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
        disp.clipped(&self.area).draw_iter(pixels).await
    }

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let mut guard = self.display_ref.lock().await;
        let disp = guard.as_mut().unwrap();
        disp.clipped(&self.area).fill_solid(&self.area, color).await
    }
}

pub async fn split_vertically<D>(
    display: &'static Mutex<CriticalSectionRawMutex, Option<D>>,
) -> (SharedDisplayReference<D>, SharedDisplayReference<D>)
where
    D: DrawTarget + OriginDimensions,
{
    let (top_left, size) = {
        let guard = display.lock().await;
        let disp = guard.as_ref().unwrap();
        let bounding_box = disp.bounding_box();
        (bounding_box.top_left, bounding_box.size)
    };

    let split_size = Size {
        width: size.width / 2,
        height: size.height,
    };

    (
        SharedDisplayReference::from_rectangle(display, Rectangle::new(top_left, split_size)),
        SharedDisplayReference::from_rectangle(
            display,
            Rectangle::new(
                Point {
                    x: top_left.x + size.width as i32 / 2,
                    y: top_left.y,
                },
                split_size,
            ),
        ),
    )
}
