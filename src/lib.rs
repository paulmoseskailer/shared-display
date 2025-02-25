use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_graphics::{
    draw_target::{DrawTarget, DrawTargetExt},
    geometry::{OriginDimensions, Point},
    prelude::{PixelColor, Size},
    primitives::Rectangle,
    Pixel,
};
use std::sync::{Arc, Weak};

pub struct SharedDisplay<D: DrawTarget + OriginDimensions> {
    display_ref: Weak<Mutex<NoopRawMutex, D>>,
    area: Rectangle,
}

pub enum DisplayAlive {
    Yes,
    No,
}

impl<C: PixelColor, E, D: DrawTarget<Color = C, Error = E> + OriginDimensions> SharedDisplay<D> {
    pub fn from_rectangle(display: Weak<Mutex<NoopRawMutex, D>>, rect: Rectangle) -> Self {
        SharedDisplay {
            display_ref: display,
            area: rect,
        }
    }

    pub fn is_alive(&self) -> DisplayAlive {
        match self.display_ref.upgrade() {
            Some(_) => DisplayAlive::Yes,
            None => DisplayAlive::No,
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
        if let Some(display_ref) = self.display_ref.upgrade() {
            display_ref
                .lock()
                .await
                .clipped(&self.area)
                .draw_iter(pixels)
                .await
        } else {
            // No way to know Self::Error, just ignore the call
            Ok(())
        }
    }

    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        if let Some(display_ref) = self.display_ref.upgrade() {
            display_ref
                .lock()
                .await
                .clipped(&self.area)
                .fill_solid(&self.area, color)
                .await
        } else {
            // No way to know Self::Error, just ignore the call
            Ok(())
        }
    }
}

pub async fn split_vertically<D>(
    display: Arc<Mutex<NoopRawMutex, D>>,
) -> (SharedDisplay<D>, SharedDisplay<D>)
where
    D: DrawTarget + OriginDimensions,
{
    let (top_left, size) = {
        let bounding_box = display.lock().await.bounding_box();
        (bounding_box.top_left, bounding_box.size)
    };
    let split_size = Size {
        width: size.width / 2,
        height: size.height,
    };
    (
        SharedDisplay::from_rectangle(
            Arc::downgrade(&display),
            Rectangle::new(top_left, split_size),
        ),
        SharedDisplay::from_rectangle(
            Arc::downgrade(&display),
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
