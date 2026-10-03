//! Off-thread read of an artwork handle, shared by the MilkDrop cover sampler
//! and the dynamic accent: both take the playing cover's pixels into
//! something of their own on a blocking thread.

use iced::widget::image::Handle;

/// A blocking job that turns an artwork handle's pixels into `T`.
pub(crate) type CoverJob<T> = Box<dyn FnOnce() -> Option<T> + Send>;

/// The job that reads `handle` into `T` off the UI thread: encoded bytes
/// through `from_encoded`, decoded RGBA through `from_rgba`. `None` for a
/// path handle, which holds no pixels in memory. The handle's buffer is
/// reference-counted, so building the job copies nothing.
pub(crate) fn cover_job<T: Send + 'static>(
    handle: &Handle,
    from_encoded: fn(&[u8]) -> Option<T>,
    from_rgba: fn(u32, u32, &[u8]) -> Option<T>,
) -> Option<CoverJob<T>> {
    match handle {
        Handle::Bytes(_, bytes) => {
            let bytes = bytes.clone();
            Some(Box::new(move || from_encoded(&bytes)))
        }
        Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } => {
            let (width, height, pixels) = (*width, *height, pixels.clone());
            Some(Box::new(move || from_rgba(width, height, &pixels)))
        }
        Handle::Path(..) => None,
    }
}
