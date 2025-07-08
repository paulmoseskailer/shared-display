use core::convert::Infallible;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use shared_display_core::{
    DisplayPartition, MAX_APPS_PER_SCREEN, NewPartitionError, SharableBufferedDisplay,
};

const DISP_WIDTH: usize = 16;
const DISP_HEIGHT: usize = 2;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT;

const PRINT_FLUSH: bool = false;
static FLUSH_REQUESTS: Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN> = Channel::new();

struct FakeDisplay {
    buffer: [u8; NUM_PIXELS],
}

impl FakeDisplay {
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
    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        (point.y * parent_size.width as i32 + point.x)
            .try_into()
            .unwrap()
    }
    fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement {
        match color {
            BinaryColor::On => 1,
            BinaryColor::Off => 0,
        }
    }
}

#[tokio::test]
async fn simple_split_clear() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay { buffer };
    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    let parent_size = d.bounding_box().size;
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let mut left_display = DisplayPartition::<FakeDisplay>::new(
        0,
        d.get_buffer(),
        parent_size,
        left_area,
        &FLUSH_REQUESTS,
    )?;
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let mut right_display = DisplayPartition::<FakeDisplay>::new(
        1,
        d.get_buffer(),
        parent_size,
        right_area,
        &FLUSH_REQUESTS,
    )?;

    left_display.clear(BinaryColor::Off).await.unwrap();
    let expected = string_to_buffer(String::from("00000000 11111111 00000000 11111111"));
    assert_eq!(expected, *d.flush());

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    right_display.clear(BinaryColor::Off).await.unwrap();
    let expected = string_to_buffer(String::from("11111111 00000000 11111111 00000000"));
    assert_eq!(expected, *d.flush());

    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay { buffer };
    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    let parent_size = d.bounding_box().size;
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let mut left_display = DisplayPartition::<FakeDisplay>::new(
        0,
        d.get_buffer(),
        parent_size,
        left_area,
        &FLUSH_REQUESTS,
    )?;
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let mut right_display = DisplayPartition::<FakeDisplay>::new(
        1,
        d.get_buffer(),
        parent_size,
        right_area,
        &FLUSH_REQUESTS,
    )?;

    let rect = Rectangle::new(Point::new(0, 0), Size::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("00000000 11000000 00000000 11000000"));
    assert_eq!(expected, *d.flush());

    let rect = Rectangle::new(Point::new(0, 0), Size::new(5, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut left_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("11111000 11000000 11111000 11000000"));
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
