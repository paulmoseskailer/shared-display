use core::convert::Infallible;
use embedded_graphics::{
    draw_target::DrawTarget, geometry::Point, pixelcolor::BinaryColor, prelude::OriginDimensions,
    prelude::Size, Pixel,
};
use shared_display::sharable_display::{DisplayPartition, SharableBufferedDisplay};

struct FakeDisplay {
    buffer: [u8; 4],
}

impl FakeDisplay {
    fn new(buffer: [u8; 4]) -> Self {
        FakeDisplay { buffer }
    }

    fn flush(&mut self) -> &[u8; 4] {
        println!(
            "{}{}{}{}",
            self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3]
        );
        &self.buffer
    }
}

impl OriginDimensions for FakeDisplay {
    fn size(&self) -> Size {
        Size::new(4, 1)
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
            self.buffer[(pos.x % 4) as usize] = match color {
                BinaryColor::On => 1,
                BinaryColor::Off => 0,
            };
        });
        Ok(())
    }
}

impl SharableBufferedDisplay for FakeDisplay {
    fn split_display_buffer(
        &mut self, /* add option to split vertically here */
    ) -> (DisplayPartition<Self>, DisplayPartition<Self>) {
        let (l, r) = self.buffer.split_at_mut(2);
        (DisplayPartition::new(l), DisplayPartition::new(r))
    }

    fn set_pixel(
        partition: &mut DisplayPartition<Self>,
        pixel: Pixel<Self::Color>,
    ) -> Result<(), Self::Error> {
        let (pos, color) = (pixel.0, pixel.1);
        let b = partition.buffer.as_mut();
        b[(pos.x % 4) as usize] = match color {
            BinaryColor::Off => 0,
            BinaryColor::On => 1,
        };
        Ok(())
    }
}

#[tokio::test]
async fn simple_split() {
    let buffer = [0, 0, 0, 0];
    let mut d = FakeDisplay::new(buffer);

    assert_eq!(*d.flush(), [0, 0, 0, 0]);

    println!("drawing 1011 on original display:");
    let pixels = get_n_pixels(4, &[1, 1, 1, 1]);
    let _ = d.draw_iter(pixels).await;

    assert_eq!(*d.flush(), [1, 1, 1, 1]);

    println!("splitting display");
    let mut ld: DisplayPartition<FakeDisplay>;
    let mut rd: DisplayPartition<FakeDisplay>;
    (ld, rd) = d.split_display_buffer();

    println!("drawing 10 left and 01 right (draw_iter)");
    let left_pixels = get_n_pixels(2, &[1, 0]);
    let right_pixels = get_n_pixels(2, &[0, 1]);

    let _ = ld.draw_iter(left_pixels).await;
    let _ = rd.draw_iter(right_pixels).await;

    assert_eq!(*d.flush(), [1, 0, 0, 1]);
}

fn get_n_pixels(n: u8, values: &[u8]) -> Vec<Pixel<BinaryColor>> {
    assert_eq!(values.len(), n as usize);
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Pixel(
                Point::new(i as i32, 0),
                match v {
                    0 => BinaryColor::Off,
                    _ => BinaryColor::On,
                },
            )
        })
        .collect()
}
