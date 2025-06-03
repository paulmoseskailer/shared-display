#![allow(async_fn_in_trait)]
extern crate alloc;
use alloc::boxed::Box;
use alloc::{vec, vec::Vec};

use core::{future::Future, pin::Pin};
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

// TODO: allow user to choose chunk size
const SCREEN_WIDTH: usize = 128;
const CHUNK_SIZE: Size = Size::new(SCREEN_WIDTH as u32, SCREEN_WIDTH as u32 / 4); // assumed to have screen width
const CHUNK_AREAS: [Rectangle; 2] = [
    const { Rectangle::new(Point::new(0, 0), CHUNK_SIZE) },
    const { Rectangle::new(Point::new(0, SCREEN_WIDTH as i32 / 4), CHUNK_SIZE) },
];

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

    pub async fn flush_loop<F>(&self, mut flush_complete_fn: F)
    where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            if self.partition_areas.len() == 0 {
                Timer::after(FLUSH_INTERVAL).await;
                continue;
            }

            let chunk_areas = CHUNK_AREAS;
            for chunk_area in chunk_areas {
                let decompressed_chunk: Vec<D::BufferElement> = FlushLock::new()
                    .protect_flush(async || self.decompress_chunk(chunk_area.clone()))
                    .await;
                self.real_display
                    .lock()
                    .await
                    .flush_chunk(decompressed_chunk, chunk_area)
                    .await;
            }

            let flush_result = FlushLock::new()
                .protect_flush(async || {
                    flush_complete_fn(&mut *self.real_display.lock().await).await
                })
                .await;
            match flush_result {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break;
                }
            }

            Timer::after(FLUSH_INTERVAL).await;
        }
    }

    fn decompress_chunk(&self, chunk_area: Rectangle) -> Vec<D::BufferElement> {
        let resolution = chunk_area.size.width * chunk_area.size.height;
        assert_eq!(
            chunk_area.top_left.x, 0,
            "a chunk does not span the entire width of the screen"
        );
        assert_eq!(
            chunk_area.size.width, self.size.width,
            "a chunk does not span the entire width of the screen"
        );

        let mut decompressed_chunk: Vec<D::BufferElement> =
            vec![D::BufferElement::default(); resolution as usize];
        for (i, partition_area) in self.partition_areas.iter().enumerate() {
            let intersection: Rectangle = partition_area.intersection(&chunk_area);
            if intersection.size == Size::zero() {
                continue;
            }

            // decompress intersection with partition
            let compressed_partition: &Vec<(B, u8)> = unsafe { &*self.buffer_pointers[i] };
            let decompressed_intersection =
                Self::decompress_intersection(compressed_partition, *partition_area, intersection);

            // copy decompressed intersection into chunk row by row
            let y_offset_in_chunk = (intersection.top_left.y - chunk_area.top_left.y) as usize;
            let x_offset_in_chunk = intersection.top_left.x as usize; //chunk starts at x=0
            let start_index_in_chunk =
                y_offset_in_chunk * chunk_area.size.width as usize + x_offset_in_chunk;
            let pixels_to_copy_per_row = intersection.size.width as usize;

            for row in 0..(intersection.size.height as usize) {
                let row_start_index_chunk =
                    start_index_in_chunk + (chunk_area.size.width as usize) * row;
                let row_start_index_intersection = row * intersection.size.width as usize;
                if row_start_index_chunk + pixels_to_copy_per_row > decompressed_chunk.len() {
                    panic!("destination buffer index out of range");
                }
                if row_start_index_intersection + pixels_to_copy_per_row
                    > decompressed_intersection.len()
                {
                    panic!("src buffer index out of range");
                }
                decompressed_chunk
                    [row_start_index_chunk..(row_start_index_chunk + pixels_to_copy_per_row)]
                    .copy_from_slice(
                        &decompressed_intersection[row_start_index_intersection
                            ..(row_start_index_intersection + pixels_to_copy_per_row)],
                    );
            }
        }
        decompressed_chunk
    }

    fn decompress_intersection(
        compressed_partition: &Vec<(D::BufferElement, u8)>,
        compressed_partition_area: Rectangle,
        intersection: Rectangle,
    ) -> Vec<D::BufferElement> {
        // we can assume that the intersection is as wide as the partition, since chunks are as
        // wide as the screen
        assert_eq!(
            intersection.size.width, compressed_partition_area.size.width,
            "CHUNK_SIZE needs to be as wide as the screen"
        );

        let intersection_top_left_relative_to_src =
            intersection.top_left - compressed_partition_area.top_left;
        let intersection_start_index_relative_to_src: usize =
            (intersection_top_left_relative_to_src.y as u32 * compressed_partition_area.size.width
                + intersection_top_left_relative_to_src.x as u32)
                .try_into()
                .unwrap();

        // find first run in RLE compressed buffer
        let mut decompressed_index_in_src: usize = 0;
        let mut run_iter = compressed_partition.iter();
        let run = run_iter
            .next()
            .expect("RLE-compressed partition has no runs!");
        let mut next_color = run.0;
        let mut next_run_len: u8 = run.1;

        while (decompressed_index_in_src + next_run_len as usize)
            < intersection_start_index_relative_to_src as usize
        {
            decompressed_index_in_src += next_run_len as usize;
            let run = run_iter.next().expect(
                "RLE-compressed partition ran out of runs before finding chunk intersection!",
            );
            (next_color, next_run_len) = *run;
        }

        let total_pixels = intersection.size.width as usize * intersection.size.height as usize;
        let mut result = Vec::with_capacity(total_pixels);

        // copy run by run
        // special case for first run
        let first_run_overlap = (decompressed_index_in_src + next_run_len as usize)
            - intersection_start_index_relative_to_src;
        let pixels_to_copy = first_run_overlap.min(total_pixels);
        result.extend(core::iter::repeat_n(next_color, pixels_to_copy));
        let mut pixels_copied = pixels_to_copy;

        // all other runs
        while pixels_copied < total_pixels {
            let run = run_iter.next().expect(
                "RLE-compressed partition has no runs left, but hasn't copied the entire chunk intersection!",
            );
            (next_color, next_run_len) = *run;
            let pixels_left = total_pixels.saturating_sub(pixels_copied);
            let pixels_to_copy = (next_run_len as usize).min(pixels_left);
            result.extend(core::iter::repeat_n(next_color, pixels_to_copy));
            pixels_copied += pixels_to_copy as usize;
        }

        assert_eq!(pixels_copied, result.len());
        assert_eq!(pixels_copied, total_pixels);
        result
    }
}
