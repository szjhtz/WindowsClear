use std::path::PathBuf;
use walkdir::WalkDir;
use rayon::prelude::*;
use anyhow::Result;
use directories::UserDirs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppDataCategory {
    Local,
    Roaming,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub category: AppDataCategory,
    pub parent_total_size: u64,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified_time: u64, // UNIX timestamp
}

#[derive(Serialize, Deserialize)]
struct ScanCache {
    entries: Vec<CacheEntry>,
}

pub struct Scanner;

impl Scanner {
    /// 获取目录最后修改时间（取本身元数据）
    fn get_modified_time(path: &PathBuf) -> u64 {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                 return modified.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            }
        }
        0
    }

    /// 加载缓存
    fn load_cache() -> Option<ScanCache> {
        let cache_path = std::env::temp_dir().join("cpan_mover_cache.json");
        if cache_path.exists() {
            if let Ok(content) = std::fs::read_to_string(cache_path) {
                return serde_json::from_str(&content).ok();
            }
        }
        None
    }

    /// 保存缓存
    fn save_cache(results: &[ScanResult]) {
        let entries: Vec<CacheEntry> = results.iter().map(|r| {
            CacheEntry {
                path: r.path.clone(),
                size: r.size_bytes,
                modified_time: Self::get_modified_time(&r.path),
            }
        }).collect();
        
        let cache = ScanCache { entries };
        let cache_path = std::env::temp_dir().join("cpan_mover_cache.json");
        if let Ok(content) = serde_json::to_string(&cache) {
            let _ = std::fs::write(cache_path, content);
        }
    }

    /// 计算指定目录的大小（递归）
    pub fn get_dir_size(path: &PathBuf) -> u64 {
        WalkDir::new(path)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// 检查路径是否是符号链接或 Junction Point
    pub fn is_symlink_or_junction(path: &PathBuf) -> bool {
        // 在 Windows 上，fs::symlink_metadata 可以检测 symlink 和 reparse points (junctions)
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            return metadata.file_type().is_symlink();
        }
        false
    }

    /// 扫描并返回占用超过 10% 的文件夹
    /// 
    /// `progress_cb`: 回调函数，参数为 (已完成数量, 总数量, 当前正在处理的文件夹名称)
    pub fn scan_large_folders<F>(progress_cb: F) -> Result<Vec<ScanResult>>
    where
        F: Fn(usize, usize, String) + Sync + Send + Clone,
    {
        let _user_dirs = UserDirs::new().ok_or_else(|| anyhow::anyhow!("无法获取用户目录"))?;
        
        let local_appdata = std::env::var("LOCALAPPDATA").map(PathBuf::from)?;
        let appdata = std::env::var("APPDATA").map(PathBuf::from)?; // Roaming

        let mut results = Vec::new();

        // 1. 收集所有一级子目录，以便计算总任务数
        let mut all_targets = Vec::new();
        
        for (root, category) in vec![
            (local_appdata, AppDataCategory::Local),
            (appdata, AppDataCategory::Roaming),
        ] {
            if !root.exists() { continue; }
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        // 过滤掉已经是软链接的文件夹
                        if !Self::is_symlink_or_junction(&path) {
                            all_targets.push((path, category.clone(), root.clone()));
                        }
                    }
                }
            }
        }

        let total_count = all_targets.len();
        let finished_count = Arc::new(AtomicUsize::new(0));

        // Load Cache
        let cache_map: std::collections::HashMap<PathBuf, (u64, u64)> = if let Some(cache) = Self::load_cache() {
            cache.entries.into_iter().map(|e| (e.path, (e.size, e.modified_time))).collect()
        } else {
            std::collections::HashMap::new()
        };

        // 2. 并行处理
        let sizes: Vec<(PathBuf, AppDataCategory, PathBuf, u64)> = all_targets.into_par_iter()
            .map(|(path, category, parent)| {
                // 上报进度
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                
                // Check Cache
                // 简单的缓存策略：如果文件夹最后修改时间没变，或者我们不深度检查，直接用缓存
                // 但文件夹的 mtime 并不总是反映内容的改变。
                // 更好的策略：如果用户没操作过，我们认为没变？不靠谱。
                // 真正的快速是：只 check 一级子文件的 mtime？太慢。
                // 我们这里只对比文件夹本身的 mtime。注意：Windows 文件夹 mtime 在子文件变动时不一定会变。
                // 妥协：如果缓存存在，先用缓存值，但给用户一个标识？
                // 需求说："大文件夹如果没有变动的话 扫描可以直接调用缓存"
                // 这里的"没有变动"很难界定。我们假设用户短时间内重复扫描。
                
                let current_mtime = Self::get_modified_time(&path);
                let size = if let Some((cached_size, cached_mtime)) = cache_map.get(&path) {
                    if *cached_mtime == current_mtime {
                        // 命中缓存 (注意：这在 Windows 上很不准，但为了速度只能这样，或者用户手动刷新)
                        // *cached_size
                        // 为了准确性，我们还是算吧？或者只在极短时间内缓存？
                        // 既然用户要求了，我们就先用这个策略，但加上 mtime check。
                        *cached_size
                    } else {
                        Self::get_dir_size(&path)
                    }
                } else {
                    Self::get_dir_size(&path)
                };
                
                let finished = finished_count.fetch_add(1, Ordering::SeqCst) + 1;
                progress_cb(finished, total_count, name);
                
                (path, category, parent, size)
            })
            .collect();

        // 3. 按父目录聚合计算 total_size 并筛选
        let mut parent_sizes = std::collections::HashMap::new();
        for (_, _, parent, size) in &sizes {
            *parent_sizes.entry(parent.clone()).or_insert(0) += size;
        }

        for (path, category, parent, size) in sizes {
            let total_size = *parent_sizes.get(&parent).unwrap_or(&0);
            if total_size == 0 { continue; }
            
            let threshold = (total_size as f64 * 0.1) as u64;
            if size > threshold {
                results.push(ScanResult {
                    name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                    path,
                    size_bytes: size,
                    category,
                    parent_total_size: total_size,
                });
            }
        }
        
        // 按大小降序排列
        results.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

        // Save new cache
        Self::save_cache(&results);

        Ok(results)
    }
}
