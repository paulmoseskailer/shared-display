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
    prelude::Dimensions,
    primitives::Rectangle,
};
use static_cell::StaticCell;

use shared_display_core::{
    AreaToFlush, DisplayPartition, DrawTracker, FlushLock, MAX_APPS_PER_SCREEN, PartitioningError,
    SharableBufferedDisplay,
    compressed::{CompressableDisplay, CompressedDisplayPartition},
};

const FLUSH_INTERVAL: Duration = Duration::from_millis(20);
const EVENT_QUEUE_SIZE: usize = MAX_APPS_PER_SCREEN;
pub static EVENTS: Channel<CriticalSectionRawMutex, ResizeEvent, EVENT_QUEUE_SIZE> = Channel::new();
static SPAWNER: StaticCell<Spawner> = StaticCell::new();
static DRAW_TRACKERS: [DrawTracker; MAX_APPS_PER_SCREEN] =
    [const { DrawTracker::new() }; MAX_APPS_PER_SCREEN];

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
    ) -> Result<DisplayPartition<B, D>, PartitioningError> {
        let real_display: &mut D = &mut *self.real_display.lock().await;

        // check area inside display
        let bb = real_display.bounding_box();
        if !(bb.contains(area.top_left)
            && bb.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return Err(PartitioningError::OutsideParent);
        }

        // check area not overlapping with existing partition_areas
        for p in self.partition_areas.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return Err(PartitioningError::Overlaps);
            }
        }

        let index = self.partition_areas.len();
        let result = real_display.new_partition(area, &self.draw_trackers[index]);

        if result.is_ok() {
            self.partition_areas.push(area.clone()).unwrap();
        }

        result
    }

    pub async fn partition_vertically(
        &mut self,
    ) -> Result<(DisplayPartition<B, D>, DisplayPartition<B, D>), PartitioningError> {
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
    ) -> Result<(), PartitioningError>
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
    ) -> Result<(), PartitioningError>
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
    bounding_box: Rectangle,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    buffer_pointers: heapless::Vec<*const Vec<(D::BufferElement, u8)>, MAX_APPS_PER_SCREEN>,

    spawner: &'static Spawner,
}

impl<D: CompressableDisplay> Dimensions for SharedCompressedDisplay<D> {
    fn bounding_box(&self) -> Rectangle {
        self.bounding_box
    }
}

impl<B, D> SharedCompressedDisplay<D>
where
    D: CompressableDisplay<BufferElement = B>,
{
    pub fn new(mut real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        real_display.drop_buffer();
        let bounding_box = real_display.bounding_box();
        SharedCompressedDisplay {
            real_display: Mutex::new(real_display),
            bounding_box,
            partition_areas: heapless::Vec::new(),
            buffer_pointers: heapless::Vec::new(),
            spawner: spawner_ref,
        }
    }

    async fn new_partition(
        &mut self,
        area: Rectangle,
    ) -> Result<CompressedDisplayPartition<B, D>, PartitioningError> {
        if area.size.width < 8 {
            return Err(PartitioningError::PartitionTooSmall);
        }
        // TODO: sanity checks on area
        let partition = CompressedDisplayPartition::new(self.bounding_box.size, area);
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
    ) -> Result<(), PartitioningError>
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
        'flush_loop: loop {
            let mut partition_buffers: Vec<Vec<D::BufferElement>> =
                Vec::with_capacity(self.partition_areas.len());
            assert_eq!(self.partition_areas.len(), self.buffer_pointers.len());

            // decompress every partition's buffer
            for (buffer, &ptr) in self.buffer_pointers.iter().enumerate() {
                if ptr.is_null() {
                    panic!("found null ptr in CompressedDisplay.buffer_pointers!");
                }
                // SAFETY: We assume these pointers are valid and point to `Vec<(D::BufferElement, u8)>`.
                let rle_buffer: &Vec<(D::BufferElement, u8)> = unsafe { &*ptr };

                let partition_area = self.partition_areas[buffer];
                let partition_resolution =
                    (partition_area.size.width * partition_area.size.height) as usize;
                partition_buffers.push(Vec::with_capacity(partition_resolution));

                let mut decompressed_index: usize = 0;
                for &(value, run_len) in rle_buffer {
                    // TODO: extend from slice instead of pushing every time
                    for _ in 0..run_len {
                        partition_buffers[buffer].push(value);
                    }
                    decompressed_index += run_len as usize;
                }
                assert_eq!(decompressed_index, partition_resolution);
            }

            // combine partition buffers to one single buffer
            // TODO: take different splitting into account, correctly piece together partitions
            assert_eq!(partition_buffers.len(), 2);
            assert_eq!(partition_buffers[0].len(), partition_buffers[1].len());
            let mut entire_buffer: Vec<D::BufferElement> =
                Vec::with_capacity(2 * partition_buffers[0].len());

            // combine one row of each partition per row of entire_buffer
            let screen_height = self.bounding_box.size.height as usize;
            let screen_width = self.bounding_box.size.width as usize;

            let partition_resolution = screen_width * screen_height / 2;
            assert_eq!(partition_buffers[0].len(), partition_resolution);
            assert_eq!(partition_buffers[1].len(), partition_resolution);

            for row in 0..screen_height {
                let start = row * (screen_width / 2);
                let end = start + (screen_width / 2);
                entire_buffer.extend_from_slice(&partition_buffers[0][start..end]);
                entire_buffer.extend_from_slice(&partition_buffers[1][start..end]);
            }

            assert_eq!(entire_buffer.len(), screen_width * screen_height);

            let flush_result = FlushLock::new()
                .protect_flush(async || {
                    flush_fn(&mut *self.real_display.lock().await, entire_buffer).await
                })
                .await;
            match flush_result {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break 'flush_loop;
                }
            }

            Timer::after(FLUSH_INTERVAL).await;
        }
    }
}
