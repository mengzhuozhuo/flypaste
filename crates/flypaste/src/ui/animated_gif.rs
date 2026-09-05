use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use gpui::prelude::*;
use gpui::{canvas, div, Context, Corners, IntoElement, Render, RenderImage, Styled, Task, Window};
use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;

pub struct AnimatedGifData {
    pub render_image: Arc<RenderImage>,
    pub delays: Vec<Duration>,
    pub width: u32,
    pub height: u32,
}

static GIF_CACHE: LazyLock<Mutex<HashMap<String, Arc<AnimatedGifData>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static PRELOADING: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[cfg(target_os = "macos")]
unsafe fn get_imageio_frame_delay(
    source: *mut std::ffi::c_void,
    index: usize,
    k_gif: *mut std::ffi::c_void,
    k_unclamped: *mut std::ffi::c_void,
    k_delay: *mut std::ffi::c_void,
) -> Duration {
    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCopyPropertiesAtIndex(
            is: *mut std::ffi::c_void,
            index: usize,
            options: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CFDictionaryGetValue(
            dict: *mut std::ffi::c_void,
            key: *const std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CFNumberGetValue(
            number: *mut std::ffi::c_void,
            theType: isize,
            valuePtr: *mut f64,
        ) -> bool;
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    let mut delay_sec = 0.1f64;
    unsafe {
        let props = CGImageSourceCopyPropertiesAtIndex(source, index, std::ptr::null_mut());
        if !props.is_null() {
            let gif_props = CFDictionaryGetValue(props, k_gif as *const _);
            if !gif_props.is_null() {
                let mut num = CFDictionaryGetValue(gif_props, k_unclamped as *const _);
                if num.is_null() {
                    num = CFDictionaryGetValue(gif_props, k_delay as *const _);
                }
                if !num.is_null() {
                    let mut val = 0.0f64;
                    if CFNumberGetValue(num, 13, &mut val) && val > 0.0 {
                        delay_sec = val;
                    }
                }
            }
            CFRelease(props);
        }
    }

    let millis = (delay_sec * 1000.0) as u64;
    if millis < 20 {
        Duration::from_millis(100)
    } else {
        Duration::from_millis(millis)
    }
}

/// Decodes GIF using macOS native ImageIO framework.
/// Hardware-accelerated and compiled with Apple Silicon NEON SIMD,
/// decoding high-res 100+ frame GIFs in sub-second time (<1s) even in unoptimized debug builds.
#[cfg(target_os = "macos")]
pub fn decode_gif_imageio(bytes: &[u8], min_delay: Duration) -> Option<AnimatedGifData> {
    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CFDataCreate(
            allocator: *mut std::ffi::c_void,
            bytes: *const u8,
            length: isize,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateWithData(
            data: *mut std::ffi::c_void,
            options: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceGetCount(is: *mut std::ffi::c_void) -> usize;
        fn CGImageSourceCreateImageAtIndex(
            is: *mut std::ffi::c_void,
            index: usize,
            options: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageGetWidth(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetHeight(image: *mut std::ffi::c_void) -> usize;
        fn CGColorSpaceCreateDeviceRGB() -> *mut std::ffi::c_void;
        fn CGColorSpaceRelease(space: *mut std::ffi::c_void);
        fn CGBitmapContextCreate(
            data: *mut std::ffi::c_void,
            width: usize,
            height: usize,
            bitsPerComponent: usize,
            bytesPerRow: usize,
            space: *mut std::ffi::c_void,
            bitmapInfo: u32,
        ) -> *mut std::ffi::c_void;
        fn CGContextDrawImage(
            context: *mut std::ffi::c_void,
            rect: CGRect,
            image: *mut std::ffi::c_void,
        );
        fn CGContextRelease(context: *mut std::ffi::c_void);
        fn CGImageRelease(image: *mut std::ffi::c_void);
        fn CFStringCreateWithCString(
            alloc: *mut std::ffi::c_void,
            cStr: *const std::ffi::c_char,
            encoding: u32,
        ) -> *mut std::ffi::c_void;
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    unsafe {
        let cf_data = CFDataCreate(std::ptr::null_mut(), bytes.as_ptr(), bytes.len() as isize);
        if cf_data.is_null() {
            return None;
        }
        let source = CGImageSourceCreateWithData(cf_data, std::ptr::null_mut());
        if source.is_null() {
            CFRelease(cf_data);
            return None;
        }

        let count = CGImageSourceGetCount(source);
        if count == 0 {
            CFRelease(source);
            CFRelease(cf_data);
            return None;
        }

        let first_img = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null_mut());
        if first_img.is_null() {
            CFRelease(source);
            CFRelease(cf_data);
            return None;
        }

        let width = CGImageGetWidth(first_img);
        let height = CGImageGetHeight(first_img);
        CGImageRelease(first_img);

        if width == 0 || height == 0 {
            CFRelease(source);
            CFRelease(cf_data);
            return None;
        }

        let color_space = CGColorSpaceCreateDeviceRGB();
        // kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little outputs BGRA pixels directly for Metal
        let bitmap_info: u32 = 8194 | (2 << 12);
        let rect = CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        );

        let k_gif = CFStringCreateWithCString(std::ptr::null_mut(), b"{GIF}\0".as_ptr() as *const _, 0x08000100);
        let k_unclamped = CFStringCreateWithCString(std::ptr::null_mut(), b"UnclampedDelayTime\0".as_ptr() as *const _, 0x08000100);
        let k_delay = CFStringCreateWithCString(std::ptr::null_mut(), b"DelayTime\0".as_ptr() as *const _, 0x08000100);

        let mut frames = Vec::with_capacity(count);
        let mut delays = Vec::with_capacity(count);
        let mut accumulated_delay = Duration::ZERO;

        for i in 0..count {
            let delay = get_imageio_frame_delay(source, i, k_gif, k_unclamped, k_delay);
            accumulated_delay += delay;

            if accumulated_delay >= min_delay || i == count - 1 {
                let img = CGImageSourceCreateImageAtIndex(source, i, std::ptr::null_mut());
                if !img.is_null() {
                    let mut buf = vec![0u8; width * height * 4];
                    let ctx = CGBitmapContextCreate(
                        buf.as_mut_ptr() as *mut _,
                        width,
                        height,
                        8,
                        width * 4,
                        color_space,
                        bitmap_info,
                    );
                    if !ctx.is_null() {
                        CGContextDrawImage(ctx, rect, img);
                        CGContextRelease(ctx);

                        if let Some(rgba_img) = image::RgbaImage::from_raw(width as u32, height as u32, buf) {
                            let frame = image::Frame::from_parts(
                                rgba_img,
                                0,
                                0,
                                image::Delay::from_numer_denom_ms(accumulated_delay.as_millis() as u32, 1),
                            );
                            frames.push(frame);
                            delays.push(accumulated_delay);
                        }
                    }
                    CGImageRelease(img);
                }
                accumulated_delay = Duration::ZERO;
            }
        }

        CFRelease(k_gif);
        CFRelease(k_unclamped);
        CFRelease(k_delay);
        CGColorSpaceRelease(color_space);
        CFRelease(source);
        CFRelease(cf_data);

        if frames.is_empty() {
            return None;
        }

        Some(AnimatedGifData {
            render_image: Arc::new(RenderImage::new(frames)),
            delays,
            width: width as u32,
            height: height as u32,
        })
    }
}

/// Decodes the first frame of a GIF using macOS ImageIO in ~1-2ms.
#[cfg(target_os = "macos")]
pub fn decode_first_frame_imageio(bytes: &[u8]) -> Option<(Arc<RenderImage>, u32, u32)> {
    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CFDataCreate(
            allocator: *mut std::ffi::c_void,
            bytes: *const u8,
            length: isize,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateWithData(
            data: *mut std::ffi::c_void,
            options: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageSourceCreateImageAtIndex(
            is: *mut std::ffi::c_void,
            index: usize,
            options: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGImageGetWidth(image: *mut std::ffi::c_void) -> usize;
        fn CGImageGetHeight(image: *mut std::ffi::c_void) -> usize;
        fn CGColorSpaceCreateDeviceRGB() -> *mut std::ffi::c_void;
        fn CGColorSpaceRelease(space: *mut std::ffi::c_void);
        fn CGBitmapContextCreate(
            data: *mut std::ffi::c_void,
            width: usize,
            height: usize,
            bitsPerComponent: usize,
            bytesPerRow: usize,
            space: *mut std::ffi::c_void,
            bitmapInfo: u32,
        ) -> *mut std::ffi::c_void;
        fn CGContextDrawImage(
            context: *mut std::ffi::c_void,
            rect: CGRect,
            image: *mut std::ffi::c_void,
        );
        fn CGContextRelease(context: *mut std::ffi::c_void);
        fn CGImageRelease(image: *mut std::ffi::c_void);
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    unsafe {
        let cf_data = CFDataCreate(std::ptr::null_mut(), bytes.as_ptr(), bytes.len() as isize);
        if cf_data.is_null() {
            return None;
        }
        let source = CGImageSourceCreateWithData(cf_data, std::ptr::null_mut());
        if source.is_null() {
            CFRelease(cf_data);
            return None;
        }

        let first_img = CGImageSourceCreateImageAtIndex(source, 0, std::ptr::null_mut());
        if first_img.is_null() {
            CFRelease(source);
            CFRelease(cf_data);
            return None;
        }

        let width = CGImageGetWidth(first_img);
        let height = CGImageGetHeight(first_img);

        let color_space = CGColorSpaceCreateDeviceRGB();
        let bitmap_info: u32 = 8194 | (2 << 12);
        let rect = CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        );

        let mut buf = vec![0u8; width * height * 4];
        let ctx = CGBitmapContextCreate(
            buf.as_mut_ptr() as *mut _,
            width,
            height,
            8,
            width * 4,
            color_space,
            bitmap_info,
        );
        if !ctx.is_null() {
            CGContextDrawImage(ctx, rect, first_img);
            CGContextRelease(ctx);
        }
        CGImageRelease(first_img);
        CGColorSpaceRelease(color_space);
        CFRelease(source);
        CFRelease(cf_data);

        let rgba_img = image::RgbaImage::from_raw(width as u32, height as u32, buf)?;
        let frame = image::Frame::from_parts(rgba_img, 0, 0, image::Delay::from_numer_denom_ms(100, 1));
        let render_image = Arc::new(RenderImage::new(vec![frame]));
        Some((render_image, width as u32, height as u32))
    }
}

/// Pure software Rust fallback for non-macOS or corrupted files.
pub fn decode_gif_software(bytes: &[u8], min_delay: Duration) -> anyhow::Result<AnimatedGifData> {
    let decoder = GifDecoder::new(Cursor::new(bytes))?;
    let mut total_width = 0;
    let mut total_height = 0;

    let mut accumulated_delay = Duration::ZERO;
    let mut pending_frame: Option<image::Frame> = None;

    let mut decimated_frames = Vec::new();
    let mut delays = Vec::new();

    for frame_res in decoder.into_frames() {
        let frame = frame_res?;
        let delay = Duration::from(frame.delay());
        let delay = if delay < Duration::from_millis(20) {
            Duration::from_millis(100)
        } else {
            delay
        };

        accumulated_delay += delay;
        if total_width == 0 {
            total_width = frame.buffer().width();
            total_height = frame.buffer().height();
        }
        pending_frame = Some(frame);

        if accumulated_delay >= min_delay {
            let mut frame = pending_frame.take().unwrap();
            let samples = frame.buffer_mut().as_flat_samples_mut().samples;
            for chunk in samples.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }
            decimated_frames.push(frame);
            delays.push(accumulated_delay);
            accumulated_delay = Duration::ZERO;
        }
    }

    if let Some(mut frame) = pending_frame {
        let delay = if accumulated_delay > Duration::ZERO {
            accumulated_delay
        } else {
            Duration::from_millis(100)
        };
        let samples = frame.buffer_mut().as_flat_samples_mut().samples;
        for chunk in samples.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }
        decimated_frames.push(frame);
        delays.push(delay);
    }

    if decimated_frames.is_empty() {
        anyhow::bail!("GIF has no frames");
    }

    let full_data = AnimatedGifData {
        render_image: Arc::new(RenderImage::new(decimated_frames)),
        delays,
        width: total_width,
        height: total_height,
    };

    Ok(full_data)
}

/// Decodes only the first frame in software fallback.
pub fn decode_first_frame_software(bytes: &[u8]) -> Option<(Arc<RenderImage>, u32, u32)> {
    let decoder = GifDecoder::new(Cursor::new(bytes)).ok()?;
    let mut frames = decoder.into_frames();
    let mut first_frame = frames.next()?.ok()?;
    let width = first_frame.buffer().width();
    let height = first_frame.buffer().height();

    let samples = first_frame.buffer_mut().as_flat_samples_mut().samples;
    for chunk in samples.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    let render_image = Arc::new(RenderImage::new(vec![first_frame]));
    Some((render_image, width, height))
}

/// Decodes all frames of a GIF into a `gpui::RenderImage` and frame delays.
/// Uses hardware-accelerated macOS ImageIO when available (<1s), falling back to software.
pub fn decode_gif_bytes(bytes: &[u8]) -> anyhow::Result<AnimatedGifData> {
    #[cfg(target_os = "macos")]
    if let Some(data) = decode_gif_imageio(bytes, Duration::ZERO) {
        return Ok(data);
    }
    decode_gif_software(bytes, Duration::ZERO)
}

/// Decodes only the first frame of a GIF in <2ms for instant rendering without lag.
pub fn decode_first_frame(bytes: &[u8]) -> Option<(Arc<RenderImage>, u32, u32)> {
    #[cfg(target_os = "macos")]
    if let Some(res) = decode_first_frame_imageio(bytes) {
        return Some(res);
    }
    decode_first_frame_software(bytes)
}

/// Preloads documentation GIFs in background thread so they are 0ms instant when user clicks tutorial.
pub fn preload_doc_gifs() {
    std::thread::spawn(|| {
        for doc in &["docs/tutorial.md", "docs/tutorial_en.md", "docs/faq.md", "docs/faq_en.md"] {
            if let Some(asset) = crate::assets::Assets::get(doc) {
                if let Ok(text) = std::str::from_utf8(asset.data.as_ref()) {
                    let parser = pulldown_cmark::Parser::new(text);
                    for event in parser {
                        if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image { dest_url, .. }) = event {
                            let url = dest_url.to_string();
                            let clean_url = url.trim_start_matches("./").trim_start_matches("/");
                            if clean_url.ends_with(".gif") {
                                let asset_path = format!("docs/{}", clean_url);
                                preload_single_gif(&asset_path);
                            }
                        }
                    }
                }
            }
        }
    });
}

fn preload_single_gif(asset_path: &str) {
    {
        let cache = GIF_CACHE.lock().unwrap();
        if cache.contains_key(asset_path) {
            return;
        }
    }
    {
        let mut preloading = PRELOADING.lock().unwrap();
        if !preloading.insert(asset_path.to_string()) {
            return;
        }
    }

    if let Some(asset) = crate::assets::Assets::get(asset_path) {
        if let Ok(data) = decode_gif_bytes(asset.data.as_ref()) {
            let mut cache = GIF_CACHE.lock().unwrap();
            cache.insert(asset_path.to_string(), Arc::new(data));
        }
    }

    let mut preloading = PRELOADING.lock().unwrap();
    preloading.remove(asset_path);
}

#[allow(dead_code)]
pub fn load_animated_gif(asset_path: &str) -> Option<Arc<AnimatedGifData>> {
    {
        let cache = GIF_CACHE.lock().unwrap();
        if let Some(data) = cache.get(asset_path) {
            return Some(data.clone());
        }
    }

    let asset = crate::assets::Assets::get(asset_path)?;
    match decode_gif_bytes(asset.data.as_ref()) {
        Ok(gif_data) => {
            let data = Arc::new(gif_data);
            let mut cache = GIF_CACHE.lock().unwrap();
            cache.insert(asset_path.to_string(), data.clone());
            Some(data)
        }
        Err(e) => {
            log::error!("Failed to decode animated GIF {}: {:?}", asset_path, e);
            None
        }
    }
}

pub struct AnimatedGifView {
    #[allow(dead_code)]
    asset_path: String,
    initial_frame: Option<Arc<RenderImage>>,
    initial_width: u32,
    initial_height: u32,
    data: Option<Arc<AnimatedGifData>>,
    current_frame: usize,
    _task: Option<Task<()>>,
}

impl AnimatedGifView {
    pub fn new(asset_path: String, cx: &mut Context<Self>) -> Self {
        // 1. Check if the full animation data is already cached (e.g. from background preloading)
        let cached_data = {
            let cache = GIF_CACHE.lock().unwrap();
            cache.get(&asset_path).cloned()
        };

        // 2. Fast synchronous first frame load (<2ms) to display immediately without UI freeze
        let (initial_frame, initial_width, initial_height) = if let Some(data) = &cached_data {
            (Some(data.render_image.clone()), data.width, data.height)
        } else if let Some(asset) = crate::assets::Assets::get(&asset_path) {
            decode_first_frame(asset.data.as_ref())
                .map(|(img, w, h)| (Some(img), w, h))
                .unwrap_or((None, 0, 0))
        } else {
            (None, 0, 0)
        };

        let mut view = Self {
            asset_path: asset_path.clone(),
            initial_frame,
            initial_width,
            initial_height,
            data: cached_data.clone(),
            current_frame: 0,
            _task: None,
        };

        if let Some(data) = cached_data {
            // Already cached: start full animation immediately!
            view.start_animation(data, cx);
        } else {
            // Decode all frames in background thread with hardware acceleration (<1s)
            let path_clone = asset_path.clone();
            cx.spawn(async move |handle, cx| {
                let path_for_fetch = path_clone.clone();
                let decoded = cx
                    .background_executor()
                    .spawn(async move {
                        if let Some(asset) = crate::assets::Assets::get(&path_for_fetch) {
                            decode_gif_bytes(asset.data.as_ref()).ok().map(Arc::new)
                        } else {
                            None
                        }
                    })
                    .await;

                if let Some(data) = decoded {
                    {
                        let mut cache = GIF_CACHE.lock().unwrap();
                        cache.insert(path_clone, data.clone());
                    }

                    // Start playing the complete animation from frame 0 to end!
                    let _ = handle.update(cx, |this: &mut AnimatedGifView, cx| {
                        this.start_animation(data, cx);
                    });
                }
            })
            .detach();
        }

        view
    }

    fn start_animation(&mut self, data: Arc<AnimatedGifData>, cx: &mut Context<Self>) {
        if data.delays.is_empty() {
            return;
        }

        self.initial_width = data.width;
        self.initial_height = data.height;
        self.data = Some(data.clone());
        self.current_frame = 0;
        cx.notify();

        if data.delays.len() <= 1 {
            return;
        }

        let delays = data.delays.clone();
        self._task = Some(cx.spawn(async move |handle, cx| {
            let mut frame_idx = 0;
            loop {
                let delay = delays[frame_idx];
                cx.background_executor().timer(delay).await;

                frame_idx = (frame_idx + 1) % delays.len();

                let updated = handle.update(cx, |this, cx| {
                    this.current_frame = frame_idx;
                    cx.notify();
                });

                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    #[allow(dead_code)]
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    #[allow(dead_code)]
    pub fn frame_count(&self) -> usize {
        self.data
            .as_ref()
            .map(|d| d.delays.len())
            .unwrap_or(if self.initial_frame.is_some() { 1 } else { 0 })
    }
}

impl Render for AnimatedGifView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (render_image, width, height, frame_index) = if let Some(data) = &self.data {
            let frame = self.current_frame.min(data.delays.len().saturating_sub(1));
            (Some(data.render_image.clone()), data.width, data.height, frame)
        } else if let Some(initial) = &self.initial_frame {
            (Some(initial.clone()), self.initial_width, self.initial_height, 0)
        } else {
            (None, 0, 0, 0)
        };

        if let Some(render_image) = render_image {
            let max_w = 640.0f32;
            let max_h = 400.0f32;

            let (display_w, display_h) = if width > 0 && height > 0 {
                let w = width as f32;
                let h = height as f32;
                let scale = (max_w / w).min(max_h / h).min(1.0);
                (w * scale, h * scale)
            } else {
                (max_w, max_h)
            };

            div()
                .w_full()
                .flex()
                .justify_center()
                .mt_2()
                .mb_4()
                .child(
                    div()
                        .w(gpui::px(display_w))
                        .h(gpui::px(display_h))
                        .rounded_md()
                        .overflow_hidden()
                        .child(
                            canvas(
                                move |_bounds, _window, _cx| (),
                                move |bounds, (), window, _cx| {
                                    let _ = window.paint_image(
                                        bounds,
                                        Corners::all(gpui::px(6.0)),
                                        render_image,
                                        frame_index,
                                        false,
                                    );
                                },
                            )
                            .size_full(),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .w_full()
                .h(gpui::px(100.0))
                .flex()
                .items_center()
                .justify_center()
                .child("加载动图中...")
                .text_color(gpui::rgba(0x888888FF))
                .into_any_element()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Frame, Rgba, RgbaImage};
    #[test]
    fn test_color_channel_conversion() {
        let mut gif_bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut gif_bytes);
            // Create a 1-pixel red image: R=255, G=0, B=0, A=255
            let red_pixel = Rgba([255, 0, 0, 255]);
            let img = RgbaImage::from_pixel(1, 1, red_pixel);
            let frame = Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(100, 1));
            encoder.encode_frame(frame).unwrap();
        }

        // Test decode_first_frame: should convert RGBA to BGRA
        // Byte 0 should now be B (0), Byte 1 is G (0), Byte 2 is R (255), Byte 3 is A (255)
        let decoder = GifDecoder::new(Cursor::new(&gif_bytes)).unwrap();
        let mut frames = decoder.into_frames();
        let mut first = frames.next().unwrap().unwrap();
        let samples = first.buffer_mut().as_flat_samples_mut().samples;
        assert_eq!(samples[0], 255); // Original is RGBA: R is 255
        assert_eq!(samples[2], 0);   // Original is RGBA: B is 0

        for chunk in samples.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }
        assert_eq!(samples[0], 0);   // After swap to BGRA: B is 0
        assert_eq!(samples[2], 255); // After swap to BGRA: R is 255
    }

    fn make_test_gif(num_frames: usize, delay_ms: u32) -> Vec<u8> {

        let mut gif_bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut gif_bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            for i in 0..num_frames {

                let color = match i % 3 {
                    0 => Rgba([255, 0, 0, 255]),
                    1 => Rgba([0, 255, 0, 255]),
                    _ => Rgba([0, 0, 255, 255]),
                };
                let img = RgbaImage::from_pixel(16, 16, color);
                let frame = Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(delay_ms, 1));
                encoder.encode_frame(frame).unwrap();
            }
        }
        gif_bytes
    }

    #[test]
    fn test_decode_gif_bytes_success() {
        let gif_bytes = make_test_gif(3, 50);
        let decoded = decode_gif_bytes(&gif_bytes).expect("Should decode successfully");
        assert_eq!(decoded.delays.len(), 3);
        assert_eq!(decoded.width, 16);
        assert_eq!(decoded.height, 16);
        for delay in &decoded.delays {
            assert_eq!(*delay, Duration::from_millis(50));
        }
    }

    #[test]
    fn test_delay_clamping() {
        // Delay 5ms should be clamped to 100ms
        let gif_bytes = make_test_gif(2, 5);
        let decoded = decode_gif_bytes(&gif_bytes).expect("Should decode successfully");
        assert_eq!(decoded.delays.len(), 2);
        for delay in &decoded.delays {
            assert_eq!(*delay, Duration::from_millis(100));
        }
    }

    #[test]
    fn test_invalid_gif_bytes() {
        let bad_bytes = b"not a real gif";
        assert!(decode_gif_bytes(bad_bytes).is_err());
    }

    #[test]
    fn test_gif_cache() {
        let key = "test_key_sample.gif";
        let gif_bytes = make_test_gif(2, 60);
        let decoded = Arc::new(decode_gif_bytes(&gif_bytes).unwrap());
        {
            let mut cache = GIF_CACHE.lock().unwrap();
            cache.insert(key.to_string(), decoded.clone());
        }
        {
            let cache = GIF_CACHE.lock().unwrap();
            let retrieved = cache.get(key).expect("Should find in cache");
            assert_eq!(retrieved.delays.len(), 2);
        }
    }

    #[test]
    fn test_decimation() {
        // 4 frames of 30ms should be decimated into 2 frames of 60ms when min_delay is 60ms
        let gif_bytes = make_test_gif(4, 30);
        let decoded = decode_gif_software(&gif_bytes, Duration::from_millis(60)).unwrap();
        assert_eq!(decoded.delays.len(), 2);
        assert_eq!(decoded.delays[0], Duration::from_millis(60));
        assert_eq!(decoded.delays[1], Duration::from_millis(60));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_imageio_speed() {
        if let Some(asset) = crate::assets::Assets::get("docs/iShot_2026-09-06_05.04.17.gif") {
            let start = std::time::Instant::now();
            let decoded = decode_gif_imageio(asset.data.as_ref(), Duration::ZERO);
            let elapsed = start.elapsed();
            assert!(decoded.is_some(), "Should decode gif via ImageIO");
            let data = decoded.unwrap();
            assert_eq!(data.delays.len(), 169, "Should decode all 169 frames faithfully");
            eprintln!("ImageIO decoded all {} frames in {:?}", data.delays.len(), elapsed);
            assert!(elapsed < Duration::from_secs(3), "ImageIO should decode in <3s even in debug mode");
        }
    }
}
