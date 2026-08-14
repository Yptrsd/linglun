mod diagnostics;
mod parser;
mod renderer;
mod ui;

use std::path::{Path, PathBuf};

use diagnostics::Diagnostics;
use parser::parse_and_extract;
use renderer::pdf::render_to_pdf;

/// 默认输入乐谱路径
const DEFAULT_INPUT: &str = "test/test.llpartition";
/// 默认输出 PDF 路径
const DEFAULT_OUTPUT: &str = "test/test_output.pdf";

/// 打印命令行用法
fn print_usage() {
    println!("Usage: linglun <input.llpartition> [output.pdf] [--font-dir <目录>]");
    println!("  -h, --help          Show this help");
    println!("  -p, --parse [input] Print parse tree (default input: test/test.llpartition)");
    println!("  --font-dir <dir>    字体目录（冒号分隔可多个）。也支持环境变量 LINGLUN_FONTS。");
}

/// 由输入乐谱路径推断默认输出路径（同目录、同名、扩展名换 .pdf）
fn default_output(input: &str) -> String {
    Path::new(input)
        .with_extension("pdf")
        .to_string_lossy()
        .into_owned()
}

/// 读取输入乐谱文件（失败时直接退出）
fn read_score(input_path: &str) -> String {
    std::fs::read_to_string(input_path).unwrap_or_else(|e| {
        ui::fatal(&format!("Fail to read file: {}", e));
    })
}

/// 打印解析树模式：-p / --parse [input]，仅输出解析结果不生成 PDF
fn print_parse_tree(args: &[String]) {
    let input = if args.len() >= 3 {
        args[2].clone()
    } else {
        DEFAULT_INPUT.to_string()
    };
    let score = read_score(&input);
    parser::print_parsed_events(&score);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // 打印解析树模式：-p / --parse [input]
    if args.len() >= 2 && (args[1] == "-p" || args[1] == "--parse") {
        print_parse_tree(&args);
        return;
    }

    // 收集 --font-dir 目录列表（CLI 优先于环境变量 LINGLUN_FONTS）
    let mut font_dirs: Vec<Option<PathBuf>> = Vec::new();
    for dir in std::env::var("LINGLUN_FONTS")
        .ok()
        .into_iter()
        .flat_map(|s| s.split(':').map(str::to_string).collect::<Vec<_>>())
    {
        if !dir.is_empty() {
            font_dirs.push(Some(PathBuf::from(dir)));
        }
    }
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "-h" || a == "--help" {
            print_usage();
            return;
        } else if a == "--font-dir" {
            if i + 1 < args.len() {
                for dir in args[i + 1].split(':') {
                    if !dir.is_empty() {
                        font_dirs.push(Some(PathBuf::from(dir)));
                    }
                }
                i += 2;
            } else {
                ui::fatal("--font-dir 需要一个目录参数");
            }
        } else if let Some(rest) = a.strip_prefix("--font-dir=") {
            for dir in rest.split(':') {
                if !dir.is_empty() {
                    font_dirs.push(Some(PathBuf::from(dir)));
                }
            }
            i += 1;
        } else {
            positional.push(a.clone());
            i += 1;
        }
    }

    // 解析位置参数：<input> [output]，缺省时使用默认测试路径
    let (input_path, output_path) = match positional.len() {
        0 => (DEFAULT_INPUT.to_string(), DEFAULT_OUTPUT.to_string()),
        1 => (positional[0].clone(), default_output(&positional[0])),
        2 => (positional[0].clone(), positional[1].clone()),
        _ => {
            print_usage();
            return;
        }
    };

    let score = read_score(&input_path);

    // 贯穿全流程的诊断收集器：解析/渲染的所有错误与警告统一累积、最后报告
    let mut diag = Diagnostics::new();

    let events = parse_and_extract(&score, &mut diag);
    if diag.has_error() {
        diag.report();
        ui::fatal("解析失败，已中止");
    }
    ui::success(&format!("Parse success! Total {} events", events.len()));
    for (i, event) in events.iter().enumerate() {
        println!("  [{:2}] {}", i + 1, event);
    }

    match render_to_pdf(&events, &output_path, &font_dirs, &mut diag) {
        Ok(()) => {
            // 渲染期警告（如字体缺失回退）此时一并输出
            diag.report();
            if diag.has_error() {
                // 例如"没有可渲染的事件"：错误已记入收集器，不再假报成功
                ui::fatal("渲染失败，已中止");
            }
            let abs_path = Path::new(&output_path).canonicalize().unwrap_or_else(|_| Path::new(&output_path).to_path_buf());
            ui::success(&format!("PDF generated: {}", abs_path.display()));
        }
        Err(e) => {
            diag.report();
            ui::error(&format!("Fail to generate PDF: {}", e));
        }
    }
}
