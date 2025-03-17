use core::convert::Infallible;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    Pixel,
};
use shared_display::sharable_display::SharableBufferedDisplay;

const DISP_WIDTH: usize = 12;
const DISP_HEIGHT: usize = 2;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT;

const PRINT_FLUSH: bool = false;

struct FakeDisplay {
    buffer: [u8; NUM_PIXELS],
}

impl FakeDisplay {
    fn new(buffer: [u8; NUM_PIXELS]) -> Self {
        FakeDisplay { buffer }
    }

    fn flush(&mut self) -> &[u8; NUM_PIXELS] {
        if PRINT_FLUSH {
            for row in 0..DISP_HEIGHT {
                let row_start: usize = row * DISP_WIDTH;
                for i in 0..DISP_WIDTH {
                    print!("{}", self.buffer[row_start + i]);
                }
                println!("");
            }
        }
        &self.buffer
    }
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
    fn calculate_buffer_index(point: Point, display_width: usize) -> usize {
        (point.y * display_width as i32 + point.x)
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
async fn simple_split_clear() -> Result<(), Infallible> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::new(buffer);
    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    d.clear(BinaryColor::On).await?;
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    let (mut left_display, mut right_display) = d.split_buffer_vertically();

    left_display.clear(BinaryColor::Off).await?;
    let expected = string_to_buffer(String::from("00000000 1111 00000000 1111"));
    assert_eq!(expected, *d.flush());

    d.clear(BinaryColor::On).await?;
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    right_display.clear(BinaryColor::Off).await?;
    let expected = string_to_buffer(String::from("11111111 0000 11111111 0000"));
    assert_eq!(expected, *d.flush());

    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), Infallible> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::new(buffer);
    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    let (mut left_display, mut right_display) = d.split_buffer_vertically();

    let rect = Rectangle::new(Point::new(0, 0), Size::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await?;
    let expected = string_to_buffer(String::from("00000000 1100 00000000 1100"));
    assert_eq!(expected, *d.flush());

    let rect = Rectangle::new(Point::new(0, 0), Size::new(5, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut left_display)
        .await?;
    let expected = string_to_buffer(String::from("11111000 1100 11111000 1100"));
    assert_eq!(expected, *d.flush());

    Ok(())
}

fn string_to_buffer(s: String) -> Vec<u8> {
    s.chars()
        .filter(|&c| c == '0' || c == '1')
        .map(|c| match c {
            '0' => 0,
            '1' => 1,
            _ => panic!(),
        })
        .collect()
}
