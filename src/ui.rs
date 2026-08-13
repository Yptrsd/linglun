// ============================================
// 终端提示输出：统一成功/错误等提示格式
// ============================================

/// 成功提示（输出到 stdout）
pub fn success(msg: &str) {
    println!("[SUCCESS] {}", msg);
}

/// 错误提示（输出到 stderr，不退出进程）
pub fn error(msg: &str) {
    eprintln!("[ERROR] {}", msg);
}

/// 致命错误提示（输出到 stderr 并退出进程）
pub fn fatal(msg: &str) -> ! {
    error(msg);
    std::process::exit(1);
}
