use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Context, Result, anyhow};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

pub struct Mover;

impl Mover {
    /// 迁移文件夹并创建链接
    /// 
    /// # Arguments
    /// * `source` - 原文件夹路径
    /// * `target_root` - 目标根目录
    /// * `progress_cb` - 进度回调 (已移动字节数)
    /// * `pause_signal` - 暂停信号
    pub fn move_and_link<F>(source: &Path, target_root: &Path, progress_cb: F, pause_signal: Arc<AtomicBool>) -> Result<()> 
    where F: Fn(u64) + Clone
    {
        if !source.exists() {
            return Err(anyhow!("源路径不存在: {:?}", source));
        }

        let folder_name = source.file_name()
            .ok_or_else(|| anyhow!("无效的源路径"))?;
        
        let target_path = target_root.join(folder_name);

        // 1. 确保目标根目录存在
        if !target_root.exists() {
            fs::create_dir_all(target_root)
                .context("无法创建目标根目录")?;
        }

        if target_path.exists() {
            return Err(anyhow!("目标路径已存在: {:?}", target_path));
        }

        // 2. 移动文件夹 (Copy + Delete)
        // 使用递归复制，并在每次复制文件后回调
        Self::copy_dir_all(source, &target_path, progress_cb.clone(), pause_signal.clone())
            .context("文件复制失败，正在回滚...")
            .map_err(|e| {
                let _ = fs::remove_dir_all(&target_path);
                e
            })?;

        // 3. 删除源文件夹
        fs::remove_dir_all(source)
            .context("删除源文件夹失败")?;

        // 4. 创建 Junction Point
        Self::create_junction(source, &target_path)
            .map_err(|e| {
                // 如果链接创建失败，恢复文件
                // 注意：move_dir_back 也应该支持 progress_cb，这里简化处理，传入空闭包
                let _ = Self::move_dir_back(&target_path, source);
                anyhow!("创建链接失败: {}. 已尝试恢复文件。", e)
            })?;

        Ok(())
    }

    fn copy_dir_all<F>(src: &Path, dst: &Path, progress_cb: F, pause_signal: Arc<AtomicBool>) -> Result<()> 
    where F: Fn(u64) + Clone
    {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            // Check pause
            while pause_signal.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
            }

            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                Self::copy_dir_all(&entry.path(), &dst.join(entry.file_name()), progress_cb.clone(), pause_signal.clone())?;
            } else {
                fs::copy(entry.path(), dst.join(entry.file_name()))?;
                // 获取文件大小并回调
                if let Ok(metadata) = entry.metadata() {
                    progress_cb(metadata.len());
                }
            }
        }
        Ok(())
    }

    fn move_dir_back(src: &Path, dst: &Path) -> Result<()> {
        if fs::rename(src, dst).is_ok() {
            return Ok(());
        }
        // 回滚时不需要进度条和暂停
        let dummy_signal = Arc::new(AtomicBool::new(false));
        Self::copy_dir_all(src, dst, |_| {}, dummy_signal)?;
        fs::remove_dir_all(src)?;
        Ok(())
    }

    fn create_junction(link: &Path, target: &Path) -> Result<()> {
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("mklink 失败: {}", err_msg));
        }
        Ok(())
    }
}
