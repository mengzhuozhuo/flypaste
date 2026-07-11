use clipboard::ClipboardContent;
use image::GenericImageView;
use rusqlite::{Connection as SqliteConn, params, Row};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

pub mod search;

/// Flypaste 数据根目录（`~/Library/Application Support/flypaste` 等）。
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flypaste")
}

/// 图片 content 字段即哈希字符串，拼接磁盘路径。
pub fn image_path_for_hash(images_dir: &Path, hash: &str) -> PathBuf {
    images_dir.join(format!("{hash}.png"))
}

pub fn thumbnail_path_for_hash(thumbnail_dir: &Path, hash: &str) -> PathBuf {
    thumbnail_dir.join(format!("{hash}_thumb.png"))
}

fn load_image_content(hash: &str, images_dir: &Path) -> ClipboardContent {
    let path = image_path_for_hash(images_dir, hash);

    if !path.exists() || path.metadata().map(|m| m.len() == 0).unwrap_or(false) {
        log::warn!("Image file not found or empty: {}", path.display());
        return ClipboardContent::Image {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        };
    }

    if let Ok(img) = image::open(&path) {
        let (width, height) = img.dimensions();
        let rgba = img.to_rgba8().into_raw();
        ClipboardContent::Image { width, height, rgba }
    } else {
        log::warn!("Failed to open image: {}", path.display());
        ClipboardContent::Image {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        }
    }
}

/// 历史记录条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub content_type: String,           // "text" | "image" | "file"
    pub content: String,                // 文本内容 / 图片哈希（16 位 hex）/ 文件路径
    pub content_size: u64,              // 文件大小
    pub created_at: i64,
    /// Bundle ID of the source app (e.g., "com.apple.Safari")
    pub source_app_bundle_id: Option<String>,
    /// 缩略图缓存（仅 image 类型，不存储到数据库）
    #[serde(skip)]
    pub thumbnail_cache: Option<ThumbnailCache>,
}

/// 缩略图缓存
#[derive(Clone, Debug)]
pub struct ThumbnailCache {
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

impl ClipboardItem {
    pub fn content(&self) -> ClipboardContent {
        match self.content_type.as_str() {
            "text" => ClipboardContent::Text {
                content: self.content.as_bytes().to_vec(),
            },
            "image" => load_image_content(&self.content, &data_dir().join("images")),
            "file" => ClipboardContent::File {
                path: self.content.clone(),
            },
            _ => ClipboardContent::Text {
                content: self.content.as_bytes().to_vec(),
            },
        }
    }

    /// 获取文本预览（用于显示）
    pub fn text_preview(&self) -> String {
        if self.content_type == "image" {
            return "[Image]".to_string();
        }
        self.content.clone()
    }

    /// 获取缩略图数据（缓存已预加载，仅返回）
    pub fn thumbnail(&self) -> Option<&ThumbnailCache> {
        if self.content_type != "image" {
            return None;
        }
        self.thumbnail_cache.as_ref()
    }
}

/// 历史记录管理器
pub struct Manager {
    conn: Arc<Mutex<SqliteConn>>,
    next_id: Arc<Mutex<i64>>,
    images_dir: PathBuf,
    thumbnail_dir: PathBuf,
    searcher: Arc<RwLock<Option<search::MmapSearcher>>>,
}

impl Manager {
    /// 创建新的历史记录管理器
    pub fn new() -> Self {
        Self::new_with_path(Self::get_base_dir())
    }

    /// 使用指定路径创建历史记录管理器（主要用于测试）
    pub fn new_with_path(base_dir: PathBuf) -> Self {
        // 确保目录存在
        std::fs::create_dir_all(&base_dir).ok();

        let db_path = base_dir.join("history.db");
        let images_dir = base_dir.join("images");
        let thumbnail_dir = base_dir.join("thumbnails");

        std::fs::create_dir_all(&images_dir).ok();
        std::fs::create_dir_all(&thumbnail_dir).ok();

        let conn = SqliteConn::open(&db_path).expect("Failed to open database");

        // 创建表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS clipboard_items (
                id INTEGER PRIMARY KEY,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                content_size INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                source_app_bundle_id TEXT DEFAULT NULL,
                content_hash TEXT NOT NULL
            )",
            [],
        ).expect("Failed to create table");

        // 创建 created_at 索引（用于按时间排序查询）
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_clipboard_items_created_at ON clipboard_items(created_at DESC)",
            [],
        ).ok();

        // 创建 content_hash 唯一索引（防重复）
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_clipboard_items_content_hash ON clipboard_items(content_hash)",
            [],
        ).ok();

        // 获取当前最大的id
        let next_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM clipboard_items",
            [],
            |row| row.get(0),
        ).unwrap_or(1);

        Self {
            conn: Arc::new(Mutex::new(conn)),
            next_id: Arc::new(Mutex::new(next_id)),
            images_dir,
            thumbnail_dir,
            searcher: Arc::new(RwLock::new(None)),
        }
    }

    /// 加载或构建 mmap 搜索器
    pub fn load_or_build_searcher(&self) {
        let base_dir = data_dir();
        let searcher = search::MmapSearcher::load(&base_dir)
            .or_else(|_| {
                // 如果加载失败（文件不存在或为空），从 DB 构建
                let items = self.get_all_text_items();
                search::MmapSearcher::build_from_db(&items, &base_dir)
            });

        // 检查索引完整性
        if let Ok(ref s) = searcher {
            // 1. validate() 检查 offsets 文件是否损坏
            if !s.validate() {
                log::warn!("Search index validate failed, rebuilding from database");
                std::fs::remove_dir_all(base_dir.join("search_index")).ok();
                let items = self.get_all_text_items();
                *self.searcher.write().unwrap() = search::MmapSearcher::build_from_db(&items, &base_dir).ok();
                return;
            }

            // 2. 数据库最大 id 校验：确保索引包含所有记录
            if let Some(id_max_db) = self.get_max_text_id() {
                if let Some(id_max_idx) = s.max_id() {
                    if id_max_idx < id_max_db {
                        log::warn!(
                            "Search index missing latest records (db_max={}, idx_max={}), rebuilding",
                            id_max_db,
                            id_max_idx
                        );
                        std::fs::remove_dir_all(base_dir.join("search_index")).ok();
                        let items = self.get_all_text_items();
                        *self.searcher.write().unwrap() = search::MmapSearcher::build_from_db(&items, &base_dir).ok();
                        return;
                    }
                }
            }
        }

        *self.searcher.write().unwrap() = searcher.ok();
    }

    /// 获取数据库中 text 类型记录的最大 id
    fn get_max_text_id(&self) -> Option<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT MAX(id) FROM clipboard_items WHERE content_type = 'text'",
            [],
            |row| row.get(0),
        ).ok()
    }

    /// 获取所有文本记录（用于构建 mmap 索引）
    fn get_all_text_items(&self) -> Vec<(i64, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, content FROM clipboard_items WHERE content_type = 'text'"
        ).unwrap();
        let mut results = Vec::new();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let id: i64 = row.get(0).unwrap_or(0);
            if let Ok(content) = row.get::<_, String>(1) {
                    results.push((id, content));
                }
        }
        results
    }

    /// 保存搜索器（程序退出时调用）
    pub fn save_searcher(&self) {
        if let Some(ref s) = *self.searcher.read().unwrap() {
            s.save().ok();
        }
    }

    fn get_base_dir() -> PathBuf {
        data_dir()
    }

    /// 添加新内容到历史记录
    /// source_app_bundle_id: 来源应用的 bundle ID (如 "com.apple.Safari")
    pub async fn add(&self, content: ClipboardContent, source_app_bundle_id: Option<String>) {
        let (content_value, content_type, content_size, content_hash) = match &content {
            ClipboardContent::Text { content } => {
                let hash = Self::compute_text_hash(content);
                let text = String::from_utf8_lossy(content).into_owned();
                (text, "text", content.len() as u64, hash)
            }
            ClipboardContent::Image { rgba, .. } => {
                let hash_str = Self::compute_image_hash(rgba);
                let hash = format!("{:016x}", hash_str);
                let file_size = rgba.len() as u64;
                (hash.clone(), "image", file_size, hash)
            }
            ClipboardContent::File { path } => {
                let hash = Self::compute_text_hash(path.as_bytes());
                (path.clone(), "file", path.len() as u64, hash)
            }
        };

        let created_at: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let id: i64 = {
            let mut next_id = self.next_id.lock().unwrap();
            let id = *next_id;
            *next_id += 1;
            id
        };

        let mut insert_success = false;
        let mut is_duplicate = false;

        // 先插入数据库，检查是否成功
        {
            let conn = self.conn.lock().unwrap();
            let result = conn.execute(
                "INSERT OR IGNORE INTO clipboard_items (id, content_type, content, content_size, created_at, source_app_bundle_id, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, content_type, content_value.clone(), content_size as i64, created_at, source_app_bundle_id, content_hash.clone()],
            );

            match result {
                Ok(rows) if rows > 0 => {
                    insert_success = true;
                }
                Ok(_) => {
                    log::info!(
                        "Duplicate content ignored (hash: {}), moving to front",
                        content_hash
                    );
                    is_duplicate = true;
                }
                Err(e) => {
                    log::warn!("Failed to insert clipboard item: {}", e);
                }
            }
        };

        // 如果是重复内容，更新其时间戳（created_at），使其在列表中置顶，但不更新来源应用
        if is_duplicate {
            let conn = self.conn.lock().unwrap();
            let update_result = conn.execute(
                "UPDATE clipboard_items SET created_at = ?1 WHERE content_hash = ?2",
                params![created_at, content_hash.clone()],
            );
            if let Err(e) = update_result {
                log::warn!("Failed to update duplicate item created_at: {}", e);
            }
        }

        // 只有数据库插入成功时才写入图片文件
        if insert_success && content_type == "image" {
            if let ClipboardContent::Image { rgba, width, height } = &content {
                let hash = content_hash.clone();
                let img_path = image_path_for_hash(&self.images_dir, &hash);

                // 保存原始图片
                if let Some(img) = image::RgbaImage::from_raw(*width, *height, rgba.clone()) {
                    let _ = img.save(&img_path);
                }

                // 生成缩略图
                let thumb_path = thumbnail_path_for_hash(&self.thumbnail_dir, &hash);
                let thumb_data = Self::generate_thumbnail(rgba, *width, *height, 660, 96);
                if !thumb_data.is_empty() {
                    std::fs::write(&thumb_path, &thumb_data).ok();
                }

                // 更新数据库中的文件大小（如果之前计算的不准确）
                let actual_size = std::fs::metadata(&img_path).map(|m| m.len()).unwrap_or(0);
                if actual_size != content_size {
                    let conn = self.conn.lock().unwrap();
                    conn.execute(
                        "UPDATE clipboard_items SET content_size = ?1 WHERE id = ?2",
                        params![actual_size as i64, id],
                    ).ok();
                }
            }
        }

        // 同步追加到 mmap 搜索器（仅当全新插入成功时）
        if insert_success && content_type == "text" {
            if let Some(ref mut s) = *self.searcher.write().unwrap() {
                s.append(id, &content_value).ok();
            }
        }
    }

    /// 计算文本内容的哈希
    fn compute_text_hash(content: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// 计算图片内容的哈希（通过均匀采样分块，兼顾大图性能与哈希准确性）
    fn compute_image_hash(rgba: &[u8]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        
        let len = rgba.len();
        // 首先哈希总长度，防止不同尺寸但采样数据碰巧相同的图片冲突
        len.hash(&mut hasher);

        let chunk_size = 1024;
        let num_chunks = 8; // 采样 8 个 1KB 的块
        
        if len <= chunk_size * num_chunks {
            // 对于小图片（<= 8KB），直接进行全量哈希
            rgba.hash(&mut hasher);
        } else {
            // 对于大图片，均匀抽取 num_chunks 个块进行哈希
            for i in 0..num_chunks {
                let offset = (len - chunk_size) * i / (num_chunks - 1);
                rgba[offset..offset + chunk_size].hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// Generate a PNG thumbnail from RGBA image data (preserves transparency)
    /// Uses Contain logic: image fits within max_width x max_height maintaining aspect ratio
    fn generate_thumbnail(rgba_data: &[u8], width: u32, height: u32, max_width: u32, max_height: u32) -> Vec<u8> {
        if width == 0 || height == 0 || rgba_data.is_empty() {
            return Vec::new();
        }

        // If image is already smaller than max bounds, return it directly without scaling
        if width <= max_width && height <= max_height {
            let img_buffer = match image::RgbaImage::from_raw(width, height, rgba_data.to_vec()) {
                Some(buffer) => buffer,
                None => return Vec::new(),
            };
            let mut png_bytes = Vec::new();
            let mut cursor = Cursor::new(&mut png_bytes);
            if img_buffer.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                return png_bytes;
            }
            return Vec::new();
        }

        // Calculate thumbnail dimensions using Contain logic:
        // fit image within max_width x max_height while maintaining aspect ratio
        let orig_ratio = width as f32 / height as f32;
        let (thumb_width, thumb_height) = if orig_ratio > max_width as f32 / max_height as f32 {
            // Image is wider relative to container
            (max_width, (max_width as f32 / orig_ratio) as u32)
        } else {
            // Image is taller relative to container
            ((max_height as f32 * orig_ratio) as u32, max_height)
        };

        // Ensure at least 1 pixel
        let thumb_width = thumb_width.max(1);
        let thumb_height = thumb_height.max(1);

        // Create RgbaImage from raw RGBA data
        let img_buffer = match image::RgbaImage::from_raw(width, height, rgba_data.to_vec()) {
            Some(buffer) => buffer,
            None => return Vec::new(),
        };

        // Scale down the image using Triangle filter for smoother results
        use image::imageops::FilterType;
        let thumb_buffer = image::imageops::resize(&img_buffer, thumb_width, thumb_height, FilterType::Triangle);

        // Encode to PNG (supports RGBA/transparency)
        let mut png_bytes = Vec::new();
        let mut cursor = Cursor::new(&mut png_bytes);
        if thumb_buffer.write_to(&mut cursor, image::ImageFormat::Png).is_err() {
            return Vec::new();
        }

        png_bytes
    }

    /// Get the database file size in bytes
    pub fn get_db_size(&self) -> u64 {
        let base_dir = Self::get_base_dir();
        let db_path = base_dir.join("history.db");
        std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0)
    }

    /// Total size of stored image files and thumbnails on disk
    pub fn get_images_size(&self) -> u64 {
        Self::dir_files_size(&self.images_dir) + Self::dir_files_size(&self.thumbnail_dir)
    }

    fn dir_files_size(dir: &PathBuf) -> u64 {
        if !dir.exists() {
            return 0;
        }
        walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    }

    /// Get the search index file size in bytes
    pub fn get_index_size(&self) -> u64 {
        let base_dir = Self::get_base_dir();
        let index_dir = base_dir.join("search_index");
        Self::dir_files_size(&index_dir)
    }

    fn remove_image_files_by_hash(&self, hash: &str) {
        let _ = std::fs::remove_file(image_path_for_hash(&self.images_dir, hash));
        let _ = std::fs::remove_file(thumbnail_path_for_hash(&self.thumbnail_dir, hash));
    }

    /// Clean up records older than specified days (text + file share text_days).
    pub async fn cleanup_by_duration(&self, text_days: u32, image_days: u32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let text_cutoff = now - (text_days as i64 * 24 * 60 * 60 * 1000);
        let image_cutoff = now - (image_days as i64 * 24 * 60 * 60 * 1000);

        let conn = self.conn.lock().unwrap();

        conn.execute(
            "DELETE FROM clipboard_items WHERE content_type IN ('text', 'file') AND created_at < ?1",
            params![text_cutoff],
        ).ok();

        let image_hashes: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT content_hash FROM clipboard_items WHERE content_type = 'image' AND created_at < ?1",
            ).unwrap();
            stmt.query_map(params![image_cutoff], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        conn.execute(
            "DELETE FROM clipboard_items WHERE content_type = 'image' AND created_at < ?1",
            params![image_cutoff],
        ).ok();

        drop(conn);

        for hash in image_hashes {
            self.remove_image_files_by_hash(&hash);
        }
    }

    /// 同步获取最近的 N 条记录（支持分页）
    pub fn get_recent_sync(&self, offset: usize, limit: usize) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, content_type, content, content_size, created_at, source_app_bundle_id FROM clipboard_items ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        ).unwrap();

        let mut items: Vec<ClipboardItem> = stmt.query_map([limit as i64, offset as i64], |row: &Row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content_type: row.get(1)?,
                content: row.get(2)?,
                content_size: row.get::<_, i64>(3)? as u64,
                created_at: row.get(4)?,
                source_app_bundle_id: row.get(5).unwrap_or_default(),
                thumbnail_cache: None,
            })
        }).unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // 预加载图片缩略图到缓存
        for item in &mut items.iter_mut() {
            if item.content_type == "image" {
                let thumb_path = thumbnail_path_for_hash(&self.thumbnail_dir, &item.content);

                if thumb_path.exists() {
                    if let Ok((width, height)) = image::image_dimensions(&thumb_path) {
                        if let Ok(data) = std::fs::read(&thumb_path) {
                            item.thumbnail_cache = Some(ThumbnailCache {
                                data: Arc::new(data),
                                width,
                                height,
                            });
                        }
                    }
                }
            }
        }

        items
    }

    /// 获取总记录数
    pub fn get_total_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// 数据库级别搜索（使用 LIKE）
    pub fn search_in_db(&self, query: &str, limit: usize) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();

        // LIKE 搜索：在数据库中执行
        let like_query = format!("%{}%", query.to_lowercase());
        let limit_i64 = limit as i64;

        let mut stmt = match conn.prepare(
            "SELECT id, content_type, content, content_size, created_at, source_app_bundle_id
             FROM clipboard_items WHERE LOWER(content) LIKE ?1 ORDER BY created_at DESC LIMIT ?2"
        ) {
            Ok(stmt) => stmt,
            Err(_) => return vec![],
        };

        let rows = stmt.query_map(rusqlite::params![like_query, limit_i64], |row| {
            Ok(ClipboardItem {
                id: row.get(0)?,
                content_type: row.get(1)?,
                content: row.get(2)?,
                content_size: row.get::<_, i64>(3)? as u64,
                created_at: row.get(4)?,
                source_app_bundle_id: row.get(5).unwrap_or_default(),
                thumbnail_cache: None,
            })
        });

        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// 根据 ID 获取 content（用于粘贴）
    pub fn get_content_by_id(&self, id: i64) -> ClipboardContent {
        let conn = self.conn.lock().unwrap();
        let result: rusqlite::Result<(String, String)> = conn.query_row(
            "SELECT content_type, content FROM clipboard_items WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok((content_type, content)) => {
                match content_type.as_str() {
                    "text" => ClipboardContent::Text {
                        content: content.into_bytes(),
                    },
                    "image" => load_image_content(&content, &self.images_dir),
                    "file" => ClipboardContent::File { path: content },
                    _ => ClipboardContent::Text {
                        content: content.into_bytes(),
                    },
                }
            }
            Err(_) => ClipboardContent::Text { content: Vec::new() },
        }
    }

    /// 删除指定记录（图片类型同时删除磁盘上的原图与缩略图）
    pub async fn delete(&self, id: i64) {
        let image_hash = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT content_hash FROM clipboard_items WHERE id = ?1 AND content_type = 'image'",
                [id],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };

        if let Some(hash) = image_hash {
            self.remove_image_files_by_hash(&hash);
        }

        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM clipboard_items WHERE id = ?1", [id]).ok();
    }

    /// 将指定记录移动到第一位（更新 created_at）
    pub async fn move_to_front(&self, id: i64) {
        let created_at: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET created_at = ?1 WHERE id = ?2",
            params![created_at, id],
        ).ok();
    }

    /// 删除所有本地历史数据文件（数据库、图片、索引），供「清除并退出」使用。
    pub fn wipe_all_data(&self) {
        let base_dir = Self::get_base_dir();

        *self.searcher.write().unwrap() = None;

        let _ = std::fs::remove_dir_all(&self.images_dir);
        let _ = std::fs::remove_dir_all(&self.thumbnail_dir);
        let _ = std::fs::remove_dir_all(base_dir.join("search_index"));

        for name in ["history.db", "history.db-wal", "history.db-shm"] {
            let _ = std::fs::remove_file(base_dir.join(name));
        }
    }

    /// Rebuild search index from scratch
    pub fn rebuild_index(&self) {
        let base_dir = Self::get_base_dir();
        
        // Force drop and close existing searcher
        {
            let mut searcher = self.searcher.write().unwrap();
            *searcher = None;
        }

        // Remove existing index directory
        let _ = std::fs::remove_dir_all(base_dir.join("search_index"));
        
        // Rebuild from DB
        self.load_or_build_searcher();
    }

    /// 使用 mmap 搜索历史记录（统一搜索接口）
    /// use_regex: 是否使用正则模式
    pub async fn search(&self, query: &str, use_regex: bool, limit: usize, case_sensitive: bool) -> Vec<SearchResult> {
        let searcher = self.searcher.read().unwrap();
        if let Some(ref s) = *searcher {
            let raw_results = if use_regex {
                s.search_regex(query, limit, case_sensitive).unwrap_or_default()
            } else {
                s.search_text(query, limit, case_sensitive).unwrap_or_default()
            };

            if raw_results.is_empty() {
                return vec![];
            }

            let ids: Vec<i64> = raw_results.iter().map(|(id, _, _, _)| *id).collect();
            let ranges: std::collections::HashMap<i64, (usize, usize)> = raw_results.iter()
                .map(|(id, match_abs_start, match_abs_end, item_offset)| {
                    (*id, (*match_abs_start - *item_offset, *match_abs_end - *item_offset))
                })
                .collect();

            drop(searcher);
            let items = self.get_by_ids(ids).await;

            items.into_iter()
                .map(|item| {
                    let range = ranges.get(&item.id).copied();
                    SearchResult { item, match_range: range }
                })
                .collect()
        } else {
            vec![]
        }
    }

    /// 同步搜索接口（用于 GPUI 等同步上下文）
    pub fn blocking_search(&self, query: &str, use_regex: bool, limit: usize, case_sensitive: bool) -> Vec<SearchResult> {
        let searcher = self.searcher.read().unwrap();
        if let Some(ref s) = *searcher {
            let raw_results = if use_regex {
                s.search_regex(query, limit, case_sensitive).unwrap_or_default()
            } else {
                s.search_text(query, limit, case_sensitive).unwrap_or_default()
            };

            if raw_results.is_empty() {
                return vec![];
            }

            // 提取 ids 和对应的相对匹配范围
            let ids: Vec<i64> = raw_results.iter().map(|(id, _, _, _)| *id).collect();
            let ranges: std::collections::HashMap<i64, (usize, usize)> = raw_results.iter()
                .map(|(id, match_abs_start, match_abs_end, item_offset)| {
                    (*id, (*match_abs_start - *item_offset, *match_abs_end - *item_offset))
                })
                .collect();

            drop(searcher);
            let items = self.blocking_get_by_ids(ids);

            // 按 created_at 降序排序，最新的在最前面
            let mut results: Vec<SearchResult> = items.into_iter()
                .map(|item| {
                    let range = ranges.get(&item.id).copied();
                    SearchResult { item, match_range: range }
                })
                .collect();
            results.sort_by(|a, b| b.item.created_at.cmp(&a.item.created_at));

            results
        } else {
            vec![]
        }
    }

    /// 根据 ID 列表获取 ClipboardItem（最多 200 条）
    async fn get_by_ids(&self, ids: Vec<i64>) -> Vec<ClipboardItem> {
        if ids.is_empty() {
            return vec![];
        }
        const BATCH_SIZE: usize = 100;
        let mut results = Vec::new();

        for chunk in ids.chunks(BATCH_SIZE) {
            let conn = self.conn.lock().unwrap();
            let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
            let query = format!(
                "SELECT id, content_type, content, content_size, created_at, source_app_bundle_id
                 FROM clipboard_items WHERE id IN ({})",
                placeholders.join(",")
            );
            let mut stmt = match conn.prepare(&query) {
                Ok(stmt) => stmt,
                Err(_) => continue,
            };

            let rows = stmt.query_map(rusqlite::params_from_iter(chunk), |row: &Row| {
                Ok(ClipboardItem {
                    id: row.get(0)?,
                    content_type: row.get(1)?,
                    content: row.get(2)?,
                    content_size: row.get::<_, i64>(3)? as u64,
                    created_at: row.get(4)?,
                    source_app_bundle_id: row.get(5).unwrap_or_default(),
                    thumbnail_cache: None,
                })
            });

            if let Ok(rows) = rows {
                for item in rows.filter_map(|r| r.ok()) {
                    results.push(item);
                }
            }
        }
        results
    }

    /// 同步版本的 get_by_ids（用于 GPUI 等同步上下文）
    fn blocking_get_by_ids(&self, ids: Vec<i64>) -> Vec<ClipboardItem> {
        if ids.is_empty() {
            return vec![];
        }
        const BATCH_SIZE: usize = 100;
        let mut results = Vec::new();

        for chunk in ids.chunks(BATCH_SIZE) {
            let conn = self.conn.lock().unwrap();
            let placeholders: Vec<String> = chunk.iter().map(|_| "?".to_string()).collect();
            let query = format!(
                "SELECT id, content_type, content, content_size, created_at, source_app_bundle_id
                 FROM clipboard_items WHERE id IN ({})",
                placeholders.join(",")
            );
            let mut stmt = match conn.prepare(&query) {
                Ok(stmt) => stmt,
                Err(_) => continue,
            };

            let rows = stmt.query_map(rusqlite::params_from_iter(chunk), |row: &Row| {
                Ok(ClipboardItem {
                    id: row.get(0)?,
                    content_type: row.get(1)?,
                    content: row.get(2)?,
                    content_size: row.get::<_, i64>(3)? as u64,
                    created_at: row.get(4)?,
                    source_app_bundle_id: row.get(5).unwrap_or_default(),
                    thumbnail_cache: None,
                })
            });

            if let Ok(rows) = rows {
                for item in rows.filter_map(|r| r.ok()) {
                    results.push(item);
                }
            }
        }
        results
    }
}

/// 搜索结果
#[derive(Clone)]
pub struct SearchResult {
    pub item: ClipboardItem,
    /// 匹配范围 (start, end) 字节索引，基于 item 内容计算
    pub match_range: Option<(usize, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_text_content() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let conn = rusqlite::Connection::open(&db_path).unwrap();

        conn.execute(
            "CREATE TABLE clipboard_items (
                id INTEGER PRIMARY KEY,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                content_size INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                source_app_bundle_id TEXT DEFAULT NULL
            )",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO clipboard_items (id, content_type, content, content_size, created_at) VALUES (?1, 'text', ?2, ?3, ?4)",
            rusqlite::params![1i64, "hello world", 11i64, 1000i64],
        ).unwrap();
        conn.execute(
            "INSERT INTO clipboard_items (id, content_type, content, content_size, created_at) VALUES (?1, 'text', ?2, ?3, ?4)",
            rusqlite::params![2i64, "rust is awesome", 15i64, 2000i64],
        ).unwrap();
        conn.execute(
            "INSERT INTO clipboard_items (id, content_type, content, content_size, created_at) VALUES (?1, 'text', ?2, ?3, ?4)",
            rusqlite::params![3i64, "中文内容", 12i64, 3000i64],
        ).unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, content FROM clipboard_items WHERE content_type = 'text'"
        ).unwrap();

        let mut results: Vec<(i64, String)> = Vec::new();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let id: i64 = row.get(0).unwrap();
            if let Ok(content) = row.get::<_, String>(1) {
                results.push((id, content));
            }
        }

        assert_eq!(results.len(), 3);

        let item_map: std::collections::HashMap<i64, String> = results.into_iter().collect();
        assert_eq!(item_map.get(&1), Some(&"hello world".to_string()));
        assert_eq!(item_map.get(&2), Some(&"rust is awesome".to_string()));
        assert_eq!(item_map.get(&3), Some(&"中文内容".to_string()));
    }

    #[test]
    fn test_build_from_db_with_utf8_strings() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        // 模拟从 SQLite TEXT 列读取的数据
        let items: Vec<(i64, String)> = vec![
            (1i64, "hello world".to_string()),
            (2i64, "中文内容".to_string()),
            (3i64, "joyscale-region".to_string()),
        ];

        let searcher = search::MmapSearcher::build_from_db(&items, temp_dir.path()).unwrap();

        assert_eq!(searcher.len(), 3);

        // "hello world" = 11 字节, "\n\0\n" = 3 字节, offset = 0
        // "中文内容" = 12 字节 (4 汉字 * 3), "\n\0\n" = 3 字节, offset = 14
        // "joyscale-region" = 15 字节, "\n\0\n" = 3 字节, offset = 29

        // 验证搜索 "hello" - "hello world" 中 "hello" 位置 0..5
        let results = searcher.search_text("hello", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 0); // "hello" 从 0 开始
        assert_eq!(match_end - item_offset, 5);   // "hello" 长度 5

        // 验证搜索 "中文" - UTF-8 每个汉字 3 字节
        let results = searcher.search_text("中文", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 2);
        assert_eq!(item_offset, 14);
        assert_eq!(match_start - item_offset, 0); // "中" 从 0 开始
        assert_eq!(match_end - item_offset, 6);  // "中文" 长度 6 字节

        // 验证正则 ".-" - 匹配 "e-" (位置 7..9)
        let results = searcher.search_regex(".-", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 3);
        assert_eq!(item_offset, 29);
        assert_eq!(match_start - item_offset, 7); // "e-" 从 7 开始
        assert_eq!(match_end - item_offset, 9);   // "e-" 长度 2
    }

    #[test]
    fn test_image_cleanup_removes_files_by_hash() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = Manager::new_with_path(temp_dir.path().to_path_buf());

        let hash = "aabbccdd11223344";
        let images_dir = temp_dir.path().join("images");
        let thumbnails_dir = temp_dir.path().join("thumbnails");
        let image_path = image_path_for_hash(&images_dir, hash);
        let thumb_path = thumbnail_path_for_hash(&thumbnails_dir, hash);
        std::fs::write(&image_path, b"fake-png").unwrap();
        std::fs::write(&thumb_path, b"fake-thumb").unwrap();

        let old_created_at: i64 = 1_000;
        {
            let conn = manager.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO clipboard_items (id, content_type, content, content_size, created_at, content_hash) VALUES (?1, 'image', ?2, ?3, ?4, ?5)",
                params![1i64, hash, 8i64, old_created_at, hash],
            )
            .unwrap();
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            manager.cleanup_by_duration(30, 1).await;
        });

        assert!(!image_path.exists());
        assert!(!thumb_path.exists());

        let count: i64 = {
            let conn = manager.conn.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(count, 0);
    }

    #[test]
    fn test_duplicate_content_moves_to_front() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = Manager::new_with_path(temp_dir.path().to_path_buf());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            // 插入第一个文本项
            let content1 = ClipboardContent::Text {
                content: "unique content 1".as_bytes().to_vec(),
            };
            manager.add(content1.clone(), Some("app.a".to_string())).await;

            // 延迟一下，插入第二个文本项
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let content2 = ClipboardContent::Text {
                content: "unique content 2".as_bytes().to_vec(),
            };
            manager.add(content2.clone(), Some("app.b".to_string())).await;

            // 获取最近的列表，确认排序是: [content2, content1]
            let items = manager.get_recent_sync(0, 10);
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].content, "unique content 2");
            assert_eq!(items[1].content, "unique content 1");

            // 再次插入第一个文本项（重复内容，来自不同的 App 且有更新的时间戳）
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            manager.add(content1.clone(), Some("app.c".to_string())).await;

            // 再次获取最近的列表，由于第一个内容已经存在，它应该被移动到了第一位！
            let items_after = manager.get_recent_sync(0, 10);
            // 总数仍应为 2 (防重复)
            assert_eq!(items_after.len(), 2);
            // 排序应该变成: [content1, content2]，由于实现中不更新来源应用，source_app 应该仍为 "app.a"
            assert_eq!(items_after[0].content, "unique content 1");
            assert_eq!(items_after[0].source_app_bundle_id, Some("app.a".to_string()));
            assert_eq!(items_after[1].content, "unique content 2");
        });
    }

    #[test]
    fn test_image_thumbnail_load() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = Manager::new_with_path(temp_dir.path().to_path_buf());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            // 创建一个 4x4 的全红像素测试图片数据
            let width = 4;
            let height = 4;
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for _ in 0..(width * height) {
                rgba.extend_from_slice(&[255, 0, 0, 255]);
            }
            
            let content = ClipboardContent::Image { width, height, rgba };
            manager.add(content, None).await;

            // 获取最近的列表
            let items = manager.get_recent_sync(0, 10);
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item.content_type, "image");
            
            // 验证 thumbnail_cache 是否成功生成并且尺寸和数据正确
            let cache = item.thumbnail_cache.as_ref().expect("Thumbnail cache should be loaded");
            assert_eq!(cache.width, 4);
            assert_eq!(cache.height, 4);
            assert!(cache.data.len() > 0);
        });
    }
}
