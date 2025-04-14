use core::convert::Infallible;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use shared_display_core::{DrawTracker, PartitioningError, SharableBufferedDisplay};

const DISP_WIDTH: usize = 16;
const DISP_HEIGHT: usize = 4;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT;

static DRAW_TRACKERS: [DrawTracker; 2] = [DrawTracker::new(), DrawTracker::new()];

struct FakeDisplay {
    buffer: [u8; NUM_PIXELS],
}

impl OriginDimensions for FakeDisplay {
    fn size(&self) -> Size {
        Size::new(
            DISP_WIDTH.try_into().unwrap(),
            DISP_HEIGHT.try_into().unwrap(),
        )
    }
}

impl DrawTarget for FakeDisplay {
    type Color = BinaryColor;
    type Error = Infallible;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels.into_iter().for_each(|Pixel(pos, color)| {
            assert!(pos.x < DISP_WIDTH as i32);
            let pixel_index: usize = (pos.y * DISP_WIDTH as i32 + pos.x).try_into().unwrap();
            assert!(pixel_index < NUM_PIXELS);
            self.buffer[pixel_index] = match color {
                BinaryColor::On => 1,
                BinaryColor::Off => 0,
            };
        });
        Ok(())
    }
}

impl SharableBufferedDisplay for FakeDisplay {
    type BufferElement = u8;
    fn get_buffer(&mut self) -> &mut [Self::BufferElement] {
        self.buffer.as_mut()
    }
    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        (point.y * parent_size.width as i32 + point.x)
            .try_into()
            .unwrap()
    }
    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>) {
        *buffer = match pixel.1 {
            BinaryColor::On => 1,
            BinaryColor::Off => 0,
        };
    }
}

#[tokio::test]
async fn flush_dirty_only() -> Result<(), PartitioningError> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay { buffer };

    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let mut left_display = d.new_partition(left_area, &DRAW_TRACKERS[0]).unwrap();

    let right_partition_origin = Point::new(8, 0);
    let right_area = Rectangle::new(right_partition_origin, Size::new(8, 2));
    let mut right_display = d.new_partition(right_area, &DRAW_TRACKERS[1]).unwrap();

    left_display.clear(BinaryColor::Off).await.unwrap();
    assert_eq!(
        DRAW_TRACKERS[0].take_dirty_area().await.unwrap(),
        Rectangle::new_at_origin(Size::new(8, 2))
    );
    assert_eq!(DRAW_TRACKERS[0].take_dirty_area().await, None);

    let rect = Rectangle::new(Point::new(0, 0), Size::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await
        .unwrap();
    assert_eq!(
        DRAW_TRACKERS[1].take_dirty_area().await.unwrap(),
        Rectangle::new(right_partition_origin, Size::new(2, 2))
    );
    assert_eq!(DRAW_TRACKERS[1].take_dirty_area().await, None);

    Ok(())
}
