use embedded_graphics::{
    geometry::{Point, Size},
    primitives::Rectangle,
};

use crate::sharable_display::{DisplayPartition, SharableBufferedDisplay};

const MAX_PARTITIONS: usize = 6;

pub struct SharedDisplay<D: SharableBufferedDisplay> {
    pub real_display: D,
    // keep track of partition areas
    partitions: heapless::Vec<Rectangle, MAX_PARTITIONS>,
}

impl<B, D> SharedDisplay<D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    pub async fn new(real_display: D) -> Self {
        SharedDisplay {
            real_display,
            partitions: heapless::Vec::new(),
        }
    }

    pub fn new_partition(&mut self, area: Rectangle) -> Option<DisplayPartition<B, D>> {
        // check area inside display
        let bb = self.real_display.bounding_box();
        if !(bb.contains(area.top_left)
            && bb.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return None;
        }

        // check area not overlapping with existing partitions
        for p in self.partitions.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return None;
            }
        }

        self.partitions.push(area.clone()).unwrap();

        Some(self.real_display.new_partition(area))
    }

    pub fn split_vertically(&mut self) -> Option<(DisplayPartition<B, D>, DisplayPartition<B, D>)> {
        // TODO split_buffer_vertically should return Result
        let (left_part, right_part) = self.real_display.split_buffer_vertically();

        self.partitions.push(left_part.partition).unwrap();
        self.partitions.push(right_part.partition).unwrap();

        Some((left_part, right_part))
    }

    /// Re-splits an existing partition. Fails if the partition doesn't exist but does not remove
    /// the current partition's screen access
    pub fn split_existing_unchecked(
        &mut self,
        existing_area: Rectangle,
    ) -> Option<(DisplayPartition<B, D>, DisplayPartition<B, D>)> {
        let mut maybe_i = None;
        for (i, p) in self.partitions.iter().enumerate() {
            if *p == existing_area {
                maybe_i = Some(i);
            }
        }
        let existing_i = maybe_i?;

        self.partitions.remove(existing_i);

        let half_width = existing_area.size.width / 2;

        let left_part = self.new_partition(Rectangle::new(
            existing_area.top_left,
            Size::new(half_width, existing_area.size.height),
        ))?;
        let right_part = self.new_partition(Rectangle::new(
            existing_area.top_left + Point::new(half_width as i32, 0),
            Size::new(half_width, existing_area.size.height),
        ))?;

        Some((left_part, right_part))
    }
}
