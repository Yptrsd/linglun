mod parser;
mod renderer;
mod ui;

use std::path::Path;

use parser::parse_and_extract;
use renderer::pdf::render_to_pdf;

/// 默认输入乐谱路径
const DEFAULT_INPUT: &str = "test/test.llpartition";
/// 默认输出 PDF 路径
const DEFAULT_OUTPUT: &str = "test/test_output.pdf";

/// 打印命令行用法
fn print_usage() {
    println!("Usage: linglun <input.llpartition> [output.pdf]");
    println!("  -h, --help          Show this help");
    println!("  -p, --parse [input] Print parse tree (default input: test/test.llpartition)");
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

    // 解析命令行参数：<input> [output]，缺省时使用默认测试路径
    let (input_path, output_path) = match args.len() {
        1 => (DEFAULT_INPUT.to_string(), DEFAULT_OUTPUT.to_string()),
        2 => {
            if args[1] == "-h" || args[1] == "--help" {
                print_usage();
                return;
            }
            (args[1].clone(), default_output(&args[1]))
        }
        3 => (args[1].clone(), args[2].clone()),
        _ => {
            print_usage();
            return;
        }
    };

    let score = read_score(&input_path);

    match parse_and_extract(&score) {
        Ok(events) => {
            ui::success(&format!("Parse success! Total {} events", events.len()));
            for (i, event) in events.iter().enumerate() {
                println!("  [{:2}] {}", i + 1, event);
            }

            match render_to_pdf(&events, &output_path) {
                Ok(()) => {
                    let abs_path = Path::new(&output_path).canonicalize().unwrap_or_else(|_| Path::new(&output_path).to_path_buf());
                    ui::success(&format!("PDF generated: {}", abs_path.display()));
                }
                Err(e) => ui::error(&format!("Fail to generate PDF: {}", e)),
            }
        }
        Err(e) => {
            ui::error(&format!("Fail to parse score: {}", e));
        }
    }
}
