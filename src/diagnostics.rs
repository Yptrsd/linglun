// ============================================
// 运行时诊断收集器：把解析/渲染各阶段的错误与警告累积起来，
// 最后统一报告，而不是遇错即止。
// ============================================

/// 诊断级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// 单条诊断：级别 + 消息 + 可选的行/列位置
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

/// 诊断收集器。贯穿解析与渲染流程，最后统一 `report()`。
#[derive(Default)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一条错误（无位置）
    pub fn error(&mut self, msg: impl Into<String>) {
        self.items.push(Diagnostic {
            severity: Severity::Error,
            message: msg.into(),
            line: None,
            col: None,
        });
    }

    /// 记录一条错误（带行/列位置，1-based）
    pub fn error_at(&mut self, line: usize, col: usize, msg: impl Into<String>) {
        self.items.push(Diagnostic {
            severity: Severity::Error,
            message: msg.into(),
            line: Some(line),
            col: Some(col),
        });
    }

    /// 记录一条警告（无位置）
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.items.push(Diagnostic {
            severity: Severity::Warning,
            message: msg.into(),
            line: None,
            col: None,
        });
    }

    /// 是否已存在任何错误（警告不计）
    pub fn has_error(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    /// 统一报告所有诊断：错误输出到 stderr，警告输出到 stderr。
    pub fn report(&self) {
        for d in &self.items {
            let pos = match (d.line, d.col) {
                (Some(l), Some(c)) => format!("第{}行 第{}列: ", l, c),
                (Some(l), None) => format!("第{}行: ", l),
                _ => String::new(),
            };
            match d.severity {
                Severity::Error => eprintln!("[ERROR] {}{}", pos, d.message),
                Severity::Warning => eprintln!("[WARN] {}{}", pos, d.message),
            }
        }
    }
}
