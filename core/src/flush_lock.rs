use core::sync::atomic::Ordering;
use embassy_time::{Duration, Timer};
use portable_atomic::AtomicU8;

static INNER: AtomicU8 = AtomicU8::new(0);
const FLUSH_LOCK_BIT: u8 = 0b1000_0000;
const COUNTER_BITS: u8 = !FLUSH_LOCK_BIT;
const MAX_WRITERS: u8 = COUNTER_BITS;

const RETRY_DELAY: Duration = Duration::from_millis(20);

/// A lock to avoid writes to the buffer during decompression for flushing, but allow multiple
/// writes at the same time.
pub struct FlushLock {}

impl FlushLock {
    /// Creates a new lock.
    pub fn new() -> Self {
        FlushLock {}
    }

    async fn lock_flush(&self) {
        let res = INNER.fetch_add(FLUSH_LOCK_BIT, Ordering::Relaxed);
        assert_eq!(
            INNER.load(Ordering::Relaxed) & FLUSH_LOCK_BIT,
            FLUSH_LOCK_BIT
        );
        assert_eq!(
            res & FLUSH_LOCK_BIT,
            0,
            "attempted to flush lock, was already flushing"
        );

        while INNER.load(Ordering::Relaxed) & COUNTER_BITS > 0 {
            Timer::after(RETRY_DELAY).await;
        }

        assert_eq!(INNER.load(Ordering::Relaxed), FLUSH_LOCK_BIT);
    }

    async fn unlock_flush(&self) {
        let before = INNER.swap(0, Ordering::Relaxed);
        assert_eq!(
            before, FLUSH_LOCK_BIT,
            "after flush, flush lock not locked or counter != 0"
        );
    }

    /// Ensures no writes are in progress before flushing.
    pub async fn protect_flush<F, R>(&self, f: F) -> R
    where
        F: AsyncFnOnce() -> R,
    {
        self.lock_flush().await;
        // TODO: make sure unlock is called even if f panics?
        let result = f().await;
        self.unlock_flush().await;
        result
    }

    async fn lock_write(&self) {
        'lock_write_loop: loop {
            let current = INNER.load(Ordering::Relaxed);
            if current & FLUSH_LOCK_BIT > 0 {
                // flush in progress, try again
                Timer::after(RETRY_DELAY).await;
                continue;
            }
            if current & COUNTER_BITS == MAX_WRITERS {
                // max number of writers accessing, try again
                Timer::after(2 * RETRY_DELAY).await;
                continue;
            }

            // just now nobody was flushing, try to increase counter
            match INNER.compare_exchange(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
            {
                Err(_) =>
                // compare_exchange failure -> someone else wrote since last load(), try again
                {
                    Timer::after(RETRY_DELAY).await;
                    continue 'lock_write_loop;
                }
                Ok(_) =>
                // compare_exchange success -> no flush in progress, counter increased, success!
                {
                    break 'lock_write_loop;
                }
            }
        }
    }
    async fn unlock_write(&self) {
        let before = INNER.fetch_sub(1, Ordering::Relaxed);
        assert_ne!(
            before, FLUSH_LOCK_BIT,
            "before write_unlock, only FLUSH_LOCK was set, no writers registered"
        );
        assert_ne!(before & COUNTER_BITS, 0, "after write, write counter was 0");
    }

    /// Ensures no flush is in progress before writing.
    pub async fn protect_write<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.lock_write().await;
        // TODO: make sure unlock is called even if f panics?
        let result = f();
        self.unlock_write().await;
        result
    }
}
