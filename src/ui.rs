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

/// 中止提示（输出到 stderr 并退出进程），但**不带 `[ERROR]` 前缀**。
/// 用于错误详情已经由 `Diagnostics::report()` 详述过的场景
/// （如"没有可渲染的事件"已列出），这里只声明中止、不重复错误计数。
pub fn fatal_bare(msg: &str) -> ! {
    eprintln!("{}", msg);
    std::process::exit(1);
}
