use std::path::Path;
use anyhow::{Result, anyhow};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_MORE_DATA, HANDLE},
        System::{
            RestartManager::{
                RmEndSession, RmGetList, RmRegisterResources, RmStartSession,
                RM_PROCESS_INFO, CCH_RM_SESSION_KEY, 
            },
            Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
};

pub struct ProcMgr;

impl ProcMgr {
    /// 检查是否有进程占用了指定路径下的文件
    /// 返回占用该路径的进程 ID 列表
    pub fn check_locking_processes(path: &Path) -> Result<Vec<u32>> {
        unsafe {
            let mut session_handle: u32 = 0;
            // CCH_RM_SESSION_KEY is a u32 constant, need to cast to usize for array length
            let mut session_key_buf = [0u16; CCH_RM_SESSION_KEY as usize];
            let session_key = PWSTR::from_raw(session_key_buf.as_mut_ptr());

            // 1. Start Session
            let res = RmStartSession(&mut session_handle, 0, session_key);
            if res != 0 {
                return Err(anyhow!("无法启动 Restart Manager 会话: {}", res));
            }

            // 确保 session 最终被关闭
            let result = (|| -> Result<Vec<u32>> {
                // 2. Register Resources
                let path_str = path.to_string_lossy();
                let mut wide_path: Vec<u16> = path_str.encode_utf16().collect();
                wide_path.push(0);
                
                let paths = [PCWSTR::from_raw(wide_path.as_ptr())];
                
                let res = RmRegisterResources(session_handle, Some(&paths), None, None);
                if res != 0 {
                    return Err(anyhow!("无法注册资源: {}", res));
                }

                // 3. Get List
                let mut array_len_needed: u32 = 0;
                let mut array_len = 0;
                let mut reboot_reasons = 0; // Use u32 for simplicity or look up correct type
                
                // 第一次调用获取需要的长度
                let _ = RmGetList(
                    session_handle,
                    &mut array_len_needed,
                    &mut array_len,
                    None,
                    &mut reboot_reasons,
                );

                if array_len_needed == 0 {
                    return Ok(Vec::new()); // 没有进程占用
                }

                let mut process_info = vec![RM_PROCESS_INFO::default(); array_len_needed as usize];
                array_len = array_len_needed;

                let res = RmGetList(
                    session_handle,
                    &mut array_len_needed,
                    &mut array_len,
                    Some(process_info.as_mut_ptr()),
                    &mut reboot_reasons,
                );

                // ERROR_MORE_DATA is 234
                if res != 0 && res != ERROR_MORE_DATA.0 {
                     return Err(anyhow!("获取进程列表失败: {}", res));
                }

                // 提取 PID
                let pids: Vec<u32> = process_info.iter()
                    .take(array_len as usize)
                    .map(|info| info.Process.dwProcessId)
                    .collect();

                Ok(pids)
            })();

            // 4. End Session
            let _ = RmEndSession(session_handle);

            result
        }
    }

    /// 结束指定 PID 的进程
    pub fn kill_process(pid: u32) -> Result<()> {
        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
            if handle.is_invalid() {
                 return Err(anyhow!("无法打开进程 {}", pid));
            }
            
            // TerminateProcess 返回 BOOL, 即 i32 (0 失败, 非0 成功)
            let res = TerminateProcess(handle, 1);
            if res.as_bool() == false {
                return Err(anyhow!("无法结束进程 {}: (错误码不明)", pid));
            }
                
            Ok(())
        }
    }
}
