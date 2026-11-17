//! 临时诊断（sodam#204）：把拖动过程中的窗口消息写进临时目录的
//! `sodam-drag-trace.log`，用来确认「快速拖动时窗口不跟手、松手后跳一下」
//! 到底是消息没到、还是到了没处理。验证完删除本模块和调用点。

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    UI::WindowsAndMessaging::*,
};

static SINK: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
static IN_MOVE_LOOP: AtomicBool = AtomicBool::new(false);

fn sink() -> &'static Mutex<Option<std::fs::File>> {
    SINK.get_or_init(|| {
        // 每次进程启动清空一次，避免历次运行混在一起。
        let path = std::env::temp_dir().join("sodam-drag-trace.log");
        Mutex::new(std::fs::File::create(path).ok())
    })
}

/// 系统进入/退出模态移动循环时调用，用来决定是否记录高频消息。
pub(crate) fn set_move_loop(active: bool) {
    IN_MOVE_LOOP.store(active, Ordering::Relaxed);
}

fn emit(kind: &str, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, extra: &str) {
    use std::io::Write as _;

    let mut cursor = POINT { x: 0, y: 0 };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!(
        "t={t} {kind} hwnd=0x{:X} msg=0x{msg:04X} w=0x{:X} l=0x{:X} cursor={},{} rect={},{},{},{} {extra}\n",
        hwnd.0 as usize,
        wparam.0,
        lparam.0,
        cursor.x,
        cursor.y,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom
    );
    let Ok(mut guard) = sink().lock() else {
        return;
    };
    if let Some(file) = guard.as_mut() {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

/// 记录拖动相关消息。移动循环之外只记低频关键消息，循环之内连
/// WM_MOUSEMOVE / WM_PAINT 一起记，写盘量只集中在拖动那几百毫秒里。
pub(crate) fn message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    let in_loop = IN_MOVE_LOOP.load(Ordering::Relaxed);
    let kind = match msg {
        WM_NCLBUTTONDOWN => "NC-DOWN",
        WM_NCLBUTTONUP => "NC-UP",
        WM_ENTERSIZEMOVE => "ENTER-MOVE",
        WM_EXITSIZEMOVE => "EXIT-MOVE",
        WM_SYSCOMMAND => "SYSCOMMAND",
        WM_CAPTURECHANGED => "CAPTURE-CHANGED",
        WM_MOUSELEAVE => "MOUSELEAVE",
        WM_NCMOUSELEAVE => "NCMOUSELEAVE",
        WM_SETCURSOR => "SETCURSOR",
        WM_MOVE if in_loop => "MOVE",
        WM_MOUSEMOVE if in_loop => "MOUSEMOVE",
        WM_NCMOUSEMOVE if in_loop => "NCMOUSEMOVE",
        WM_TIMER if in_loop => "TIMER",
        WM_PAINT if in_loop => "PAINT",
        WM_NCHITTEST if in_loop => "HITTEST",
        _ => return,
    };
    emit(kind, hwnd, msg, wparam, lparam, "");
}

/// 记录一次绘制耗时（毫秒），只在移动循环内记录。
pub(crate) fn paint_finished(hwnd: HWND, elapsed_ms: u64) {
    if !IN_MOVE_LOOP.load(Ordering::Relaxed) {
        return;
    }
    emit(
        "PAINT-DONE",
        hwnd,
        WM_PAINT,
        WPARAM(0),
        LPARAM(0),
        &format!("elapsed_ms={elapsed_ms}"),
    );
}
