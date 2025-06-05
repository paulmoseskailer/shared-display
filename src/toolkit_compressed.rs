#![allow(async_fn_in_trait)]
extern crate alloc;
use alloc::boxed::Box;
use alloc::{vec, vec::Vec};

use crate::{FLUSH_INTERVAL, FlushResult, NewPartitionError, SPAWNER, launch_future};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embedded_graphics::{
    geometry::{Point, Size},
    prelude::*,
    primitives::Rectangle,
};
use shared_display_core::{
    CompressableDisplay, CompressedDisplayPartition, FlushLock, MAX_APPS_PER_SCREEN,
};

/// Shared Display with integrated RLE-compression.
///
/// Every partition holds its own RLE-buffer and implements [`DrawTarget`]. When flushing, the
/// screen is devided into chunks with CHUNK_HEIGHT, decompressing chunks one-by-one, see
/// [`SharedCompressedDisplay::run_flush_loop_with_completion`].
pub struct SharedCompressedDisplay<const CHUNK_HEIGHT: usize, D: CompressableDisplay> {
    /// The actual display, protected by a mutex.
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    size: Size,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    buffer_pointers: heapless::Vec<*const Vec<(D::BufferElement, u8)>, MAX_APPS_PER_SCREEN>,

    spawner: &'static Spawner,
}

impl<const CHUNK_HEIGHT: usize, D: CompressableDisplay> OriginDimensions
    for SharedCompressedDisplay<CHUNK_HEIGHT, D>
{
    fn size(&self) -> Size {
        self.size
    }
}

impl<const CHUNK_HEIGHT: usize, D: CompressableDisplay> ContainsPoint
    for SharedCompressedDisplay<CHUNK_HEIGHT, D>
{
    fn contains(&self, point: Point) -> bool {
        self.bounding_box().contains(point)
    }
}

impl<const CHUNK_HEIGHT: usize, B, D> SharedCompressedDisplay<CHUNK_HEIGHT, D>
where
    D: CompressableDisplay<BufferElement = B>,
{
    /// Creates a new Shared Compressed Display from a real display.
    pub fn new(mut real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        let size = real_display.bounding_box().size;
        assert_eq!(
            size.height as usize % CHUNK_HEIGHT,
            0,
            "chosen CHUNK_HEIGHT needs to divide screen height"
        );
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
    ) -> Result<CompressedDisplayPartition<D>, NewPartitionError> {
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

        self.partition_areas.push(area).unwrap();

        Ok(partition)
    }

    /// Launches a new app in an area of the screen.
    ///
    /// Returns an error if the area is not available, overlaps with existing apps or the screen
    /// border.
    pub async fn launch_new_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(CompressedDisplayPartition<D>) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition);
        self.spawner.must_spawn(launch_future(Box::pin(fut), area));

        Ok(())
    }

    /// Runs the flush loop, additionally calling the passed in function at the end of every flush.
    ///
    /// Note that the flushing is already done internally, chunk-by-chunk, calling
    /// [`CompressableDisplay::flush_chunk`] for every decompressed chunk. The passed in function can be used to
    /// complete a flush, for example if [`CompressableDisplay::flush_chunk`] draws to a buffer
    /// that has to be drawn to the actual screen. It is called once per flush, after all chunks have been
    /// decompressed.
    /// Only exits if the flush function returns [`FlushResult::Abort`].
    pub async fn run_flush_loop_with_completion<F>(&self, mut flush_complete_fn: F)
    where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            if self.partition_areas.is_empty() {
                Timer::after(FLUSH_INTERVAL).await;
                continue;
            }

            let num_chunks = self.size.height as usize / CHUNK_HEIGHT;
            for chunk in 0..num_chunks {
                let chunk_area = Rectangle::new(
                    Point::new(0, (chunk * CHUNK_HEIGHT) as i32),
                    Size::new(self.size.width, CHUNK_HEIGHT as u32),
                );

                let decompressed_chunk: Vec<D::BufferElement> = FlushLock::new()
                    .protect_flush(async || self.decompress_chunk(chunk_area))
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
            < intersection_start_index_relative_to_src
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
            pixels_copied += pixels_to_copy;
        }

        assert_eq!(pixels_copied, result.len());
        assert_eq!(pixels_copied, total_pixels);
        result
    }
}
