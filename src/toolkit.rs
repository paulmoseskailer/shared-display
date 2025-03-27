use std::borrow::BorrowMut;

use embedded_graphics::primitives::Rectangle;

use crate::sharable_display::{DisplayPartition, SharableBufferedDisplay};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

pub struct SharedDisplay<B, D, const MAX_PARTITIONS: usize>
where
    D: SharableBufferedDisplay,
{
    partitions: heapless::Vec<DisplayPartition<B, D>, MAX_PARTITIONS>,
}

impl<B, D, const MAX_PARTITIONS: usize> SharedDisplay<B, D, MAX_PARTITIONS>
where
    D: SharableBufferedDisplay,
{
    // Save area and dirty section for every partition
    const PARTITIONS: Mutex<
        CriticalSectionRawMutex,
        heapless::Vec<(Rectangle, u8, u8), MAX_PARTITIONS>,
    > = Mutex::new(heapless::Vec::new());

    pub async fn init() {
        println!("init called!, max partitions {}", MAX_PARTITIONS);
    }

    pub async fn new_partition(display: &mut D, area: Rectangle) -> Option<DisplayPartition<B, D>>
    where
        D: SharableBufferedDisplay<BufferElement = B>,
    {
        //TODO check no overlap
        Self::PARTITIONS
            .lock()
            .await
            .borrow_mut()
            .push((area, 0, 0))
            .unwrap();
        Some(display.new_partition(area))
    }
}
