//! MmapSearcher 单元测试
//!
//! 核心搜索逻辑测试

use history::search::MmapSearcher;
use tempfile::TempDir;

fn ids_from_results(results: &[(i64, usize, usize, usize)]) -> Vec<i64> {
    results.iter().map(|(id, _, _, _)| *id).collect()
}

#[test]
fn test_mmap_layout() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "hello" + "\n\0\n" = 5 + 3 = 8
    searcher.append(1, "hello").unwrap();
    // "world" + "\n\0\n" = 5 + 3 = 8
    searcher.append(2, "world").unwrap();

    assert_eq!(searcher.len(), 2);

    // 验证 offset_index: "hello" offset 0, "world" offset 8
    let results = searcher.search_text("world", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 8);
    assert_eq!(match_start, 8); // 'w' 在 mmap 位置 8
    assert_eq!(match_end, 13);  // 'd' 在 mmap 位置 12, +1 = 13
    assert_eq!(match_start - item_offset, 0); // "world" 相对位置 0..5
    assert_eq!(match_end - item_offset, 5);
}

#[test]
fn test_search_returns_match_range() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "hello world" -> offset 0
    searcher.append(1, "hello world").unwrap();
    // "rust is awesome" -> offset 14 (11 + "\n\0\n".len())
    searcher.append(2, "rust is awesome").unwrap();

    // "hello world" 长度 11, 分隔符 3, 所以第二个从 14 开始
    // 测试子串搜索 "world" 在第一个 item 中
    let results = searcher.search_text("world", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    // "world" 在 "hello world" 中的位置是 6..11
    assert_eq!(match_start - item_offset, 6);
    assert_eq!(match_end - item_offset, 11);

    // 测试正则搜索 "rust" 的位置
    let results = searcher.search_regex("rust", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 14);
    // "rust is awesome" 中 "rust" 位置是 0..4
    assert_eq!(match_start - item_offset, 0);
    assert_eq!(match_end - item_offset, 4);
}

#[test]
fn test_regex_match_range() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "joyscale-region" 长度 16
    searcher.append(1, "joyscale-region").unwrap();

    // joyscale-region = 16 字节
    // 测试 .* 贪婪匹配整个字符串
    let results = searcher.search_regex(".*", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    assert_eq!(match_start - item_offset, 0);
    assert_eq!(match_end - item_offset, 15);

    // joyscale-region: j-o-y-s-c-a-l-e---r-e-g-i-o-n
    //                   0-1-2-3-4-5-6-7-8-9-10-11-12-13-14
    // 分隔符是 -, 所以 "joyscale-region" 长度是 15
    // 测试 .- 匹配 "e-" (第一个 . 后跟 - 的组合)
    let results = searcher.search_regex(".-", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    // 第一个 ".-" 是 "e-" (位置 7-9)
    assert_eq!(match_start - item_offset, 7); // . 匹配 e
    assert_eq!(match_end - item_offset, 9);  // - 在位置 8, end = 9
}

#[test]
fn test_multiple_matches_returns_first() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "test test" 中 "test" 出现两次
    searcher.append(1, "test test").unwrap();

    let results = searcher.search_text("test", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (_, match_start, match_end, item_offset) = results[0];
    // 返回第一个匹配: 位置 0..4
    assert_eq!(match_start - item_offset, 0);
    assert_eq!(match_end - item_offset, 4);
}

#[test]
fn test_unicode_match_range() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "你好世界" - 每个汉字 3 字节 UTF-8
    searcher.append(1, "你好世界").unwrap();

    let results = searcher.search_text("世界", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (_, match_start, match_end, item_offset) = results[0];
    // "你好世界" = 3*2 + 3*2 = 12 字节
    // "你" 0..3, "好" 3..6, "世" 6..9, "界" 9..12
    assert_eq!(match_start - item_offset, 6);
    assert_eq!(match_end - item_offset, 12);
}

#[test]
fn test_binary_search_offset() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "ab" + "\n\0\n" = 2 + 3 = 5 bytes
    searcher.append(1, "ab").unwrap();
    // "cd" + "\n\0\n" = 2 + 3 = 5 bytes, offset = 5
    searcher.append(2, "cd").unwrap();

    // 搜索 "c" (第二个 item 的起始位置是 5), 'c' 在位置 5
    let results = searcher.search_text("c", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 5);
    assert_eq!(match_start - item_offset, 0); // "c" 在 "cd" 中位置 0
    assert_eq!(match_end - item_offset, 1);   // "c" 长度 1
}

#[test]
fn test_regex_joyscale_region() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // "joyscale-region" 长度 15
    searcher.append(1, "joyscale-region").unwrap();

    // 跨 token 匹配 "e-" (位置 7-9)
    let results = searcher.search_regex(".-", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    assert_eq!(match_start - item_offset, 7); // "e-"
    assert_eq!(match_end - item_offset, 9);

    // 复杂正则 \w+-\w+ 匹配整个 "joyscale-region"
    let results = searcher.search_regex(r"\w+-\w+", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(match_start - item_offset, 0);  // 从 'j' 开始
    assert_eq!(match_end - item_offset, 15);   // 到 'n' 结束
}

#[test]
fn test_persistence() {
    let dir = TempDir::new().unwrap();

    // 构建索引
    {
        let mut searcher = MmapSearcher::new(dir.path()).unwrap();
        searcher.append(1, "test").unwrap();
        searcher.save().unwrap();
    }

    // 重新加载
    let searcher = MmapSearcher::load(dir.path()).unwrap();
    let results = searcher.search_text("test", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    // 验证匹配范围
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    assert_eq!(match_start - item_offset, 0); // "test" 从 0 开始
    assert_eq!(match_end - item_offset, 4);  // "test" 长度 4
}

#[test]
fn test_limit() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    for i in 1..=10 {
        searcher.append(i, "x").unwrap();
    }

    let results = searcher.search_text("x", 5, false).unwrap();
    assert_eq!(results.len(), 5);
    // 验证第一个结果的匹配范围
    let (_, match_start, match_end, item_offset) = &results[0];
    assert_eq!(match_start - *item_offset, 0); // "x" 从 0 开始
    assert_eq!(match_end - *item_offset, 1);   // "x" 长度 1

    // 测试上限 200
    let results = searcher.search_text("x", 200, false).unwrap();
    assert_eq!(results.len(), 10);
}

#[test]
fn test_dedup() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // 同一条记录匹配两次
    searcher.append(1, "test test").unwrap();

    let results = searcher.search_text("test", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    // 验证只返回第一个匹配
    let (_, match_start, match_end, item_offset) = &results[0];
    assert_eq!(match_start - *item_offset, 0);  // 第一个 "test" 从 0 开始
    assert_eq!(match_end - *item_offset, 4);    // 第一个 "test" 长度 4
}

#[test]
fn test_empty_search() {
    let dir = TempDir::new().unwrap();
    let searcher = MmapSearcher::new(dir.path()).unwrap();

    let ids = ids_from_results(&searcher.search_text("anything", 10, false).unwrap());
    assert!(ids.is_empty());

    let ids = ids_from_results(&searcher.search_regex(".*", 10, false).unwrap());
    assert!(ids.is_empty());
}

#[test]
fn test_build_from_db() {
    let dir = TempDir::new().unwrap();

    let items: Vec<(i64, String)> = vec![
        (1i64, "first item".to_string()),
        (2, "second item".to_string()),
        (3, "third item".to_string()),
    ];

    let searcher = MmapSearcher::build_from_db(&items, dir.path()).unwrap();

    assert_eq!(searcher.len(), 3);

    // 验证第一个的匹配范围
    let results = searcher.search_text("first", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    assert_eq!(match_start - item_offset, 0); // "first" 从 0 开始
    assert_eq!(match_end - item_offset, 5);   // "first" 长度 5

    // 验证第二个的匹配范围
    // "first item" = 10 字节, 分隔符 = 3 字节
    // "second item" = 11 字节, offset = 10 + 3 = 13
    let results = searcher.search_text("second", 10, false).unwrap();
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 13);
    assert_eq!(match_start - item_offset, 0); // "second" 从相对位置 0 开始
    assert_eq!(match_end - item_offset, 6);  // "second" 长度 6
}

#[test]
fn test_multiple_delimiter_edge_case() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    // 测试连续的分隔符场景
    searcher.append(1, "a").unwrap();
    searcher.append(2, "b").unwrap();

    // 搜索 "a" - 第一个 item offset = 0
    let results = searcher.search_text("a", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 1);
    assert_eq!(item_offset, 0);
    assert_eq!(match_start - item_offset, 0);
    assert_eq!(match_end - item_offset, 1);

    // 搜索 "b" - 第二个 item offset = 4 (1 + "\n\0\n")
    let results = searcher.search_text("b", 10, false).unwrap();
    assert_eq!(results.len(), 1);
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 4);
    assert_eq!(match_start - item_offset, 0);
    assert_eq!(match_end - item_offset, 1);
}

#[test]
fn test_unicode_content() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    searcher.append(1, "你好世界").unwrap();
    searcher.append(2, "hello world").unwrap();

    let ids = ids_from_results(&searcher.search_text("你好", 10, false).unwrap());
    assert!(ids.contains(&1));

    let ids = ids_from_results(&searcher.search_text("世界", 10, false).unwrap());
    assert!(ids.contains(&1));
}

#[test]
fn test_case_sensitive_search() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    searcher.append(1, "Hello World").unwrap();
    searcher.append(2, "hello world").unwrap();
    searcher.append(3, "HELLO WORLD").unwrap();
    searcher.append(4, "HeLLo WoRLd").unwrap();

    // 大小写不敏感搜索 (case_sensitive = false)
    // 应该匹配所有包含 "hello" 的记录
    let results = searcher.search_text("hello", 10, false).unwrap();
    assert_eq!(results.len(), 4, "case-insensitive should match all 4 records");
    let ids: Vec<i64> = ids_from_results(&results);
    assert!(ids.contains(&1), "should match 'Hello World'");
    assert!(ids.contains(&2), "should match 'hello world'");
    assert!(ids.contains(&3), "should match 'HELLO WORLD'");
    assert!(ids.contains(&4), "should match 'HeLLo WoRLd'");

    // 大小写敏感搜索 (case_sensitive = true)
    // 只应该匹配精确的 "hello"
    let results = searcher.search_text("hello", 10, true).unwrap();
    assert_eq!(results.len(), 1, "case-sensitive should only match 1 record");
    let ids: Vec<i64> = ids_from_results(&results);
    assert!(ids.contains(&2), "should only match 'hello world'");
    assert!(!ids.contains(&1), "should not match 'Hello World'");
    assert!(!ids.contains(&3), "should not match 'HELLO WORLD'");
    assert!(!ids.contains(&4), "should not match 'HeLLo WoRLd'");

    // 大小写敏感搜索 "Hello"
    let results = searcher.search_text("Hello", 10, true).unwrap();
    assert_eq!(results.len(), 1);
    let ids: Vec<i64> = ids_from_results(&results);
    assert!(ids.contains(&1), "should match 'Hello World'");

    // 大小写敏感搜索 "HELLO"
    let results = searcher.search_text("HELLO", 10, true).unwrap();
    assert_eq!(results.len(), 1);
    let ids: Vec<i64> = ids_from_results(&results);
    assert!(ids.contains(&3), "should match 'HELLO WORLD'");

    // 验证大小写敏感搜索的匹配范围
    // "Hello World" = 11, offset = 0
    // "hello world" = 11, offset = 14 (0 + 11 + 3)
    // "HELLO WORLD" = 11, offset = 28 (14 + 11 + 3)
    // "HeLLo WoRLd" = 11, offset = 42 (28 + 11 + 3)
    let results = searcher.search_text("hello", 10, true).unwrap();
    let (id, match_start, match_end, item_offset) = results[0];
    assert_eq!(id, 2);
    assert_eq!(item_offset, 14); // "hello world" offset = 14
    assert_eq!(match_start - item_offset, 0); // "hello" 从相对位置 0 开始
    assert_eq!(match_end - item_offset, 5);  // "hello" 长度 5
}

#[test]
fn test_case_sensitive_regex() {
    let dir = TempDir::new().unwrap();
    let mut searcher = MmapSearcher::new(dir.path()).unwrap();

    searcher.append(1, "Hello World").unwrap();
    searcher.append(2, "hello world").unwrap();
    searcher.append(3, "HELLO WORLD").unwrap();

    // 大小写不敏感正则搜索 "hello.*world"
    let results = searcher.search_regex("(?i)hello.*world", 10, false).unwrap();
    assert_eq!(results.len(), 3, "case-insensitive regex should match all 3");

    // 大小写敏感正则搜索 "Hello.*World" (带 (?i) 前缀)
    let results = searcher.search_regex("(?i)Hello.*World", 10, false).unwrap();
    assert_eq!(results.len(), 3, "regex with (?i) should match all 3 variants");

    // 纯大小写敏感正则搜索 "Hello.*World" (不带 (?i))
    let results = searcher.search_regex("Hello.*World", 10, true).unwrap();
    assert_eq!(results.len(), 1, "case-sensitive regex should only match 1");
    let ids: Vec<i64> = ids_from_results(&results);
    assert!(ids.contains(&1), "should only match 'Hello World'");
}