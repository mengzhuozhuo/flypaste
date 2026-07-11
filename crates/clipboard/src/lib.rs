#![allow(unexpected_cfgs)]

use arboard::Clipboard;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// 共享的剪贴板 changeCount（用于检测是否有新内容）
static LAST_CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

/// 获取当前 NSPasteboard changeCount
#[cfg(target_os = "macos")]
fn get_change_count() -> u64 {
    use cocoa::foundation::NSAutoreleasePool;
    use cocoa::base::nil;

    unsafe {
        let pool = NSAutoreleasePool::new(nil);
        let cls = objc::runtime::Class::get("NSPasteboard").unwrap();
        let pb: *mut objc::runtime::Object = msg_send![cls, generalPasteboard];
        let count: u64 = msg_send![pb, changeCount];
        pool.drain();
        count
    }
}

/// 在 inject 设置剪贴板内容后调用，同步 changeCount
/// 这样 Monitor 下次 poll 就知道这是我们设置的内容
pub fn sync_change_count() {
    #[cfg(target_os = "macos")]
    {
        LAST_CHANGE_COUNT.store(get_change_count(), Ordering::SeqCst);
    }
}

/// 剪贴板内容类型
#[derive(Debug, Clone)]
pub enum ClipboardContent {
    Text {
        content: Vec<u8>,  // UTF-8 文本内容
    },
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    File {
        path: String,  // 文件路径
    },
}

impl ClipboardContent {
    /// 获取纯文本预览
    pub fn preview(&self) -> String {
        match self {
            ClipboardContent::Text { content } => {
                String::from_utf8_lossy(content).trim().to_string()
            }
            ClipboardContent::Image { width, height, .. } => {
                format!("[Image {}x{}]", width, height)
            }
            ClipboardContent::File { path } => path.clone(),
        }
    }

    /// 获取内容字节大小
    pub fn size(&self) -> usize {
        match self {
            ClipboardContent::Text { content } => content.len(),
            ClipboardContent::Image { rgba, .. } => rgba.len(),
            ClipboardContent::File { path } => path.len(),
        }
    }
}

/// 剪贴板监控器
pub struct Monitor {
    clipboard: Arc<std::sync::Mutex<Clipboard>>,
}

impl Monitor {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            clipboard: Arc::new(std::sync::Mutex::new(Clipboard::new()?)),
        })
    }

    /// 轮询剪贴板，检查是否有新内容
    pub async fn poll(&mut self) -> Option<ClipboardContent> {
        // 使用 changeCount 检测剪贴板是否有新变化
        #[cfg(target_os = "macos")]
        {
            let current_count = get_change_count();
            if current_count == LAST_CHANGE_COUNT.load(Ordering::SeqCst) {
                return None;  // 没有变化，跳过
            }
            LAST_CHANGE_COUNT.store(current_count, Ordering::SeqCst);
        }

        let mut clipboard = self.clipboard.lock().unwrap();

        // 1. 优先尝试获取文件路径 (macOS 专用逻辑)
        #[cfg(target_os = "macos")]
        {
            if let Some(path) = get_macos_file_url() {
                // 直接返回，让数据库去重
                return Some(ClipboardContent::File { path });
            }
        }

        // 2. 尝试获取图片
        if let Ok(img) = clipboard.get_image() {
            return Some(ClipboardContent::Image {
                width: img.width as u32,
                height: img.height as u32,
                rgba: img.bytes.into_owned(),
            });
        }

        // 3. 尝试获取文本
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                return Some(ClipboardContent::Text { content: text.into_bytes() });
            }
        }

        None
    }
}

#[cfg(target_os = "macos")]
fn get_macos_file_url() -> Option<String> {
    use cocoa::foundation::NSAutoreleasePool;

    unsafe {
        let pool = NSAutoreleasePool::new(cocoa::base::nil);
        let cls = objc::runtime::Class::get("NSPasteboard").unwrap();
        let pb: *mut objc::runtime::Object = msg_send![cls, generalPasteboard];

        // Check for URLs
        let url_type: *mut objc::runtime::Object = msg_send![objc::runtime::Class::get("NSString").unwrap(), stringWithUTF8String: "public.file-url\0".as_ptr() as *const _];
        let types: *mut objc::runtime::Object = msg_send![pb, types];
        let contains: bool = msg_send![types, containsObject: url_type];

        if !contains {
            let _: () = msg_send![pool, release];
            return None;
        }

        let cls_url = objc::runtime::Class::get("NSURL").unwrap();
        let cls_array = objc::runtime::Class::get("NSArray").unwrap();
        let dict: *mut objc::runtime::Object = msg_send![objc::runtime::Class::get("NSDictionary").unwrap(), dictionary];
        let classes: *mut objc::runtime::Object = msg_send![cls_array, arrayWithObject: cls_url];
        let urls: *mut objc::runtime::Object = msg_send![pb, readObjectsForClasses: classes options: dict];

        if urls.is_null() {
            let _: () = msg_send![pool, release];
            return None;
        }

        let count: usize = msg_send![urls, count];
        if count == 0 {
            let _: () = msg_send![pool, release];
            return None;
        }

        // 收集所有文件路径，用换行连接
        let mut paths = Vec::with_capacity(count);
        for i in 0..count {
            let url: *mut objc::runtime::Object = msg_send![urls, objectAtIndex: i];
            // Use filePathURL to convert file-reference URLs to path-based URLs
            let path_url: *mut objc::runtime::Object = msg_send![url, filePathURL];
            let final_url = if path_url.is_null() { url } else { path_url };
            let path_str: *mut objc::runtime::Object = msg_send![final_url, absoluteString];
            let c_str: *const std::ffi::c_char = msg_send![path_str, UTF8String];
            if !c_str.is_null() {
                paths.push(std::ffi::CStr::from_ptr(c_str).to_string_lossy().to_string());
            }
        }

        let result = if paths.is_empty() {
            None
        } else {
            Some(paths.join("\n"))
        };

        let _: () = msg_send![pool, release];
        result
    }
}
