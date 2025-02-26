//use embassy_sync::blocking_mutex::raw::NoopRawMutex;
//use embassy_sync::mutex::Mutex;
use embedded_graphics::{
    draw_target::{DrawTarget, DrawTargetExt},
    geometry::{OriginDimensions, Point},
    prelude::{PixelColor, Size},
    primitives::Rectangle,
    Pixel,
};
use std::boxed::Box;
use std::sync::Mutex;

pub struct SharedDisplay<D: DrawTarget + OriginDimensions + 'static> {
    display_ref: &'static Mutex<Option<Box<D>>>,
    area: Rectangle,
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + OriginDimensions + 'static>
    SharedDisplay<D>
{
    pub fn from_rectangle(display: &'static Mutex<Option<Box<D>>>, rect: Rectangle) -> Self {
        SharedDisplay {
            display_ref: display,
            area: rect,
        }
    }
}

impl<D: DrawTarget + OriginDimensions> OriginDimensions for SharedDisplay<D> {
    fn size(&self) -> Size {
        self.area.size
    }
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + OriginDimensions> DrawTarget
    for SharedDisplay<D>
{
    type Color = C;
    type Error = E;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let mut r = self.display_ref.lock().unwrap();
        r.as_mut()
            .unwrap()
            .clipped(&self.area)
            .draw_iter(pixels)
            .await
    }

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let mut r = self.display_ref.lock().unwrap();
        r.as_mut()
            .unwrap()
            .clipped(&self.area)
            .fill_solid(&self.area, color)
            .await
    }
}

pub async fn split_vertically<D>(
    display: &'static Mutex<Option<Box<D>>>,
) -> (SharedDisplay<D>, SharedDisplay<D>)
where
    D: DrawTarget + OriginDimensions,
{
    let (top_left, size) = {
        let r = display.lock().unwrap();
        let bounding_box = r.as_ref().unwrap().bounding_box();
        (bounding_box.top_left, bounding_box.size)
    };
    let split_size = Size {
        width: size.width / 2,
        height: size.height,
    };
    (
        SharedDisplay::from_rectangle(display, Rectangle::new(top_left, split_size)),
        SharedDisplay::from_rectangle(
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
