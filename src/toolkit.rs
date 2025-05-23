#![allow(async_fn_in_trait)]
extern crate alloc;
use alloc::boxed::Box;

use core::future::Future;
use core::pin::Pin;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_graphics::{
    geometry::{Point, Size},
    prelude::*,
    primitives::Rectangle,
};
use static_cell::StaticCell;

use shared_display_core::{
    AreaToFlush, DisplayPartition, DisplaySidePartitioningError, DrawTracker, FlushLock,
    MAX_APPS_PER_SCREEN, SharableBufferedDisplay,
    compressed::{CompressableDisplay, CompressedDisplayPartition},
};

const FLUSH_INTERVAL: Duration = Duration::from_millis(20);
const EVENT_QUEUE_SIZE: usize = MAX_APPS_PER_SCREEN;
pub static EVENTS: Channel<CriticalSectionRawMutex, ResizeEvent, EVENT_QUEUE_SIZE> = Channel::new();
static SPAWNER: StaticCell<Spawner> = StaticCell::new();
static DRAW_TRACKERS: [DrawTracker; MAX_APPS_PER_SCREEN] =
    [const { DrawTracker::new() }; MAX_APPS_PER_SCREEN];

#[derive(Debug)]
pub enum NewPartitionError {
    Overlaps,
    OutsideParent,
    DisplaySide(DisplaySidePartitioningError),
}

pub enum AppStart {
    Success,
    Failure,
}

#[derive(PartialEq, Eq)]
pub enum FlushResult {
    Continue,
    Abort,
}

pub enum ResizeEvent {
    AppClosed(Rectangle),
}

pub struct SharedDisplay<D: SharableBufferedDisplay> {
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    draw_trackers: &'static [DrawTracker; MAX_APPS_PER_SCREEN],

    spawner: &'static Spawner,
}

impl<B, D> SharedDisplay<D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    pub fn new(real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        SharedDisplay {
            real_display: Mutex::new(real_display),
            partition_areas: heapless::Vec::new(),
            draw_trackers: &DRAW_TRACKERS,
            spawner: spawner_ref,
        }
    }

    async fn new_partition(
        &mut self,
        area: Rectangle,
    ) -> Result<DisplayPartition<B, D>, NewPartitionError> {
        let real_display: &mut D = &mut *self.real_display.lock().await;

        // check area inside display
        let bb = real_display.bounding_box();
        if !(bb.contains(area.top_left)
            && bb.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return Err(NewPartitionError::OutsideParent);
        }

        // check area not overlapping with existing partition_areas
        for p in self.partition_areas.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return Err(NewPartitionError::Overlaps);
            }
        }

        let index = self.partition_areas.len();
        let result = real_display.new_partition(area, &self.draw_trackers[index]);

        if result.is_ok() {
            self.partition_areas.push(area.clone()).unwrap();
        }

        result.map_err(NewPartitionError::DisplaySide)
    }

    pub async fn partition_vertically(
        &mut self,
    ) -> Result<(DisplayPartition<B, D>, DisplayPartition<B, D>), NewPartitionError> {
        let total_area = self.real_display.lock().await.bounding_box();
        let half_size = Size::new(total_area.size.width / 2, total_area.size.height);
        let left_area = Rectangle::new(total_area.top_left, half_size);
        let right_area = Rectangle::new(
            Point::new(
                total_area.top_left.x + half_size.width as i32,
                total_area.top_left.y,
            ),
            half_size,
        );

        let left_partition = self.new_partition(left_area).await?;
        let right_partition = self.new_partition(right_area).await?;

        Ok((left_partition, right_partition))
    }

    pub async fn launch_new_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(DisplayPartition<B, D>) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition);
        self.spawner
            .must_spawn(launch_future(Box::pin(fut), area.clone()));

        Ok(())
    }

    pub async fn launch_new_recursive_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(DisplayPartition<B, D>, &'static Spawner) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition, self.spawner);
        self.spawner
            .must_spawn(launch_future(Box::pin(fut), area.clone()));

        Ok(())
    }

    pub async fn flush_loop<F>(&self, mut flush_area: F)
    where
        F: AsyncFnMut(&mut D, Rectangle) -> FlushResult,
    {
        'outer: loop {
            for (index, draw_tracker) in self.draw_trackers.iter().enumerate() {
                let area_to_flush = match draw_tracker.take_dirty_area().await {
                    AreaToFlush::None => None,
                    AreaToFlush::All => Some(self.partition_areas[index]),
                    AreaToFlush::Some(rect) => Some(rect),
                };
                if let Some(rect) = area_to_flush {
                    let result = flush_area(&mut *self.real_display.lock().await, rect).await;
                    if result == FlushResult::Abort {
                        break 'outer;
                    }
                }
            }
            Timer::after(FLUSH_INTERVAL).await;
        }
    }
}

#[embassy_executor::task(pool_size = MAX_APPS_PER_SCREEN)]
async fn launch_future(app_future: Pin<Box<dyn Future<Output = ()>>>, area: Rectangle) {
    app_future.await;

    EVENTS.send(ResizeEvent::AppClosed(area)).await;
}

pub async fn launch_app_in_app<F, B, D>(
    spawner: &'static Spawner,
    mut app_fn: F,
    partition: DisplayPartition<B, D>,
) -> AppStart
where
    F: AsyncFnMut(DisplayPartition<B, D>) -> (),
    for<'b> F::CallRefFuture<'b>: 'static,
{
    let area = partition.area.clone();
    let fut = app_fn(partition);
    spawner.must_spawn(launch_future(Box::pin(fut), area));

    return AppStart::Success;
}

/* ------------------- SHARED COMPRESSED DISPLAY ---------------------- */
pub struct SharedCompressedDisplay<D: CompressableDisplay> {
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    size: Size,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    buffer_pointers: heapless::Vec<*const Vec<(D::BufferElement, u8)>, MAX_APPS_PER_SCREEN>,

    spawner: &'static Spawner,
}

impl<D: CompressableDisplay> OriginDimensions for SharedCompressedDisplay<D> {
    fn size(&self) -> Size {
        self.size
    }
}

impl<D: CompressableDisplay> ContainsPoint for SharedCompressedDisplay<D> {
    fn contains(&self, point: Point) -> bool {
        self.bounding_box().contains(point)
    }
}

impl<B, D> SharedCompressedDisplay<D>
where
    D: CompressableDisplay<BufferElement = B>,
{
    pub fn new(mut real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        let size = real_display.bounding_box().size;
        real_display.drop_buffer();
        SharedCompressedDisplay {
            real_display: Mutex::new(real_display),
            size,
            partition_areas: heapless::Vec::new(),
            buffer_pointers: heapless::Vec::new(),
            spawner: spawner_ref,
        }
    }

    async fn new_partition(
        &mut self,
        area: Rectangle,
    ) -> Result<CompressedDisplayPartition<B, D>, NewPartitionError> {
        // check area inside display
        if !(self.contains(area.top_left)
            && self.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return Err(NewPartitionError::OutsideParent);
        }

        // check area not overlapping with existing partition_areas
        for p in self.partition_areas.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return Err(NewPartitionError::Overlaps);
            }
        }
        let partition = CompressedDisplayPartition::new(self.size, area)
            .map_err(NewPartitionError::DisplaySide)?;
        self.buffer_pointers
            .push(partition.get_ptr_to_buffer())
            .unwrap();

        self.partition_areas.push(area.clone()).unwrap();

        Ok(partition)
    }

    pub async fn launch_new_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(CompressedDisplayPartition<B, D>) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition);
        self.spawner
            .must_spawn(launch_future(Box::pin(fut), area.clone()));

        Ok(())
    }

    pub async fn flush_loop<F>(&self, mut flush_fn: F)
    where
        F: AsyncFnMut(&mut D, Vec<D::BufferElement>) -> FlushResult,
    {
        'outer: loop {
            if self.partition_areas.len() == 0 {
                Timer::after(FLUSH_INTERVAL).await;
                continue;
            }

            let decompressed_buffer = self.decompress_all_buffers();

            let flush_result = FlushLock::new()
                .protect_flush(async || {
                    flush_fn(&mut *self.real_display.lock().await, decompressed_buffer).await
                })
                .await;
            match flush_result {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break 'outer;
                }
            }

            Timer::after(FLUSH_INTERVAL).await;
        }
    }

    fn decompress_all_buffers(&self) -> Vec<B> {
        assert_eq!(self.partition_areas.len(), self.buffer_pointers.len());

        let mut area_to_flush = self.partition_areas[0];
        for area in self.partition_areas.iter().skip(1) {
            area_to_flush = area_to_flush.envelope(&area);
        }

        let mut decompressed_buffer: Vec<B> =
            vec![B::default(); (self.size.width * self.size.height) as usize];

        // decompress every partition's buffer
        for (i, &compressed_partition_ptr) in self.buffer_pointers.iter().enumerate() {
            let compressed_partition: &Vec<(B, u8)> = unsafe { &*compressed_partition_ptr };
            Self::decompress_into_buffer(
                compressed_partition,
                self.partition_areas[i],
                &mut decompressed_buffer,
                self.size,
            );
        }

        decompressed_buffer
    }

    fn decompress_into_buffer(
        compressed_partition: &Vec<(B, u8)>,
        partition_area: Rectangle,
        decompressed_buffer: &mut Vec<B>,
        total_size: Size,
    ) {
        let decompressed_len = compressed_partition
            .iter()
            .fold(0_u64, |before, (_color, run_len)| before + *run_len as u64);
        let decompressed_area_resolution = partition_area.size.width * partition_area.size.height;
        assert_eq!(decompressed_area_resolution as u64, decompressed_len);

        let mut pixels_copied: usize = 0;
        let mut pixels_left_in_row: usize = partition_area.size.width as usize;
        let partition_offset: usize = (partition_area.top_left.y as u32 * total_size.width
            + partition_area.top_left.x as u32) as usize;
        for (value, run_length) in compressed_partition {
            let overlap = (*run_length as usize).saturating_sub(pixels_left_in_row);
            if overlap == 0 {
                // simple case, just copy into the buffer
                let inside_partition_x_offset =
                    partition_area.size.width as usize - pixels_left_in_row;
                let inside_partition_y_offset = pixels_copied / partition_area.size.width as usize;
                let decompressed_row_start =
                    partition_offset + (inside_partition_y_offset * total_size.width as usize);
                let decompressed_index = decompressed_row_start + inside_partition_x_offset;
                decompressed_buffer
                    [decompressed_index..(decompressed_index + *run_length as usize)]
                    .fill(*value);

                pixels_left_in_row -= *run_length as usize;
                // if we filled the entire row
                if pixels_left_in_row == 0 {
                    pixels_left_in_row = partition_area.size.width as usize;
                }
            } else {
                // we have {overlap} pixels in the next row
                // first, copy all except the overlapping pixels
                let inside_partition_x_offset =
                    partition_area.size.width as usize - pixels_left_in_row;
                let inside_partition_y_offset = pixels_copied / partition_area.size.width as usize;
                let decompressed_first_row_start =
                    partition_offset + (inside_partition_y_offset * total_size.width as usize);
                let decompressed_index = decompressed_first_row_start + inside_partition_x_offset;
                decompressed_buffer[decompressed_index..(decompressed_index + pixels_left_in_row)]
                    .fill(*value);

                // then, check how many full rows are inside the overlap
                let full_rows = overlap / partition_area.size.width as usize;
                for row in 0..full_rows {
                    let this_row_start =
                        decompressed_first_row_start + (row + 1) * total_size.width as usize;
                    decompressed_buffer
                        [this_row_start..(this_row_start + partition_area.size.width as usize)]
                        .fill(*value);
                }

                // lastly, copy the remaining overlap
                let last_overlap = overlap - (full_rows * partition_area.size.width as usize);
                if last_overlap > 0 {
                    let last_row_start =
                        decompressed_first_row_start + (full_rows + 1) * total_size.width as usize;
                    decompressed_buffer[last_row_start..(last_row_start + last_overlap)]
                        .fill(*value);
                }

                pixels_left_in_row = partition_area.size.width as usize - last_overlap;
            }
            pixels_copied += *run_length as usize;
        }
    }
}
