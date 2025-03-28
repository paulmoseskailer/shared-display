use embedded_graphics::{geometry::Size, primitives::Rectangle};

use crate::sharable_display::{DisplayPartition, SharableBufferedDisplay};

const MAX_PARTITIONS: usize = 4;

pub struct SharedDisplay {
    // keep track of partition areas
    partitions: heapless::Vec<Rectangle, MAX_PARTITIONS>,
}

impl SharedDisplay {
    pub async fn new() -> Self {
        SharedDisplay {
            partitions: heapless::Vec::new(),
        }
    }

    pub fn new_partition<B, D>(
        &mut self,
        display: &mut D,
        area: Rectangle,
    ) -> Option<DisplayPartition<B, D>>
    where
        D: SharableBufferedDisplay<BufferElement = B>,
    {
        // check area inside display
        let bb = display.bounding_box();
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

        Some(display.new_partition(area))
    }
}
