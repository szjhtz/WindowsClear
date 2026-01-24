#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::scanner::{Scanner, ScanResult};
use crate::core::mover::Mover;
use crate::core::proc_mgr::ProcMgr;
use crate::core::i18n::{self, Language};

mod core;

// Actual implementation with channels
enum AppEvent {
    // Scan events
    ScanProgress(usize, usize, String), // current, total, folder name
    ScanComplete(Vec<ScanResult>),
    ScanError(String),
    
    // Move events
    MoveStart(u64), // Total bytes to move
    MoveProgressBytes(u64), // Bytes moved in this chunk (incremental)
    MoveComplete,
    MoveError(String),
}

pub struct App {
    rx: std::sync::mpsc::Receiver<AppEvent>,
    tx: std::sync::mpsc::Sender<AppEvent>,
    
    scan_results: Vec<ScanResult>,
    selected_items: std::collections::HashSet<usize>, 
    
    target_root: PathBuf, 
    
    is_processing: bool,
    is_paused: bool,
    processing_type: ProcessingType, 
    
    status_msg: String,
    
    // Scan Progress
    scan_current: usize,
    scan_total: usize,
    scan_current_item: String,
    
    // Move Progress
    move_total_bytes: u64,
    move_current_bytes: u64,
    move_start_time: Option<Instant>,
    move_speed_bps: f64,
    move_remaining_secs: u64,
    pause_signal: Arc<AtomicBool>,
    
    auto_kill: bool,
    lang: Language,
}

#[derive(PartialEq)]
enum ProcessingType {
    None,
    Scanning,
    Moving,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            rx,
            tx,
            scan_results: Vec::new(),
            selected_items: std::collections::HashSet::new(),
            target_root: PathBuf::from("D:\\AppData"),
            is_processing: false,
            is_paused: false,
            processing_type: ProcessingType::None,
            status_msg: i18n::t("准备就绪"),
            
            scan_current: 0,
            scan_total: 0,
            scan_current_item: String::new(),
            
            move_total_bytes: 0,
            move_current_bytes: 0,
            move_start_time: None,
            move_speed_bps: 0.0,
            move_remaining_secs: 0,
            pause_signal: Arc::new(AtomicBool::new(false)),
            
            auto_kill: false,
            lang: Language::Chinese,
        }
    }
    
    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        
        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
    
    fn format_duration(secs: u64) -> String {
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    fn toggle_lang(&mut self) {
        let new_lang = match self.lang {
            Language::Chinese => Language::English,
            Language::English => Language::Chinese,
        };
        self.lang = new_lang;
        i18n::get_i18n().set_lang(new_lang);
        
        // Refresh status message if idle
        if !self.is_processing {
             self.status_msg = i18n::t("准备就绪");
        }
    }
    
    fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused;
        self.pause_signal.store(self.is_paused, Ordering::SeqCst);
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    let font_path = "C:\\Windows\\Fonts\\msyh.ttc";
    let font_path_alt = "C:\\Windows\\Fonts\\simhei.ttf";
    
    let font_data = if std::path::Path::new(font_path).exists() {
        std::fs::read(font_path).ok()
    } else if std::path::Path::new(font_path_alt).exists() {
        std::fs::read(font_path_alt).ok()
    } else {
        None
    };

    if let Some(data) = font_data {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(data),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "my_font".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "my_font".to_owned());
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle events
        while let Ok(event) = self.rx.try_recv() {
            match event {
                // --- SCAN EVENTS ---
                AppEvent::ScanProgress(current, total, name) => {
                    self.scan_current = current;
                    self.scan_total = total;
                    self.scan_current_item = name;
                    
                    if self.lang == Language::English {
                        self.status_msg = format!("Scanning: {} ({}/{})", self.scan_current_item, current, total);
                    } else {
                        self.status_msg = format!("正在扫描: {} ({}/{})", self.scan_current_item, current, total);
                    }
                },
                AppEvent::ScanComplete(res) => {
                    self.is_processing = false;
                    self.processing_type = ProcessingType::None;
                    self.scan_results = res;
                    
                    if self.lang == Language::English {
                         self.status_msg = format!("Scan complete, found {} large folders", self.scan_results.len());
                    } else {
                         self.status_msg = format!("扫描完成，共找到 {} 个大文件夹", self.scan_results.len());
                    }
                },
                AppEvent::ScanError(e) => {
                    self.is_processing = false;
                    self.processing_type = ProcessingType::None;
                    self.status_msg = format!("{}: {}", i18n::t("扫描出错: {}").replace("{}", ""), e);
                },
                
                // --- MOVE EVENTS ---
                AppEvent::MoveStart(total_bytes) => {
                    self.move_total_bytes = total_bytes;
                    self.move_current_bytes = 0;
                    self.move_start_time = Some(Instant::now());
                    self.is_paused = false;
                    self.pause_signal.store(false, Ordering::SeqCst);
                    
                    if self.lang == Language::English {
                        self.status_msg = format!("Preparing to move... Total size: {}", Self::format_bytes(total_bytes));
                    } else {
                        self.status_msg = format!("准备迁移... 总大小: {}", Self::format_bytes(total_bytes));
                    }
                },
                AppEvent::MoveProgressBytes(bytes_delta) => {
                    self.move_current_bytes += bytes_delta;
                    
                    // Calculate speed & ETA
                    if let Some(start_time) = self.move_start_time {
                        // 如果暂停了，我们需要调整 start_time 或者 elapsed 逻辑，否则速度会掉到 0
                        // 简单处理：暂停时不更新速度，或者速度为 0
                        if !self.is_paused {
                            let elapsed = start_time.elapsed().as_secs_f64();
                            if elapsed > 0.5 { 
                                self.move_speed_bps = self.move_current_bytes as f64 / elapsed;
                                if self.move_speed_bps > 0.0 {
                                    let remaining_bytes = self.move_total_bytes.saturating_sub(self.move_current_bytes);
                                    self.move_remaining_secs = (remaining_bytes as f64 / self.move_speed_bps) as u64;
                                }
                            }
                        }
                    }
                    
                    let percent = if self.move_total_bytes > 0 {
                         (self.move_current_bytes as f32 / self.move_total_bytes as f32) * 100.0
                    } else {
                        0.0
                    };
                    
                    let remaining_str = Self::format_duration(self.move_remaining_secs);
                    
                    if self.is_paused {
                        self.status_msg = i18n::t("已暂停").to_string();
                    } else {
                        if self.lang == Language::English {
                            self.status_msg = format!(
                                "Moving... {:.1}% - {}/s - ETA {}", 
                                percent, 
                                Self::format_bytes(self.move_speed_bps as u64),
                                remaining_str
                            );
                        } else {
                            self.status_msg = format!(
                                "正在迁移... {:.1}% - {}/s - 剩余约 {}", 
                                percent, 
                                Self::format_bytes(self.move_speed_bps as u64),
                                remaining_str
                            );
                        }
                    }
                },
                AppEvent::MoveComplete => {
                    self.is_processing = false;
                    self.processing_type = ProcessingType::None;
                    self.is_paused = false;
                    self.status_msg = i18n::t("所有迁移任务完成");
                    self.move_current_bytes = self.move_total_bytes;
                },
                AppEvent::MoveError(e) => {
                     self.status_msg = format!("{}: {}", i18n::t("错误: {}").replace("{}", ""), e);
                }
            }
        }
        
        // Force repaint if moving to update animation/progress bar smoothly
        if self.processing_type == ProcessingType::Moving {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(i18n::t("AppData Mover"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let lang_text = match self.lang {
                        Language::Chinese => "English",
                        Language::English => "中文",
                    };
                    if ui.button(lang_text).clicked() {
                        self.toggle_lang();
                    }
                });
            });
            
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.is_processing, egui::Button::new(i18n::t("扫描大文件夹"))).clicked() {
                    self.is_processing = true;
                    self.processing_type = ProcessingType::Scanning;
                    self.status_msg = i18n::t("正在初始化扫描...");
                    self.scan_results.clear();
                    self.selected_items.clear();
                    
                    // Reset scan state
                    self.scan_current = 0;
                    self.scan_total = 0;
                    
                    let tx = self.tx.clone();
                    let ctx = ctx.clone();
                    thread::spawn(move || {
                        let tx_clone = tx.clone();
                        let cb = move |current, total, name| {
                            let _ = tx_clone.send(AppEvent::ScanProgress(current, total, name));
                            ctx.request_repaint();
                        };
                        
                        match Scanner::scan_large_folders(cb) {
                            Ok(res) => {
                                tx.send(AppEvent::ScanComplete(res)).unwrap();
                            },
                            Err(e) => {
                                tx.send(AppEvent::ScanError(e.to_string())).unwrap();
                            }
                        }
                    });
                }
                
                if ui.add_enabled(!self.is_processing, egui::Button::new(i18n::t("执行迁移"))).clicked() {
                    // Clone tasks
                    let tasks: Vec<ScanResult> = self.scan_results.iter().enumerate()
                        .filter(|(i, _)| self.selected_items.contains(i))
                        .map(|(_, r)| r.clone())
                        .collect();
                    
                    if tasks.is_empty() {
                         self.status_msg = i18n::t("请先勾选需要迁移的文件夹");
                    } else {
                        self.is_processing = true;
                        self.processing_type = ProcessingType::Moving;
                        self.is_paused = false;
                        self.pause_signal.store(false, Ordering::SeqCst);
                        
                        let tx = self.tx.clone();
                        let ctx = ctx.clone();
                        let target_base = self.target_root.clone();
                        let auto_kill = self.auto_kill;
                        let pause_signal = self.pause_signal.clone();
                        
                        // Calculate total size first
                        let total_bytes: u64 = tasks.iter().map(|t| t.size_bytes).sum();
                        
                        thread::spawn(move || {
                            tx.send(AppEvent::MoveStart(total_bytes)).unwrap();
                            
                            for task in tasks {
                                let target_root = match task.category {
                                    crate::core::scanner::AppDataCategory::Local => target_base.join("Local"),
                                    crate::core::scanner::AppDataCategory::Roaming => target_base.join("Roaming"),
                                };

                                if let Ok(pids) = ProcMgr::check_locking_processes(&task.path) {
                                    if !pids.is_empty() && auto_kill {
                                        for pid in pids {
                                            let _ = ProcMgr::kill_process(pid);
                                        }
                                    }
                                }

                                let tx_clone = tx.clone();
                                let progress_cb = move |bytes_delta| {
                                    let _ = tx_clone.send(AppEvent::MoveProgressBytes(bytes_delta));
                                };

                                if let Err(e) = Mover::move_and_link(&task.path, &target_root, progress_cb, pause_signal.clone()) {
                                     tx.send(AppEvent::MoveError(format!("{} 失败: {}", task.name, e))).unwrap();
                                }
                            }
                            tx.send(AppEvent::MoveComplete).unwrap();
                            ctx.request_repaint();
                        });
                    }
                }
                
                // Pause/Resume Button
                if self.processing_type == ProcessingType::Moving {
                    let btn_text = if self.is_paused { i18n::t("继续") } else { i18n::t("暂停") };
                    if ui.button(btn_text).clicked() {
                        self.toggle_pause();
                    }
                }
            });
            
            ui.separator();
            
            ui.horizontal(|ui| {
                ui.label(i18n::t("目标根目录:"));
                let path_str = self.target_root.to_string_lossy();
                ui.label(path_str);
                if ui.add_enabled(!self.is_processing, egui::Button::new(i18n::t("选择..."))).clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.target_root = path;
                    }
                }
            });
            
            ui.checkbox(&mut self.auto_kill, i18n::t("自动结束占用进程 (慎用)"));
            
            ui.separator();
            
            if self.is_processing {
                ui.vertical(|ui| {
                    if self.processing_type == ProcessingType::Scanning {
                        // Scan Progress Bar
                        let progress = if self.scan_total > 0 {
                            self.scan_current as f32 / self.scan_total as f32
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(progress).text(format!("{}/{}", self.scan_current, self.scan_total)));
                        
                        if self.lang == Language::English {
                            ui.label(format!("Analyzing: {}", self.scan_current_item));
                        } else {
                            ui.label(format!("正在分析: {}", self.scan_current_item));
                        }
                        
                    } else if self.processing_type == ProcessingType::Moving {
                        // Move Progress Bar
                        let progress = if self.move_total_bytes > 0 {
                            self.move_current_bytes as f32 / self.move_total_bytes as f32
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(progress).show_percentage());
                        ui.horizontal(|ui| {
                            ui.label(format!("{}/{}", Self::format_bytes(self.move_current_bytes), Self::format_bytes(self.move_total_bytes)));
                            
                            if self.lang == Language::English {
                                ui.label(format!("Speed: {}/s", Self::format_bytes(self.move_speed_bps as u64)));
                                ui.label(format!("ETA: {}", Self::format_duration(self.move_remaining_secs)));
                            } else {
                                ui.label(format!("速度: {}/s", Self::format_bytes(self.move_speed_bps as u64)));
                                ui.label(format!("剩余: {}", Self::format_duration(self.move_remaining_secs)));
                            }
                        });
                    }
                });
            } else {
                ui.label(&self.status_msg);
            }
            
            ui.separator();
            
            // List
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, res) in self.scan_results.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut selected = self.selected_items.contains(&i);
                        if ui.add_enabled(!self.is_processing, egui::Checkbox::new(&mut selected, "")).changed() {
                            if selected {
                                self.selected_items.insert(i);
                            } else {
                                self.selected_items.remove(&i);
                            }
                        }
                        
                        ui.label(format!("{} ({})", res.name, Self::format_bytes(res.size_bytes)));
                        ui.weak(format!("[{:?}]", res.category));
                        ui.weak(res.path.to_string_lossy());
                    });
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "AppData Mover",
        native_options,
        Box::new(|cc| Box::new(App::new(cc))),
    )
}
