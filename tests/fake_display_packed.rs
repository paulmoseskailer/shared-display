use core::convert::Infallible;
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use shared_display_core::{PartitioningError, SharableBufferedDisplay};

const DISP_WIDTH: usize = 16;
const DISP_HEIGHT: usize = 2;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT;
const BUFFER_LEN: usize = NUM_PIXELS / 8;

const PRINT_FLUSH: bool = false;

struct FakePackedDisplay {
    buffer: [u8; BUFFER_LEN],
}

impl FakePackedDisplay {
    fn flush(&mut self) -> &[u8; BUFFER_LEN] {
        if PRINT_FLUSH {
            for byte in self.buffer {
                println!("{:#010b}", byte);
            }
        }
        &self.buffer
    }
}

impl OriginDimensions for FakePackedDisplay {
    fn size(&self) -> Size {
        Size::new(
            DISP_WIDTH.try_into().unwrap(),
            DISP_HEIGHT.try_into().unwrap(),
        )
    }
}

impl DrawTarget for FakePackedDisplay {
    type Color = BinaryColor;
    type Error = Infallible;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels.into_iter().for_each(|pixel| {
            let pos = pixel.0;
            assert!(pos.x < DISP_WIDTH as i32);
            assert!(pos.y < DISP_HEIGHT as i32);
            let buffer_index = FakePackedDisplay::calculate_buffer_index(pos, self.size());
            assert!(buffer_index < BUFFER_LEN);
            FakePackedDisplay::set_pixel(&mut self.buffer[buffer_index], pixel);
        });
        Ok(())
    }
}

impl SharableBufferedDisplay for FakePackedDisplay {
    type BufferElement = u8;
    fn get_buffer(&mut self) -> &mut [Self::BufferElement] {
        self.buffer.as_mut()
    }
    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        ((point.y * parent_size.width as i32 + point.x) / 8)
            .try_into()
            .unwrap()
    }
    fn set_pixel(buffer: &mut Self::BufferElement, pixel: Pixel<Self::Color>) {
        let value = pixel.1.is_on() as u8;
        let bit_be = pixel.0.x % 8;
        let bit = 7 - bit_be;

        // Set pixel value in byte
        // Ref this comment https://stackoverflow.com/questions/47981/how-do-you-set-clear-and-toggle-a-single-bit#comment46654671_47990
        *buffer = *buffer & !(1 << bit) | (value << bit);
    }
}

#[tokio::test]
async fn simple_split_clear() -> Result<(), PartitioningError> {
    let buffer = [0; BUFFER_LEN];
    let mut d = FakePackedDisplay { buffer };
    assert_eq!(*d.flush(), [0; BUFFER_LEN]);

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(), [255, 255, 255, 255]);

    let (mut left_display, mut right_display) = d.split_buffer_vertically()?;

    left_display.clear(BinaryColor::Off).await.unwrap();
    assert_eq!(*d.flush(), [0, 255, 0, 255]);

    left_display.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(), [255, 255, 255, 255]);

    right_display.clear(BinaryColor::Off).await.unwrap();
    assert_eq!(*d.flush(), [255, 0, 255, 0]);

    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), PartitioningError> {
    let buffer = [0; BUFFER_LEN];
    let mut d = FakePackedDisplay { buffer };
    assert_eq!(*d.flush(), [0; BUFFER_LEN]);

    let (mut left_display, mut right_display) = d.split_buffer_vertically()?;

    let rect = Rectangle::new(Point::new(0, 0), Size::new(4, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut left_display)
        .await
        .unwrap();
    assert_eq!(*d.flush(), [0b11110000, 0, 0b11110000, 0]);

    let rect = Rectangle::new(Point::new(0, 0), Size::new(5, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await
        .unwrap();
    assert_eq!(*d.flush(), [0b11110000, 0b11111000, 0b11110000, 0b11111000]);

    Ok(())
}
