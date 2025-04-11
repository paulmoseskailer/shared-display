extern crate alloc;
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::Timer;
use embedded_graphics::{
    geometry::{Point, Size},
    primitives::Rectangle,
};
use static_cell::StaticCell;

use shared_display_core::{
    DisplayPartition, DrawTracker, PartitioningError, SharableBufferedDisplay,
};

const MAX_APPS: usize = 8;
const EVENT_QUEUE_SIZE: usize = MAX_APPS;
pub static EVENTS: Channel<CriticalSectionRawMutex, ResizeEvent, EVENT_QUEUE_SIZE> = Channel::new();
static SPAWNER: StaticCell<Spawner> = StaticCell::new();

pub enum AppStart {
    Success,
    Failure,
}

pub enum FlushResult {
    Continue,
    Abort,
}

pub enum ResizeEvent {
    AppClosed(Rectangle),
}

pub struct SharedDisplay<D: SharableBufferedDisplay> {
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS>,
    draw_trackers: heapless::Vec<&'static DrawTracker, MAX_APPS>,

    spawner: &'static Spawner,
}

impl<B, D> SharedDisplay<D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    pub async fn new(real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        SharedDisplay {
            real_display: Mutex::new(real_display),
            partition_areas: heapless::Vec::new(),
            draw_trackers: heapless::Vec::new(),
            spawner: spawner_ref,
        }
    }

    pub async fn new_partition(
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

        static DRAW_TRACKER: DrawTracker = DrawTracker::new();
        let result = real_display.new_partition(area, &DRAW_TRACKER);

        if result.is_ok() {
            self.partition_areas.push(area.clone()).unwrap();
            _ = self.draw_trackers.push(&DRAW_TRACKER);
        }

        result
    }

    pub async fn split_vertically(
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

    /// Re-splits an existing partition. Fails if the partition doesn't exist but does not remove
    /// the current partition's screen access
    pub async fn split_existing_unchecked(
        &mut self,
        existing_area: Rectangle,
    ) -> Result<(DisplayPartition<B, D>, DisplayPartition<B, D>), PartitioningError> {
        let mut maybe_i = None;
        for (i, p) in self.partition_areas.iter().enumerate() {
            if *p == existing_area {
                maybe_i = Some(i);
            }
        }
        let Some(existing_i) = maybe_i else {
            return Err(PartitioningError::ExistingNotFound);
        };

        self.partition_areas.remove(existing_i);

        let half_width = existing_area.size.width / 2;

        Ok((
            self.new_partition(Rectangle::new(
                existing_area.top_left,
                Size::new(half_width, existing_area.size.height),
            ))
            .await?,
            self.new_partition(Rectangle::new(
                existing_area.top_left + Point::new(half_width as i32, 0),
                Size::new(half_width, existing_area.size.height),
            ))
            .await?,
        ))
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

    pub async fn flush_loop<F>(&self, mut flush: F)
    where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            // TODO: only flush if any partition has updates
            for &draw_tracker in self.draw_trackers.iter() {
                let area = draw_tracker.dirty_area.lock().await.size;
                println!("draw_tracker has size {}x{}", area.width, area.height);
            }
            match flush(&mut *self.real_display.lock().await).await {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break;
                }
            }
            Timer::after_millis(200).await;
        }
    }
}

#[embassy_executor::task(pool_size = MAX_APPS)]
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
