//! 诊断门禁：进程树内存采样（Windows API）。
//!
//! 用途：定位内存渐进增长（~150MB → ~1.5GB）的真实归属。Tauri 在 Windows 上
//! 经 WebView2 运行前端，renderer/GPU/network 是**独立子进程**（msedgewebview2.exe），
//! 任务管理器看到的"应用内存"是整棵进程树汇总。本模块分别采样：
//! - **主进程**（Rust，含 ort arena / 堆）：`GetCurrentProcess` + `GetProcessMemoryInfo`
//! - **子进程树**（WebView2）：`CreateToolhelp32Snapshot` 枚举当前进程的所有后代
//!
//! 两个关键指标：
//! - `WorkingSetSize`（工作集）：含共享内存，受系统换页回收影响，波动大
//! - `PrivatePageCount`（私有字节）：进程独占、不可共享，更接近"真实占用"，**判断泄漏看这个**
//!
//! 所有调用是只读查询，无副作用。打点经 `tracing::info!(target = "mem_diag", ...)`
//! 输出到 `%APPDATA%\SnapText\logs\snaptext.log`。

#![cfg(windows)]

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_ACCESS_RIGHTS};

/// 进程树内存快照（单位 KB）。main = Rust 主进程，children = 所有后代（WebView2 等）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessTreeMemory {
    /// 主进程工作集（含共享）。
    pub main_ws_kb: u64,
    /// 主进程私有字节（判断泄漏的首选指标）。
    pub main_private_kb: u64,
    /// 子进程工作集合计。
    pub children_ws_kb: u64,
    /// 子进程私有字节合计。
    pub children_private_kb: u64,
    /// 子进程数量。
    pub child_count: u32,
}

impl ProcessTreeMemory {
    /// 主进程私有字节（MB），日志格式化用。
    pub fn main_priv_mb(&self) -> u64 {
        self.main_private_kb / 1024
    }
    /// 子进程私有字节合计（MB）。
    pub fn children_priv_mb(&self) -> u64 {
        self.children_private_kb / 1024
    }
    /// 主进程工作集（MB）。
    pub fn main_ws_mb(&self) -> u64 {
        self.main_ws_kb / 1024
    }
    /// 子进程工作集合计（MB）。
    pub fn children_ws_mb(&self) -> u64 {
        self.children_ws_kb / 1024
    }
}

/// 采样当前进程树内存。任何 Windows API 失败都静默降级（返回已采集到的部分），
/// 不阻断业务——诊断是观测手段，本身不能成为故障点。
pub fn snapshot_process_tree() -> ProcessTreeMemory {
    let mut mem = ProcessTreeMemory::default();

    // 主进程：GetCurrentProcess 返回伪句柄（-1），无需 CloseHandle。
    if let Some((ws, priv_kb)) = query_process_memory(unsafe {
        // GetCurrentProcess 返回当前进程伪句柄，常量值，不能也不需要 CloseHandle。
        windows::Win32::System::Threading::GetCurrentProcess()
    }) {
        mem.main_ws_kb = ws;
        mem.main_private_kb = priv_kb;
    }

    // 子进程：快照枚举全部进程，按父 PID 找当前进程的所有后代（多层）。
    let my_pid = std::process::id();
    if let Ok(descendants) = collect_descendants(my_pid) {
        for pid in descendants {
            if let Some((ws, priv_kb)) = query_pid_memory(pid) {
                mem.children_ws_kb += ws;
                mem.children_private_kb += priv_kb;
                mem.child_count += 1;
            }
        }
    }

    mem
}

/// 查询指定句柄的进程内存（工作集 + 私有字节）。失败返回 None。
///
/// 用 `PROCESS_MEMORY_COUNTERS_EX`（EX 版有 `PrivateUsage` = 私有字节，
/// 普通版无此字段）。`GetProcessMemoryInfo` 的 cb 参数填结构体大小，
/// 它按 cb 决定填充普通版还是 EX 版——传 EX 大小即返回 EX 版。
fn query_process_memory(handle: HANDLE) -> Option<(u64, u64)> {
    unsafe {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        GetProcessMemoryInfo(
            handle,
            &mut counters as *mut _ as *mut _,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
        .ok()?;
        Some((
            counters.WorkingSetSize as u64 / 1024,
            counters.PrivateUsage as u64 / 1024,
        ))
    }
}

/// 按 PID 查询进程内存（打开句柄 → 查询 → 关闭）。
fn query_pid_memory(pid: u32) -> Option<(u64, u64)> {
    unsafe {
        // PROCESS_QUERY_LIMITED_INFORMATION(0x1000)：只读查询权限，足够取内存信息，
        // 不需要更高权限，避免被安全软件拦截。Windows Vista+ 推荐。
        let handle = OpenProcess(PROCESS_ACCESS_RIGHTS(0x1000), false, pid).ok()?;
        let result = query_process_memory(handle);
        let _ = CloseHandle(handle);
        result
    }
}

/// 枚举当前进程的所有后代 PID（递归多层，含 WebView2 renderer/GPU/network）。
///
/// 策略：一次 `CreateToolhelp32Snapshot` 拿全系统进程列表，建立 parent→children 映射，
/// 再从当前 PID 做 BFS。避免对每个后代单独查父进程（多次 snapshot 开销大）。
fn collect_descendants(root_pid: u32) -> Result<Vec<u32>, windows::core::Error> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        // snapshot 句柄必须 CloseHandle，用 RAII 守卫确保异常路径也释放。
        let _guard = HandleGuard(snapshot);
        let mut entry = PROCESSENTRY32 {
            dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
            ..Default::default()
        };
        // parent → children 映射（PID 数量上限几十万，HashMap 够用）。
        let mut children_map: std::collections::HashMap<u32, Vec<u32>> =
            std::collections::HashMap::new();
        if Process32First(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                let parent = entry.th32ParentProcessID;
                // 跳过 root 自身（它是当前进程，已单独采样主进程）。
                if pid != root_pid {
                    children_map.entry(parent).or_default().push(pid);
                }
                if Process32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        // BFS 从 root_pid 往下找所有后代（WebView2 可能有多层子进程）。
        let mut descendants = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        if let Some(kids) = children_map.get(&root_pid) {
            for k in kids {
                queue.push_back(*k);
            }
        }
        while let Some(pid) = queue.pop_front() {
            descendants.push(pid);
            if let Some(kids) = children_map.get(&pid) {
                for k in kids {
                    queue.push_back(*k);
                }
            }
        }
        Ok(descendants)
    }
}

/// Windows HANDLE 的 RAII 守卫，确保 Drop 时 CloseHandle（避免句柄泄漏）。
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 统一格式输出内存诊断日志。target 固定 `mem_diag`，便于 `grep mem_diag` 过滤。
///
/// `node` 是打点节点名（如 `ocr_before`），`extra` 是业务上下文（如 `img=1920x1080`）。
/// 输出示例：`main_ws=312MB main_priv=280MB children_ws=580MB children_priv=610MB children=3`。
pub fn log_mem_diag(node: &str, m: &ProcessTreeMemory, extra: &str) {
    tracing::info!(
        target: "mem_diag",
        node = node,
        extra = extra,
        main_ws_mb = m.main_ws_mb(),
        main_priv_mb = m.main_priv_mb(),
        children_ws_mb = m.children_ws_mb(),
        children_priv_mb = m.children_priv_mb(),
        children = m.child_count,
        "mem_diag"
    );
}

#[cfg(test)]
mod tests {
    //! 诊断模块单元测试：验证进程内存采样 API 调用链可靠（不 panic + 采样有效）。
    //! 不测 Windows API 本身的正确性（那是 OS 的职责），只测我们的调用方式无误。
    //! 真实内存增长曲线验证由 stress-test.ps1 驱动（需桌面会话 + 模型）。
    use super::*;

    #[test]
    fn snapshot_does_not_panic() {
        // 最基本：采样整棵进程树不应 panic（Windows API 调用链全程安全）。
        let _ = snapshot_process_tree();
    }

    #[test]
    fn main_private_bytes_positive() {
        // 当前测试进程本身有内存占用，私有字节必 > 0（证明主进程采样有效）。
        let m = snapshot_process_tree();
        assert!(
            m.main_private_kb > 0,
            "主进程私有字节应 > 0，实际 main_private_kb={}",
            m.main_private_kb
        );
    }

    #[test]
    fn main_working_set_ge_private() {
        // 工作集含共享内存，必 >= 私有字节（OS 语义不变式，佐证两个字段都读对了）。
        let m = snapshot_process_tree();
        assert!(
            m.main_ws_kb >= m.main_private_kb,
            "工作集应 >= 私有字节，实际 ws={} priv={}",
            m.main_ws_kb,
            m.main_private_kb
        );
    }

    #[test]
    fn mb_conversions_are_safe() {
        // MB 换算不应 panic（小值除法、零值边界）。
        let zero = ProcessTreeMemory::default();
        assert_eq!(zero.main_priv_mb(), 0);
        assert_eq!(zero.children_priv_mb(), 0);
        assert_eq!(zero.main_ws_mb(), 0);
        assert_eq!(zero.children_ws_mb(), 0);
    }

    #[test]
    fn collect_descendants_finds_spawned_child() {
        // 起一个真实子进程（ping 挂 3 秒），验证 collect_descendants 能枚举到它。
        // 这是子进程树遍历逻辑的唯一可自动化验证点——WebView2 多层子进程的
        // 真实场景由 stress-test.ps1 覆盖（需 Tauri 运行时）。
        let my_pid = std::process::id();
        // ping -n 4 会阻塞约 3 秒，足够在 collect 前存活。
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping -n 4 127.0.0.1 > nul"])
            .spawn()
            .expect("启动测试子进程失败");
        let child_pid = child.id();
        let descendants = collect_descendants(my_pid).expect("collect_descendants 不应失败");
        assert!(
            descendants.contains(&child_pid),
            "应枚举到刚 spawn 的子进程 PID {child_pid}，实际后代列表：{descendants:?}"
        );
        // 等子进程自然退出，避免泄漏（Drop 不会 kill）。
        let _ = child.wait();
    }
}
