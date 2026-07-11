//! mmap 文本搜索模块
//!
//! 核心搜索逻辑：使用 mmap 文件存储纯文本，用 \0 分隔，
//! 配合偏移量数组实现高速正则搜索。

use memmap2::Mmap;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 内存映射搜索器
///
/// 使用 mmap 文件存储 append-only 的纯文本记录，
/// 内存中维护 (id, byte_offset) 偏移量数组用于二分查找。
pub struct MmapSearcher {
    mmap: Mmap,
    offset_index: Vec<(i64, usize)>,
    file: Arc<Mutex<File>>,
    offsets_path: PathBuf,
}

/// 搜索结果
#[derive(Debug)]
pub struct SearchResult {
    /// 匹配的 SQLite ID
    pub id: i64,
    /// 匹配的字节偏移量
    pub offset: usize,
}

impl MmapSearcher {
    /// 创建新的搜索器（会创建新的 mmap 文件）
    pub fn new(base_dir: &Path) -> std::io::Result<Self> {
        let index_dir = base_dir.join("search_index");
        std::fs::create_dir_all(&index_dir)?;

        let index_path = index_dir.join("content.bin");
        let offsets_path = index_dir.join("offsets.bin");

        // 创建空的 mmap 文件
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&index_path)?;

        // 创建空的偏移量索引文件
        File::create(&offsets_path)?;

        let mmap = unsafe { Mmap::map(&file) }?;
        let file = Arc::new(Mutex::new(file));

        Ok(Self {
            mmap,
            offset_index: Vec::new(),
            file,
            offsets_path,
        })
    }

    /// 从磁盘加载已有的 mmap 文件和偏移量索引
    pub fn load(base_dir: &Path) -> std::io::Result<Self> {
        let index_dir = base_dir.join("search_index");
        let index_path = index_dir.join("content.bin");
        let offsets_path = index_dir.join("offsets.bin");

        // 打开 mmap 文件
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&index_path)?;
        let mmap = unsafe { Mmap::map(&file) }?;
        let file = Arc::new(Mutex::new(file));

        // 加载偏移量索引
        let offset_index = Self::load_offsets(&offsets_path)?;

        Ok(Self {
            mmap,
            offset_index,
            file,
            offsets_path,
        })
    }

    /// 从 DB 数据构建新索引（当 mmap 文件不存在时调用）
    pub fn build_from_db(items: &[(i64, String)], base_dir: &Path) -> std::io::Result<Self> {
        let mut searcher = Self::new(base_dir)?;

        // 追加所有文本
        for &(id, ref content) in items {
            searcher.append(id, content)?;
        }

        // 保存偏移量索引
        searcher.save()?;

        Ok(searcher)
    }

    /// 追加新文本到 mmap 文件（同时更新 offset_index）
    /// 使用 \n\0\n 作为分隔符，方便 ^ 和 $ 锚点匹配
    /// 注意：不调用 sync_all()，mmap 持久化由系统自动管理
    pub fn append(&mut self, id: i64, content: &str) -> std::io::Result<()> {
        let offset = self.mmap.len();

        // 追加到文件：内容 + 分隔符 \n\0\n
        let mut file = self.file.lock().unwrap();
        file.seek(std::io::SeekFrom::End(0))?;
        file.write_all(content.as_bytes())?;
        file.write_all(b"\n\0\n")?;

        // 重新映射（文件已变大）
        let new_mmap = unsafe { Mmap::map(&*file)? };
        self.mmap = new_mmap;

        // 更新偏移量数组
        self.offset_index.push((id, offset));

        Ok(())
    }

    /// 使用正则扫描，返回匹配的 (id, 匹配起始偏移, 匹配结束偏移, item起始偏移)（最多返回 limit 条）
    /// 自动添加 (?m) 多行模式，让 ^ 和 $ 匹配每行的首尾
    /// case_sensitive: false 时自动添加 (?i) 前缀
    pub fn search_regex(&self, pattern: &str, limit: usize, case_sensitive: bool) -> Result<Vec<(i64, usize, usize, usize)>, regex::Error> {
        let limit = limit.min(200);

        let pattern_with_flags = if pattern.contains("(?m)") || pattern.contains("(?-m)") {
            pattern.to_string()
        } else {
            format!("(?m){}", pattern)
        };

        let pattern_with_flags = if case_sensitive {
            pattern_with_flags
        } else if pattern_with_flags.contains("(?i)") || pattern_with_flags.contains("(?-i)") {
            pattern_with_flags
        } else {
            format!("(?i){}", pattern_with_flags)
        };

        let regex = regex::Regex::new(&pattern_with_flags)?;
        let bytes = self.mmap.as_ref();
        let content = std::str::from_utf8(bytes).map_err(|_| regex::Error::Syntax("Invalid UTF-8".to_string()))?;
        let mut matched = Vec::new();
        let mut seen = HashSet::new();

        for cap in regex.find_iter(content) {
            if let Some((id, item_offset)) = self.find_id_and_offset_by_position(cap.start()) {
                if seen.insert(id) {
                    matched.push((id, cap.start(), cap.end(), item_offset));
                    if matched.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(matched)
    }

    /// 子串搜索，返回 (id, 匹配起始偏移, 匹配结束偏移, item起始偏移)
    pub fn search_text(&self, query: &str, limit: usize, case_sensitive: bool) -> Result<Vec<(i64, usize, usize, usize)>, regex::Error> {
        let pattern = if case_sensitive {
            regex::escape(query)
        } else {
            format!("(?i){}", regex::escape(query))
        };
        let regex = regex::Regex::new(&pattern)?;
        let bytes = self.mmap.as_ref();
        let content = std::str::from_utf8(bytes).map_err(|_| regex::Error::Syntax("Invalid UTF-8".to_string()))?;
        let mut matched = Vec::new();
        let mut seen = HashSet::new();

        for cap in regex.find_iter(content) {
            if let Some((id, item_offset)) = self.find_id_and_offset_by_position(cap.start()) {
                if seen.insert(id) {
                    matched.push((id, cap.start(), cap.end(), item_offset));
                    if matched.len() >= limit.min(200) {
                        break;
                    }
                }
            }
        }

        Ok(matched)
    }

    /// 保存偏移量索引到磁盘
    pub fn save(&self) -> std::io::Result<()> {
        let mut file = File::create(&self.offsets_path)?;

        for &(id, offset) in &self.offset_index {
            // 每条记录: 8 bytes (i64 id) + 8 bytes (u64 offset)
            file.write_all(&id.to_le_bytes())?;
            file.write_all(&(offset as u64).to_le_bytes())?;
        }

        file.flush()?;
        log::info!("Search index saved: {} records", self.offset_index.len());
        Ok(())
    }

    /// 加载偏移量索引从磁盘
    fn load_offsets(path: &Path) -> std::io::Result<Vec<(i64, usize)>> {
        let mut file = File::open(path)?;
        let mut offsets = Vec::new();

        let mut buf = [0u8; 16]; // 16 bytes per record
        while std::io::Read::read(&mut file, &mut buf)? == 16 {
            let id = i64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]);
            let offset = u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]) as usize;
            offsets.push((id, offset));
        }

        Ok(offsets)
    }

    /// 获取当前索引的记录数
    pub fn len(&self) -> usize {
        self.offset_index.len()
    }

    /// 获取索引中最大的记录 id
    pub fn max_id(&self) -> Option<i64> {
        self.offset_index.last().map(|(id, _)| *id)
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.offset_index.is_empty()
    }

    /// 验证 offsets 索引的有效性（检测 offsets 文件损坏）
    /// 注意：不验证 mmap 内容完整性，该检查由 Manager 层的数据库 id 校验负责
    pub fn validate(&self) -> bool {
        if self.offset_index.is_empty() {
            return self.mmap.is_empty();
        }

        // 检查 1: offsets 文件大小是否与 offset_index 匹配
        if let Ok(metadata) = std::fs::metadata(&self.offsets_path) {
            let expected_size = self.offset_index.len() * 16;
            if metadata.len() as usize != expected_size {
                log::warn!(
                    "validate failed: offsets file size mismatch (file={}, expected={})",
                    metadata.len(),
                    expected_size
                );
                return false;
            }
        }

        // 检查 2: 验证最大 offset 在 mmap 范围内（防止 content.bin 数据丢失）
        if let Some((_, max_offset)) = self.offset_index.last() {
            if *max_offset >= self.mmap.len() {
                log::warn!(
                    "validate failed: max_offset {} >= mmap.len() {}",
                    max_offset,
                    self.mmap.len()
                );
                return false;
            }
        }

        true
    }

    /// 在 offset_index 中查找匹配字节坐标对应的 SQLite ID 和该 item 的起始偏移
    /// 找 "<= offset 的最大起始位置" 对应的 id
    fn find_id_and_offset_by_position(&self, target_offset: usize) -> Option<(i64, usize)> {
        if self.offset_index.is_empty() {
            return None;
        }

        let mut lo = 0;
        let mut hi = self.offset_index.len();

        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.offset_index[mid].1 <= target_offset {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo > 0 {
            let (id, offset) = self.offset_index[lo - 1];
            Some((id, offset))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_new_and_append() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        assert_eq!(searcher.len(), 0);
        assert!(searcher.is_empty());

        searcher.append(1, "hello").unwrap();
        assert_eq!(searcher.len(), 1);
        assert!(!searcher.is_empty());

        // 验证 mmap 内容："hello" + "\n\0\n" = 5 + 3 = 8
        let bytes = searcher.mmap.as_ref();
        assert_eq!(bytes.len(), 8);

        searcher.append(2, "world").unwrap();
        assert_eq!(searcher.len(), 2);

        // 验证 mmap 内容："hello" + "\n\0\n" + "world" + "\n\0\n" = 5 + 3 + 5 + 3 = 16
        let bytes = searcher.mmap.as_ref();
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn test_search_text() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // "hello world" = 11 字节, offset = 0
        // "rust is awesome" = 15 字节, offset = 14
        // "hello rust" = 10 字节, offset = 32
        searcher.append(1, "hello world").unwrap();
        searcher.append(2, "rust is awesome").unwrap();
        searcher.append(3, "hello rust").unwrap();

        // 子串搜索 "hello" - 匹配 id=1 和 id=3
        let results = searcher.search_text("hello", 10, false).unwrap();
        assert_eq!(results.len(), 2);

        // 验证第一个结果 (id=1): "hello" 在 "hello world" 中位置 0..5
        let (id, match_start, match_end, item_offset) = results.iter().find(|(id, _, _, _)| *id == 1).unwrap();
        assert_eq!(*id, 1);
        assert_eq!(*item_offset, 0);
        assert_eq!(*match_start - *item_offset, 0); // "hello" 从 0 开始
        assert_eq!(*match_end - *item_offset, 5);   // "hello" 长度 5

        // 验证第二个结果 (id=3): "hello" 在 "hello rust" 中位置 0..5
        let (id, match_start, match_end, item_offset) = results.iter().find(|(id, _, _, _)| *id == 3).unwrap();
        assert_eq!(*id, 3);
        assert_eq!(*item_offset, 32);
        assert_eq!(*match_start - *item_offset, 0);
        assert_eq!(*match_end - *item_offset, 5);

        // 子串搜索 "rust" - 匹配 id=2 和 id=3
        let results = searcher.search_text("rust", 10, false).unwrap();
        assert_eq!(results.len(), 2);

        // 验证 id=2 的范围: "rust" 在 "rust is awesome" 中位置 0..4
        let (id, match_start, match_end, item_offset) = results.iter().find(|(id, _, _, _)| *id == 2).unwrap();
        assert_eq!(*id, 2);
        assert_eq!(*item_offset, 14);
        assert_eq!(*match_start - *item_offset, 0); // "rust" 从 0 开始
        assert_eq!(*match_end - *item_offset, 4);   // "rust" 长度 4

        // 子串搜索 "xyz" (不存在)
        let results = searcher.search_text("xyz", 10, false).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_regex() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // "hello world" = 11 字节, offset = 0
        // "rust is awesome" = 15 字节, offset = 14
        // "hello rust" = 10 字节, offset = 32
        searcher.append(1, "hello world").unwrap();
        searcher.append(2, "rust is awesome").unwrap();
        searcher.append(3, "hello rust").unwrap();

        // "hello.*" 匹配 "hello" 开头，后接任意非换行符直到 entry 边界
        // 所以 "hello world" (id=1) 和 "hello rust" (id=3) 都会匹配
        let results = searcher.search_regex("hello.*", 10, false).unwrap();
        assert_eq!(results.len(), 2);
        let ids: Vec<i64> = results.iter().map(|(id, _, _, _)| *id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));

        // hello.*rust 匹配 "hello" 后跟任意非换行符直到遇到 "rust"
        // 只匹配 "hello rust" (id=3)，因为 "hello world" 里 "hello" 后面是 " world"
        let results = searcher.search_regex("hello.*rust", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 3);
        assert_eq!(item_offset, 32);
        assert_eq!(match_start - item_offset, 0);   // "hello" 从 0 开始
        assert_eq!(match_end - item_offset, 10);   // "hello rust" 长度 10
    }

    #[test]
    fn test_binary_search_offset() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // "ab" + "\n\0\n" = 2 + 3 = 5 bytes, offset = 0
        // "cd" + "\n\0\n" = 2 + 3 = 5 bytes, offset = 5
        searcher.append(1, "ab").unwrap();
        searcher.append(2, "cd").unwrap();

        // 搜索 "c" (offset 5)，应该返回 id 2
        let results = searcher.search_text("c", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 2);
        assert_eq!(item_offset, 5);
        assert_eq!(match_start - item_offset, 0); // "c" 在 "cd" 中位置 0
        assert_eq!(match_end - item_offset, 1);   // "c" 长度 1

        // 搜索 "d" (offset 6)，应该返回 id 2
        let results = searcher.search_text("d", 10, false).unwrap();
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 2);
        assert_eq!(item_offset, 5);
        assert_eq!(match_start - item_offset, 1); // "d" 在 "cd" 中位置 1
        assert_eq!(match_end - item_offset, 2);

        // 搜索 "a" (offset 0)，应该返回 id 1
        let results = searcher.search_text("a", 10, false).unwrap();
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 0);
        assert_eq!(match_end - item_offset, 1);

        // 搜索 "b" (offset 1)，应该返回 id 1
        let results = searcher.search_text("b", 10, false).unwrap();
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 1);
        assert_eq!(match_end - item_offset, 2);
    }

    #[test]
    fn test_regex_joyscale_region() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // "joyscale-region" = 15 字节, offset = 0
        searcher.append(1, "joyscale-region").unwrap();

        // 跨 token 匹配: .- 匹配 "e-" (位置 7..9)
        let results = searcher.search_regex(".-", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 7); // "e-" 从 7 开始
        assert_eq!(match_end - item_offset, 9);   // "e-" 长度 2

        // 复杂正则: \w+-\w+ 匹配整个 "joyscale-region"
        let results = searcher.search_regex(r"\w+-\w+", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 0);  // 从 'j' 开始
        assert_eq!(match_end - item_offset, 15);   // 到 'n' 结束
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();

        // 构建索引
        {
            let mut searcher = MmapSearcher::new(dir.path()).unwrap();
            // "test" = 4 字节, offset = 0
            // "hello world" = 11 字节, offset = 7
            searcher.append(1, "test").unwrap();
            searcher.append(2, "hello world").unwrap();
            searcher.save().unwrap();
        }

        // 重新加载
        let searcher = MmapSearcher::load(dir.path()).unwrap();
        assert_eq!(searcher.len(), 2);

        let results = searcher.search_text("test", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 1);
        assert_eq!(item_offset, 0);
        assert_eq!(match_start - item_offset, 0);
        assert_eq!(match_end - item_offset, 4);

        let results = searcher.search_text("hello", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        let (id, match_start, match_end, item_offset) = results[0];
        assert_eq!(id, 2);
        assert_eq!(item_offset, 7);
        assert_eq!(match_start - item_offset, 0); // "hello" 从 0 开始
        assert_eq!(match_end - item_offset, 5);   // "hello" 长度 5
    }

    #[test]
    fn test_limit() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // 追加 10 条包含 "x" 的记录
        for i in 1..=10 {
            searcher.append(i, "x").unwrap();
        }

        // 验证 mmap 内容：10 * "x\n\0\n" = 10 * 4 = 40
        let bytes = searcher.mmap.as_ref();
        assert_eq!(bytes.len(), 40);

        // 请求 5 条
        let results = searcher.search_text("x", 5, false).unwrap();
        assert_eq!(results.len(), 5, "expected 5 results, got {:?}", results);
        // 验证第一个结果的匹配范围
        let (_, match_start, match_end, item_offset) = results[0];
        assert_eq!(match_start - item_offset, 0); // "x" 从 0 开始
        assert_eq!(match_end - item_offset, 1);   // "x" 长度 1

        // 请求 200 条但实际只有 10 条
        let results = searcher.search_text("x", 200, false).unwrap();
        assert_eq!(results.len(), 10, "expected 10 results, got {:?}", results);
    }

    #[test]
    fn test_dedup() {
        let dir = TempDir::new().unwrap();
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();

        // 同一条记录匹配两次
        searcher.append(1, "test test").unwrap();

        // 搜索 "test" 两次，但结果只应包含一个 id
        let results = searcher.search_text("test", 10, false).unwrap();
        assert_eq!(results.len(), 1);
        // 验证只返回第一个匹配
        let (_, match_start, match_end, item_offset) = results[0];
        assert_eq!(match_start - item_offset, 0);  // 第一个 "test" 从 0 开始
        assert_eq!(match_end - item_offset, 4);   // 第一个 "test" 长度 4
    }
}
