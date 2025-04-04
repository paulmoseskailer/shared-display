use core::future::Future;
use core::pin::Pin;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embedded_graphics::{
    geometry::{Point, Size},
    primitives::Rectangle,
};

use crate::sharable_display::{DisplayPartition, SharableBufferedDisplay};

const MAX_APPS: usize = 6;

pub enum AppStart {
    Success,
    Failure,
}

pub enum FlushResult {
    Continue,
    Abort,
}

pub struct SharedDisplay<D: SharableBufferedDisplay> {
    pub real_display: Mutex<CriticalSectionRawMutex, D>,

    // keep track of partition areas
    partitions: heapless::Vec<Rectangle, MAX_APPS>,

    // to launch new apps
    spawner: &'static Spawner,
}

impl<B, D> SharedDisplay<D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    pub async fn new(real_display: D, spawner: &'static Spawner) -> Self {
        SharedDisplay {
            real_display: Mutex::new(real_display),
            partitions: heapless::Vec::new(),
            spawner,
        }
    }

    pub async fn new_partition(&mut self, area: Rectangle) -> Option<DisplayPartition<B, D>> {
        let real_display: &mut D = &mut *self.real_display.lock().await;

        // check area inside display
        let bb = real_display.bounding_box();
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

        Some(real_display.new_partition(area))
    }

    pub async fn split_vertically(
        &mut self,
    ) -> Option<(DisplayPartition<B, D>, DisplayPartition<B, D>)> {
        // TODO split_buffer_vertically should return Result
        let (left_part, right_part) = self.real_display.lock().await.split_buffer_vertically();

        self.partitions.push(left_part.partition).unwrap();
        self.partitions.push(right_part.partition).unwrap();

        Some((left_part, right_part))
    }

    /// Re-splits an existing partition. Fails if the partition doesn't exist but does not remove
    /// the current partition's screen access
    pub async fn split_existing_unchecked(
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

        let left_part = self
            .new_partition(Rectangle::new(
                existing_area.top_left,
                Size::new(half_width, existing_area.size.height),
            ))
            .await?;
        let right_part = self
            .new_partition(Rectangle::new(
                existing_area.top_left + Point::new(half_width as i32, 0),
                Size::new(half_width, existing_area.size.height),
            ))
            .await?;

        Some((left_part, right_part))
    }

    pub async fn launch_new_app<F>(&mut self, mut app_fn: F, area: Rectangle) -> AppStart
    where
        F: AsyncFnMut(DisplayPartition<B, D>) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let Some(partition) = self.new_partition(area).await else {
            return AppStart::Failure;
        };

        let fut = app_fn(partition);
        self.spawner.must_spawn(launch_future(Box::pin(fut)));

        return AppStart::Success;
    }

    pub async fn flush_loop<F>(&self, mut flush: F)
    where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            match flush(&mut *self.real_display.lock().await).await {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break;
                }
            }
            Timer::after_millis(500).await;
        }
    }
}

#[embassy_executor::task(pool_size = MAX_APPS)]
async fn launch_future(app_future: Pin<Box<dyn Future<Output = ()>>>) {
    app_future.await;
    // TODO modify resize event queue
}

pub async fn flush_loop<F, D>(
    shared_display_mutex: &Mutex<CriticalSectionRawMutex, Option<SharedDisplay<D>>>,
    mut flush: F,
) where
    D: SharableBufferedDisplay,
    F: AsyncFnMut(&mut D) -> FlushResult,
{
    loop {
        match flush(
            &mut *shared_display_mutex
                .lock()
                .await
                .as_mut()
                .unwrap()
                .real_display
                .lock()
                .await,
        )
        .await
        {
            FlushResult::Continue => {}
            FlushResult::Abort => {
                break;
            }
        }
        Timer::after_millis(500).await;
    }
}
