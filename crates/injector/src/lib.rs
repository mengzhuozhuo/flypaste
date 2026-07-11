use anyhow::Result;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::Duration;

use clipboard::ClipboardContent;

static ORIGINAL_TEXT: AtomicPtr<Vec<u8>> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_IMAGE: AtomicPtr<arboard::ImageData> = AtomicPtr::new(ptr::null_mut());

/// 将内容注入到焦点窗口（通过模拟键盘粘贴）
pub async fn inject(item: &ClipboardContent) -> Result<()> {
    // 检查内容是否为空（图片文件不存在等情况）
    match item {
        ClipboardContent::Image { width, height, rgba } => {
            if *width == 0 || *height == 0 || rgba.is_empty() {
                log::warn!("Empty image content, skipping inject");
                return Ok(());
            }
        }
        _ => {}
    }

    // 0. 在设置剪贴板之前同步 changeCount，这样 Monitor 就知道这是我们设置的内容
    clipboard::sync_change_count();

    // 1. 保存当前剪贴板内容
    save_original_clipboard();

    // 2. 设置新内容到剪贴板
    set_clipboard_content(item)?;

    // 3. 等待一小段时间确保剪贴板设置完成
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. 模拟 Cmd+V
    simulate_paste()?;

    // 5. 延迟恢复原始剪贴板
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        restore_clipboard();
    });

    Ok(())
}

/// 保存原始剪贴板内容（文本和图片）
fn save_original_clipboard() {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        // 保存文本
        if let Ok(text) = clipboard.get_text() {
            let boxed = Box::new(text.into_bytes());
            ORIGINAL_TEXT.store(Box::into_raw(boxed), Ordering::Relaxed);
        }

        // 保存图片
        if let Ok(img) = clipboard.get_image() {
            let boxed = Box::new(img);
            ORIGINAL_IMAGE.store(Box::into_raw(boxed), Ordering::Relaxed);
        }
    }
}

/// 恢复原始剪贴板内容
fn restore_clipboard() {
    // 先恢复图片（因为图片数据更大，优先恢复）
    let img_ptr = ORIGINAL_IMAGE.swap(ptr::null_mut(), Ordering::Relaxed);
    if !img_ptr.is_null() {
        let img = unsafe { Box::from_raw(img_ptr) };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_image((*img).clone());
        }
    }

    // 再恢复文本
    let text_ptr = ORIGINAL_TEXT.swap(ptr::null_mut(), Ordering::Relaxed);
    if !text_ptr.is_null() {
        let text = unsafe { Box::from_raw(text_ptr) };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(String::from_utf8_lossy(&text).as_ref());
        }
    }
}

/// 设置剪贴板内容
fn set_clipboard_content(content: &ClipboardContent) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;

    match content {
        ClipboardContent::Text { content } => {
            let text = String::from_utf8_lossy(content);
            clipboard.set_text(&*text)?;
        }
        ClipboardContent::Image { width, height, rgba } => {
            let img_data = arboard::ImageData {
                width: *width as usize,
                height: *height as usize,
                bytes: rgba.clone().into(),
            };
            clipboard.set_image(img_data)?;
        }
        ClipboardContent::File { path } => {
            clipboard.set_text(path.as_str())?;
        }
    }

    Ok(())
}

/// 将内容写入系统剪贴板（不模拟粘贴、不保存/恢复原剪贴板）
pub fn copy_to_clipboard(item: &ClipboardContent) -> Result<()> {
    match item {
        ClipboardContent::Image { width, height, rgba } => {
            if *width == 0 || *height == 0 || rgba.is_empty() {
                log::warn!("Empty image content, skipping copy");
                return Ok(());
            }
        }
        _ => {}
    }

    clipboard::sync_change_count();
    set_clipboard_content(item)
}

/// 使用 CGEvent 模拟 Cmd+V
fn simulate_paste() -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow::anyhow!("Failed to create event source"))?;

    // V key = 9
    let key_code: CGKeyCode = 9;

    // Key down event with Command modifier
    let key_down = CGEvent::new_keyboard_event(source.clone(), key_code, true)
        .map_err(|_| anyhow::anyhow!("Failed to create key down event"))?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    // Use Session tap location which works at the session level
    key_down.post(CGEventTapLocation::Session);

    // Key up event
    let key_up = CGEvent::new_keyboard_event(source, key_code, false)
        .map_err(|_| anyhow::anyhow!("Failed to create key up event"))?;
    key_up.post(CGEventTapLocation::Session);

    Ok(())
}
