# 伶伦 (linglun)

> 昔黄帝令伶伦作为律。伶伦自大夏之西，乃之阮隃之阴，取竹于嶰谿之谷，以生空窍厚钧者、断两节间、其长三寸九分而吹之，以为黄钟之宫，吹曰“舍少”。次制十二筒，以之阮隃之下，听凤皇之鸣，以别十二律。其雄鸣为六，雌鸣亦六，以比黄钟之宫，适合。黄钟之宫，皆可以生之，故曰黄钟之宫，律吕之本。黄帝又命伶伦与荣将铸十二钟，以和五音，以施英韶。以仲春之月，乙卯之日，日在奎，始奏之，命之曰咸池。————《吕氏春秋·仲夏纪·古乐》

伶伦是一个简谱乐谱排版工具，能将文本格式的简谱转换为高质量的 PDF 乐谱。

核心特性：

- **简谱语法解析** — 基于 [Pest](https://pest.rs) 的 PEG 文法，支持音符、和弦、连音、装饰音、力度记号等
- **SMuFL 音乐字体渲染** — 使用 Leland 等 OpenType 音乐字体精确排版
- **中西文混排** — Source Han Serif SC（思源宋体）处理中文，Source Serif Pro 处理西文
- **PDF 直出** — 通过 [pdf-writer](https://github.com/nickel-org/pdf-writer) 直接生成 PDF，无需 LaTeX

## 快速开始

### 依赖

- Rust 2024 edition（`rustc >= 1.85`）
- 字体文件（运行时由 `--font-dir` 或环境变量 `LINGLUN_FONTS` 指定）：
  - `Leland.otf`（SMuFL 音乐记号）
  - `SourceSerifPro-Bold.otf`（音符数字粗体）
  - `SourceSerifPro-Regular.otf`（西文正文）
  - `SourceHanSerifSC-Regular.otf`（中文/非西文）

### 编译

```bash
cargo build --release
```

### 使用

```bash
# 基本用法：输入 .llpartition 文件，输出同名 .pdf
linglun input.llpartition [output.pdf]

# 指定字体目录
linglun input.llpartition --font-dir /path/to/fonts

# 多个字体目录（冒号分隔）
linglun input.llpartition --font-dir /dir1:/dir2

# 通过环境变量指定字体目录
LINGLUN_FONTS=/path/to/fonts linglun input.llpartition

# 打印解析树（调试用）
linglun -p input.llpartition
linglun --parse input.llpartition
```

## 乐谱语法

伶伦使用自定义的 `.llpartition` 文本格式。基本语法：

```
#title<Title>
#subtitle<Subtitle>
#credit<Credit>

#key<C>
#tempo<♩ = 120 Moderato / 中速>
#timesig<4/4>

1 2 3 4 | 5 6 7 1^ |
```

### 语法要素

| 元素 | 语法 | 示例 |
|------|------|------|
| 音符 | `0-9`（1-7 为 do-si，0 为休止） | `1 2 3 4` |
| 高八度 | `^` 后缀 | `1^`（高音 do） |
| 低八度 | `-` 后缀 | `1-`（低音 do） |
| 附点 | `.` 后缀 | `1.`（附点音符） |
| 和弦 | `[...]` | `[1 3 5]`（C 和弦） |
| 连音 | `(...)` | `(1 2 3)` |
| 装饰音 | `{...}` | `{1 3 5}` |
| 小节线 | `\|` `\|\|` `\|\|\|` | 单线 / 双线 / 终止线 |
| 重复 | `:\|` `:\|:` | 左重复 / 右重复 |

### 控制指令

```
#key<C>                  调号
#tempo<♩ = 120>          速度记号（支持 #icon<note_4> 渲染时值音符）
#timesig<4/4>            拍号
#dynamics<pp>            力度记号（pp / p / mp / mf / f / ff / fff 等）
#sharp<1>                升号（作用于后续音符）
#flat<3>                 降号
#nat<5>                  还原号
#cresc<...>              渐强
#dim<...>                渐弱
#grace<...>              装饰音组
```

## 项目结构

```
linglun/
├── src/
│   ├── main.rs              入口：CLI 参数解析、流程编排
│   ├── parser/
│   │   ├── mod.rs           解析器：Pest 文法 + AST 定义
│   │   └── score.pest       PEG 文法规则
│   ├── renderer/
│   │   ├── mod.rs           渲染模块入口
│   │   ├── font.rs          字体加载、SMuFL 映射、PDF 嵌入
│   │   ├── control.rs       控制指令注册表与渲染
│   │   └── pdf.rs           PDF 布局引擎与渲染核心
│   ├── ui.rs                终端输出样式（彩色提示）
│   └── diagnostics.rs       统一错误/警告收集与报告
├── test/
│   ├── test.llpartition     示例乐谱
│   ├── test.lllayout        布局配置（预留）
│   └── test.lllyric         歌词配置（预留）
├── .pre-commit-config.yaml  Git hooks 配置
├── Cargo.toml
└── README.md
```

## 开发

### 构建与运行

```bash
# 开发模式编译
cargo build

# 运行测试
cargo test

# 运行示例
cargo run -- test/test.llpartition
```

### Pre-commit 钩子

本项目使用 [pre-commit](https://pre-commit.com/) 管理 Git hooks，确保提交前代码质量。

#### 安装

```bash
pip install pre-commit
# 或
brew install pre-commit
```

在项目根目录执行一次即可激活：

```bash
pre-commit install
```

#### 钩子列表

`.pre-commit-config.yaml` 中配置了以下钩子：

| 钩子 | 来源 | 说明 |
|------|------|------|
| `trailing-whitespace` | pre-commit-hooks | 自动去除行尾空白 |
| `end-of-file-fixer` | pre-commit-hooks | 确保文件以换行符结尾 |
| `check-yaml` | pre-commit-hooks | 校验 YAML 文件格式 |
| `check-added-large-files` | pre-commit-hooks | 阻止提交过大的文件 |
| `fmt` | pre-commit-rust | `cargo fmt` 自动格式化 Rust 代码 |
| `cargo-check` | local | `cargo check --all-targets --all-features` 快速编译检查 |

#### 日常使用

```bash
# 检查所有文件（不修改）
pre-commit run --all-files

# 仅检查已暂存的文件
pre-commit run

# 自动运行（git commit 时自动触发）
git add .
git commit -m "your message"
```

#### 添加 clippy 钩子（可选）

如果希望在提交前也运行 clippy 检查，可在 `.pre-commit-config.yaml` 的 `local` hooks 部分添加：

```yaml
  - repo: local
    hooks:
      # ... existing hooks ...
      - id: clippy
        name: clippy
        entry: cargo clippy --all-targets -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false
        always_run: true
        stages: [pre-commit]
```

## 许可

[Unlicense](LICENSE)
