use embedded_graphics::{prelude::*, primitives::Rectangle};

use crate::sharable_display::{DisplayPartition, SharableBufferedDisplay};

const MAX_PARTITIONS: usize = 4;

pub struct SharedDisplay {
    partitions: heapless::Vec<(Rectangle, usize, usize), MAX_PARTITIONS>,
}

impl SharedDisplay {
    pub async fn new() -> Self {
        println!("new called!, max partitions {}", MAX_PARTITIONS);
        SharedDisplay {
            partitions: heapless::Vec::new(),
        }
    }

    pub async fn new_partition<B, D>(
        &mut self,
        display: &mut D,
        area: Rectangle,
    ) -> Option<DisplayPartition<B, D>>
    where
        D: SharableBufferedDisplay<BufferElement = B>,
    {
        let bb = display.bounding_box();
        if !(bb.contains(area.top_left)
            && bb.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return None;
        }

        //TODO check no overlap with other partitions

        self.partitions.push((area.clone(), 0, 0)).unwrap();

        Some(display.new_partition(area))
    }
}
