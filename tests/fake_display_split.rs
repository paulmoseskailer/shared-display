use core::convert::Infallible;
use embedded_graphics::{
    draw_target::DrawTarget, geometry::Point, pixelcolor::BinaryColor, prelude::OriginDimensions,
    prelude::Size, primitives::Rectangle, Pixel,
};
use shared_display::sharable_display::{DisplayPartition, SharableBufferedDisplay};

const NUM_PIXELS: usize = 8;

/// Assumes width of 4 pixels
struct FakeDisplay {
    buffer: [u8; NUM_PIXELS],
}

impl FakeDisplay {
    fn new(buffer: [u8; NUM_PIXELS]) -> Self {
        assert_eq!(NUM_PIXELS % 4, 0);
        FakeDisplay { buffer }
    }

    fn flush(&mut self) -> &[u8; NUM_PIXELS] {
        let num_rows = (NUM_PIXELS as u32).div_ceil(4);
        for row in 0..num_rows {
            let offset: usize = (row * 4).try_into().unwrap();
            println!(
                "{}{}{}{}",
                self.buffer[offset + 0],
                self.buffer[offset + 1],
                self.buffer[offset + 2],
                self.buffer[offset + 3]
            );
        }
        &self.buffer
    }
}

impl OriginDimensions for FakeDisplay {
    fn size(&self) -> Size {
        assert_eq!(NUM_PIXELS % 4, 0);
        Size::new(4, (NUM_PIXELS / 4).try_into().unwrap())
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
            assert!(pos.x < 4);
            let pixel_index: usize = (pos.y * 4 + pos.x).try_into().unwrap();
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
    type BufferType = u8;
    fn split_display_buffer(
        &mut self, /* add option to split vertically here */
    ) -> (DisplayPartition<Self, u8>, DisplayPartition<Self, u8>) {
        (
            DisplayPartition::new(
                &mut self.buffer,
                Rectangle::new(Point::new(0, 0), Size::new(2, 2)),
            ),
            DisplayPartition::new(
                &mut self.buffer,
                Rectangle::new(Point::new(2, 0), Size::new(2, 2)),
            ),
        )
    }

    fn get_pixel_value(pixel: Pixel<Self::Color>) -> Self::BufferType {
        match pixel.1 {
            BinaryColor::Off => 0,
            BinaryColor::On => 1,
        }
    }
}

#[tokio::test]
async fn simple_split_clear() -> Result<(), Infallible> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::new(buffer);

    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    d.clear(BinaryColor::On).await?;

    assert_eq!(*d.flush(), [1; NUM_PIXELS]);
    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), Infallible> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::new(buffer);

    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    let ones = get_n_pixels(String::from("11111111"));
    d.draw_iter(ones).await?;
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    let zeros = get_n_pixels(String::from("00000000"));
    d.draw_iter(zeros).await?;
    assert_eq!(*d.flush(), [0; NUM_PIXELS]);

    let mut ld: DisplayPartition<FakeDisplay, u8>;
    let mut rd: DisplayPartition<FakeDisplay, u8>;
    (ld, rd) = d.split_display_buffer();

    let ones = get_n_pixels(String::from("11111111"));
    ld.draw_iter(ones).await?;
    assert_eq!(*d.flush(), [1, 1, 0, 0, 1, 1, 0, 0]);

    let ones = get_n_pixels(String::from("11111111"));
    d.draw_iter(ones).await?;
    assert_eq!(*d.flush(), [1; NUM_PIXELS]);

    let zeros = get_n_pixels(String::from("00000000"));
    rd.draw_iter(zeros).await?;
    assert_eq!(*d.flush(), [1, 1, 0, 0, 1, 1, 0, 0]);
    Ok(())
}

fn get_n_pixels(s: String) -> Vec<Pixel<BinaryColor>> {
    s.chars()
        .enumerate()
        .map(|(i, v)| {
            let x = i % 4;
            let y = i / 4;
            Pixel(
                Point::new(x.try_into().unwrap(), y.try_into().unwrap()),
                match v {
                    '0' => BinaryColor::Off,
                    _ => BinaryColor::On,
                },
            )
        })
        .collect()
}
