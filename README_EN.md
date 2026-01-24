# WindowsClear (AppData Mover)

[中文](README.md) | [English](README_EN.md)

![Screenshot](img1.png)

## Introduction
A powerful Windows system disk cleanup tool focused on freeing up massive space occupied by the `AppData` directory. It automatically identifies large software data folders and allows you to move them to another drive (e.g., Drive D) with one click. It automatically creates Directory Junctions (Junction Points) so that software runs seamlessly as if the files were never moved.

## Key Features
*   **Smart Scan**: Automatically analyzes `%LOCALAPPDATA%` and `%APPDATA%` to quickly locate folders taking up more than 10% of space.
*   **Seamless Migration**: Moves files across drives and creates Junction links in place; no software reconfiguration needed.
*   **Safe & Reliable**:
    *   **Process Guard**: Detects file locks and supports auto-killing locking processes (using Windows Restart Manager).
    *   **Rollback**: Automatically attempts to restore files if an error occurs during migration.
*   **User Experience**:
    *   **High Performance**: Built with Rust, utilizing multi-threaded scanning for blazing speed.
    *   **Smart Cache**: Instant results for repeated scans if no changes are detected.
    *   **Visual Progress**: Byte-level precision progress bar with real-time speed and ETA.
    *   **Bilingual**: One-click switch between English and Chinese interface.
    *   **Pause/Resume**: Pause anytime during large file transfers.

## Usage
1.  Run `WindowsClear.exe` (or `cpan_mover.exe`) as **Administrator**.
2.  Click **"Scan Large Folders"**.
3.  Check the folders you want to move (start with non-critical apps recommended).
4.  Select **Target Root** (e.g., `D:\AppData`).
5.  Click **"Move Folders"** and wait for completion.

## FAQ
*   **Why Administrator privileges?**
    *   Creating Directory Junctions (`mklink /J`) and managing other processes typically require admin rights.
*   **Will software work after moving?**
    *   Yes. Directory Junctions are transparent to applications; they still "see" files on C drive.
*   **How to restore?**
    *   Simply delete the shortcut (folder with arrow icon) on C drive, then cut and paste the folder from D drive back to its original location on C.

## Build Guide
Built with Rust.

```bash
# Clone repository
git clone https://github.com/tanaer/WindowsClear.git
cd WindowsClear

# Build Release
cargo build --release
```

## License
MIT License
