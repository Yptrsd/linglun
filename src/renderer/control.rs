//! 控制函数（#key/#tempo/#timesig/#dynamics 等）的注册表与渲染接口。
//!
//! 把原本散落在 `pdf.rs` 的 4 处 `match name`（字体预热 / 布局宽度 / 行内排版 / 渲染）
//! 收拢为 `ControlFunc` trait + 静态注册表。新增一个渲染型控制函数只需：
//!   1. 实现 `ControlFunc`（按需覆写 `width`/`header_advance`/`prewarm_chars`/`render`）
//!   2. 在 `REGISTRY` 切片加一行
//! `pdf.rs` 的布局与渲染核心代码不再被改动。

use pdf_writer::{Content, Name};

use crate::renderer::font::{dynamics_char, icon_to_smufl, time_sig, FontFamily};
use crate::renderer::pdf::{
    parse_timesig, render_mixed_text, render_tempo, show_glyph, NOTE_FONT_SIZE, NOTE_GLYPH_WIDTH,
};

// ============================================
// 位置偏移常量（从 pdf.rs 迁入，仅控制函数渲染使用）
// ============================================
/// 调号距音符基线的上偏移（紧贴音符上方，略高于八度点）
const KEY_Y_OFFSET: f32 = 22.0;
/// 速度记号距音符基线的上偏移（调号上方一行）
const TEMPO_Y_OFFSET: f32 = 36.0;
/// 力度记号距音符基线的偏移（音符下方）
const DYNAMICS_Y_OFFSET: f32 = 28.0;

// ============================================
// 渲染上下文：打包 content + fonts + fallback，便于 trait 方法传递
// ============================================

/// 控制函数渲染所需的全部上下文。
pub(crate) struct RenderCtx<'a> {
    pub content: &'a mut Content,
    pub fonts: &'a mut FontFamily,
    pub fallback: Name<'a>,
}

impl<'a> RenderCtx<'a> {
    /// 集中再借用：返回 `(content, fonts, fallback)` 三个独立可变借用，
    /// 使实现内部可同时操作 `fonts.leland` 与 `content`（如拍号画分数线）。
    fn split(&mut self) -> (&mut Content, &mut FontFamily, Name<'a>) {
        (&mut *self.content, &mut *self.fonts, self.fallback)
    }
}

// ============================================
// 控制函数 trait
// ============================================

/// 渲染型控制函数接口。`Sync` 上界是静态注册表（`static REGISTRY`）的要求。
pub(crate) trait ControlFunc: Sync {
    /// 该函数在 y_base 行占用的视觉宽度（用于布局）。默认 0 = 零宽，附着下一个实事件。
    fn width(&self, _value: &str) -> f32 {
        0.0
    }
    /// 若为 header 组（如连续调号 `#key`），返回在 `header_cursor` 渲染并推进的量；否则 `None`。
    fn header_advance(&self) -> Option<f32> {
        None
    }
    /// 字体嵌入前需额外预热的字形（SMuFL 码位等）。默认空。
    fn prewarm_chars(&self, _value: &str) -> Vec<char> {
        Vec::new()
    }
    /// 该函数渲染在音符上方时，相对 y_base 向上延伸的高度（用于自适应行距）。默认 0。
    fn top_extent(&self) -> f32 {
        0.0
    }
    /// 该函数渲染在音符下方时，相对 y_base 向下延伸的高度（用于自适应行距）。默认 0。
    fn bottom_extent(&self) -> f32 {
        0.0
    }
    /// 渲染。y 由实现内部从 `y_base` 自行计算（不同函数偏移不同，甚至多层如拍号）。
    fn render(&self, ctx: &mut RenderCtx, value: &str, x: f32, y_base: f32);
}

// ============================================
// 具体控制函数实现
// ============================================

/// `#key<C>`：调号，渲染 `1=C` 于音符上方，连续多个 `#key` 在 header 并排。
struct KeyFunc;
impl ControlFunc for KeyFunc {
    fn header_advance(&self) -> Option<f32> {
        Some(26.0)
    }
    fn top_extent(&self) -> f32 {
        KEY_Y_OFFSET + NOTE_FONT_SIZE * 0.8
    }
    fn render(&self, ctx: &mut RenderCtx, value: &str, x: f32, y_base: f32) {
        let (content, fonts, fallback) = ctx.split();
        let y = y_base + KEY_Y_OFFSET;
        let size = NOTE_FONT_SIZE * 0.8;
        let text = format!("1={}", value);
        render_mixed_text(content, &text, x, y, size, fonts, fallback);
    }
}

/// `#tempo<...>`：速度记号，渲染于调号上方一行。支持 `#icon<note_4> = 120`、意大利术语、中文、混合。
struct TempoFunc;
impl ControlFunc for TempoFunc {
    fn prewarm_chars(&self, value: &str) -> Vec<char> {
        // 扫描 `#icon<name>` 模式，映射为 SMuFL 码位用于字体预热
        let mut chars = Vec::new();
        let mut rest = value;
        while let Some(start) = rest.find("#icon<") {
            let after = &rest[start + "#icon<".len()..];
            if let Some(end) = after.find('>') {
                let icon_name = &after[..end];
                if let Some(smufl_chars) = icon_to_smufl(icon_name) {
                    for &c in smufl_chars {
                        chars.push(c);
                    }
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
        chars
    }
    fn top_extent(&self) -> f32 {
        TEMPO_Y_OFFSET + NOTE_FONT_SIZE * 0.8
    }
    fn render(&self, ctx: &mut RenderCtx, value: &str, x: f32, y_base: f32) {
        let (content, fonts, fallback) = ctx.split();
        let y = y_base + TEMPO_Y_OFFSET;
        let size = NOTE_FONT_SIZE * 0.8;
        render_tempo(content, value, x, y, size, fonts, fallback);
    }
}

/// `#timesig<4/4>`：拍号，与音符同行，占水平空间。SMuFL 拍号数字 + 分数线。
struct TimesigFunc;
impl ControlFunc for TimesigFunc {
    fn width(&self, _value: &str) -> f32 {
        NOTE_GLYPH_WIDTH
    }
    fn top_extent(&self) -> f32 {
        // 分子渲染于 y_base + 0.35em，拍号数字字形上探 ≈ 0.25em
        NOTE_FONT_SIZE * 0.6
    }
    fn bottom_extent(&self) -> f32 {
        // 分母渲染于 y_base - 0.25em，字形下探 ≈ 0.25em
        NOTE_FONT_SIZE * 0.5
    }
    fn prewarm_chars(&self, value: &str) -> Vec<char> {
        if let Some((num, den)) = parse_timesig(value) {
            vec![time_sig::digit(num), time_sig::digit(den)]
        } else {
            Vec::new()
        }
    }
    fn render(&self, ctx: &mut RenderCtx, value: &str, x: f32, y_base: f32) {
        let (content, fonts, fallback) = ctx.split();
        let size = NOTE_FONT_SIZE * 0.85;
        // 分子在上、分母在下，整体垂直居中于音符视觉中心
        let y_top = y_base + NOTE_FONT_SIZE * 0.35;
        let y_bot = y_base - NOTE_FONT_SIZE * 0.25;
        if let Some((num, den)) = parse_timesig(value) {
            if let Some((f, em)) = &mut fonts.leland {
                let num_ch = time_sig::digit(num);
                let den_ch = time_sig::digit(den);
                let (gid_n, _) = match f.glyph_entry(num_ch) {
                    Some(g) => g,
                    None => {
                        render_mixed_text(content, value, x, y_base, size, fonts, fallback);
                        return;
                    }
                };
                let (gid_d, _) = match f.glyph_entry(den_ch) {
                    Some(g) => g,
                    None => {
                        render_mixed_text(content, value, x, y_base, size, fonts, fallback);
                        return;
                    }
                };
                // 用两个数字 bbox x_max 的较大值作为分数线长度（与字符等宽）
                let num_w = f
                    .glyph_bbox(num_ch)
                    .map(|(_, _, xm, _)| xm * size / 1000.0)
                    .unwrap_or(size * 0.6);
                let den_w = f
                    .glyph_bbox(den_ch)
                    .map(|(_, _, xm, _)| xm * size / 1000.0)
                    .unwrap_or(size * 0.6);
                let frac_width = num_w.max(den_w);

                show_glyph(content, em.name, gid_n, x, y_top, size);
                show_glyph(content, em.name, gid_d, x, y_bot, size);

                // 画分数线：与字符等宽
                let y_frac = (y_top + y_bot) / 2.0;
                content.save_state();
                content.set_line_width(1.0);
                content.move_to(x, y_frac);
                content.line_to(x + frac_width, y_frac);
                content.stroke();
                content.restore_state();
                return;
            }
        }
        // 回退：混合文本
        render_mixed_text(content, value, x, y_base, size, fonts, fallback);
    }
}

/// `#dynamics<pp>`：力度记号，渲染于音符下方。优先 Leland SMuFL，回退混合文本。
struct DynamicsFunc;
impl ControlFunc for DynamicsFunc {
    fn prewarm_chars(&self, value: &str) -> Vec<char> {
        dynamics_char(value).into_iter().collect()
    }
    fn bottom_extent(&self) -> f32 {
        // 实测 Leland 力度字形（ff/fff 最深）y_min ≈ -196/1000em，渲染字号
        // 18pt → 下探约 3.5pt；回退文本 descender 约 4.5pt，留 1.5pt 余量。
        // 之前用 28 + 18 = 46 高估了 12pt，导致带力度的行行距虚大。
        DYNAMICS_Y_OFFSET + 6.0
    }
    fn render(&self, ctx: &mut RenderCtx, value: &str, x: f32, y_base: f32) {
        let (content, fonts, fallback) = ctx.split();
        let y = y_base - DYNAMICS_Y_OFFSET;
        let size = NOTE_FONT_SIZE * 1.2;
        // 优先用 Leland SMuFL 力度记号
        if let Some(ch) = dynamics_char(value) {
            if let Some((f, em)) = &mut fonts.leland {
                if let Some((gid, adv)) = f.glyph_entry(ch) {
                    show_glyph(content, em.name, gid, x, y, size);
                    let _ = adv;
                    return;
                }
            }
        }
        // 回退：混合文本
        render_mixed_text(content, value, x, y, size, fonts, fallback);
    }
}

// ============================================
// 注册表
// ============================================

/// 已注册的控制函数表（name → 实现）。新增函数在此加一行即可。
static REGISTRY: &[(&'static str, &'static dyn ControlFunc)] = &[
    ("key", &KeyFunc),
    ("tempo", &TempoFunc),
    ("timesig", &TimesigFunc),
    ("dynamics", &DynamicsFunc),
];

/// 按名字查找控制函数实现。
pub(crate) fn lookup(name: &str) -> Option<&'static dyn ControlFunc> {
    REGISTRY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// 未知控制函数的兜底渲染：`name=value` 文本，位于音符上方（调号位置）。
pub(crate) fn render_default_control(
    ctx: &mut RenderCtx,
    name: &str,
    value: &str,
    x: f32,
    y_base: f32,
) {
    let (content, fonts, fallback) = ctx.split();
    let y = y_base + KEY_Y_OFFSET;
    let size = NOTE_FONT_SIZE * 0.75;
    let text = format!("{}={}", name, value);
    render_mixed_text(content, &text, x, y, size, fonts, fallback);
}

/// 未知控制函数兜底渲染的相对 y_base 向上延伸高度（用于自适应行距）。
pub(crate) fn default_control_extent() -> f32 {
    KEY_Y_OFFSET + NOTE_FONT_SIZE * 0.75
}
