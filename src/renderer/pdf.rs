use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

use crate::parser::{flatten_beam, Accidental, BarlineType, BeamElement, Note, ScoreEvent};
use crate::renderer::font::{
    accidental, char_class, icon_to_smufl, CharClass, FontFamily,
};

// ============================================
// 页面布局常量（A4 纸，单位：PDF 点 = 1/72 英寸）
// ============================================
const PAGE_WIDTH: f32 = 595.0;
const PAGE_HEIGHT: f32 = 842.0;
const MARGIN_X: f32 = 40.0;
const MARGIN_TOP: f32 = 45.0;
const MARGIN_BOTTOM: f32 = 45.0;

// ============================================
// 音符布局常量
// ============================================
/// 音符字体大小
pub(crate) const NOTE_FONT_SIZE: f32 = 15.0;
/// 音符水平间距
const NOTE_SPACING: f32 = 25.0;

// ============================================
// 自适应行距常量
// ============================================
/// 相邻两行内容之间的最小视觉间距（防重叠）
const MIN_LINE_GAP: f32 = 8.0;
/// 页内均匀分配时单个行间隙可额外增加的间距上限（避免行距过散）
const MAX_EXTRA_PER_GAP: f32 = 30.0;
// ============================================
// 视觉元素尺寸
// ============================================
/// 八度点 / 附点直径
const DOT_SIZE: f32 = 2.8;
/// Source Serif Pro Bold 数字大写高度（≈ 0.7em）
const NOTE_CAP_HEIGHT: f32 = NOTE_FONT_SIZE * 0.7;
/// 八度点距音符视觉边缘的间距
const OCT_DOT_GAP: f32 = 1.2;
/// 上下相邻八度点的间距
const OCT_DOT_VGAP: f32 = 0.8;
/// 音符数字字形宽度（Source Serif Pro 数字 advance ≈ 0.52em，粗体略宽取 0.54）
pub(crate) const NOTE_GLYPH_WIDTH: f32 = NOTE_FONT_SIZE * 0.54;
/// 多条减时线之间的间距
const DURATION_LINE_GAP: f32 = 2.8;
/// 减时线距音符基线的偏移（紧贴音符下方）
const DURATION_LINE_Y: f32 = 4.0;
/// 线条粗细
const STROKE_WIDTH: f32 = 0.7;
/// 粗线条粗细（小节线用）
const STROKE_WIDTH_BOLD: f32 = 2.0;
/// 小节线垂直范围（上沿偏移），1.25em
const BARLINE_TOP: f32 = 12.0;
/// 小节线垂直范围（下沿偏移）
const BARLINE_BOTTOM: f32 = 6.0;
/// 和弦内相邻音符的垂直间距（render_chord 堆叠用）
const CHORD_STACK_HEIGHT: f32 = 16.0;
/// 楔形渐强/渐弱（hairpin）与覆盖音符最下层（低八度点/减时线）之间的间距
const HAIRPIN_GAP: f32 = 6.0;
/// 楔形渐强/渐弱的开口半高
const HAIRPIN_HALF: f32 = 3.0;
/// 覆盖音符无减时线/低八度点时，楔形仍保留的最小下移深度（避免紧贴音符基线）
const HAIRPIN_MIN_DEPTH: f32 = 9.0;
/// 装饰音（#grace）字号：主音符的 50%
const GRACE_FONT_SIZE: f32 = NOTE_FONT_SIZE * 0.5;
/// 装饰音音符之间的紧凑间距
const GRACE_SPACING: f32 = 6.0;
/// 装饰音紧贴间距（bracket 组内无空格音符）：不小于数字宽，避免重叠
const GRACE_TIGHT_SPACING: f32 = GRACE_FONT_SIZE * 0.54;
/// 装饰音与主音符之间的间隙
const GRACE_GAP: f32 = 0.0;
/// 装饰音音符相对基线的上抬量（装饰音整体高于主音符基线）
const GRACE_RAISE: f32 = 12.0;
/// 装饰音弧线半径（小弧，围绕装饰音下方）
const GRACE_ARC_RADIUS: f32 = 3.0;
/// 装饰音圆弧与最下层内容（低八度点/减时线）之间的间隙
const GRACE_ARC_GAP: f32 = 2.0;
// 注：KEY_Y_OFFSET / TEMPO_Y_OFFSET / DYNAMICS_Y_OFFSET 已迁至 control.rs（仅控制函数渲染使用）
/// 升降号视觉宽度（用于 layout 估算；渲染时用 bbox 实际值）
/// SMuFL accidental 字形 advance/bbox 远小于常规字符，实际宽度约 0.24em
const ACCIDENTAL_ADVANCE: f32 = NOTE_FONT_SIZE * 0.24;
/// 升降号与音符的紧贴间距
const ACCIDENTAL_NOTE_GAP: f32 = 1.0;
/// 升降号上移偏移（相对于音符基线，使其与音符视觉中心对齐）
const ACCIDENTAL_Y_OFFSET: f32 = NOTE_FONT_SIZE * 0.1;

// ============================================
// 页面标题区常量（#title / #subtitle / #credit）
// ============================================
/// 主标题字号
const TITLE_FONT_SIZE: f32 = 20.0;
/// 副标题字号
const SUBTITLE_FONT_SIZE: f32 = 14.0;
/// 版权行字号
const CREDIT_FONT_SIZE: f32 = 12.0;
/// 标题区各行之间的间距
const TITLE_LINE_GAP: f32 = 4.0;
/// 标题区底部与首行音符之间的间距
const TITLE_AREA_GAP: f32 = 50.0;

/// 将乐谱事件渲染为 PDF 文件。
/// `font_dirs`：可选的字体查找目录，传给字体模块以替代内置的系统字体路径。
/// 错误与警告统一记录到 `diag`（如无事件、字体缺失回退、写文件失败），
/// 不再用 `Result` 中断——调用方通过 `diag.has_error()` 判断是否整体失败。
pub fn render_to_pdf(
    events: &[ScoreEvent],
    output_path: &str,
    font_dirs: &[Option<std::path::PathBuf>],
    diag: &mut crate::diagnostics::Diagnostics,
) {
    // 提取页面元数据（#title/#subtitle/#credit），不参与音符行内布局
    let (meta, note_events) = extract_page_meta(events);
    let title_area = title_area_height(&meta);

    let lines = layout_lines(&note_events);
    if lines.is_empty() {
        diag.error("没有可渲染的事件");
        return;
    }

    let mut pdf = Pdf::new();
    let mut next_ref: i32 = 1;

    let catalog_id = Ref::new(next_ref);
    next_ref += 1;
    let pages_id = Ref::new(next_ref);
    next_ref += 1;

    // 回退字体（Helvetica），在未加载自定义字体时使用
    let fallback_font_id = Ref::new(next_ref);
    next_ref += 1;
    let fallback_font_name = Name(b"F1");
    pdf.type1_font(fallback_font_id).base_font(Name(b"Helvetica"));

    // 收集乐谱中所有控制指令出现的文本字符（含 SMuFL 映射），
    // 在嵌入前预热，确保 PDF 宽度表和 ToUnicode 完整。
    let extra_chars = collect_extra_chars(events);

    // 加载并嵌入 FontFamily（Leland + Source Serif Pro 系列 + 思源宋体）
    let mut fonts = FontFamily::load_and_embed(&mut pdf, &mut next_ref, &extra_chars, font_dirs);

    // 字体缺失警告（回退到 Helvetica，中文/音乐符号会显示异常）
    if fonts.leland.is_none() {
        diag.warn("未找到 Leland 字体：力度/拍号/升降号等 SMuFL 符号将无法渲染");
    }
    if fonts.latin_bold.is_none() {
        diag.warn("未找到 Source Serif Pro Bold：音符数字回退到 Helvetica");
    }
    if fonts.latin.is_none() {
        diag.warn("未找到 Source Serif Pro Regular：西文回退到 Helvetica");
    }
    if fonts.cjk.is_none() {
        diag.warn("未找到思源宋体：中文文本将无法正确渲染");
    }

    // 收集所有页面需要引用的字体资源（name → id）
    let mut font_resources: Vec<(Name<'static>, Ref)> = Vec::new();
    font_resources.push((fallback_font_name, fallback_font_id));
    if let Some((_, em)) = &fonts.leland {
        font_resources.push((em.name, em.id));
    }
    if let Some((_, em)) = &fonts.latin_bold {
        font_resources.push((em.name, em.id));
    }
    if let Some((_, em)) = &fonts.latin {
        font_resources.push((em.name, em.id));
    }
    if let Some((_, em)) = &fonts.cjk {
        font_resources.push((em.name, em.id));
    }

    // ============================================
    // 自适应行距：按每行内容的垂直占用分页，再页内均匀分配
    // ============================================
    // 每行相对 y_base 的垂直占用：(top, bottom)
    let extents: Vec<(f32, f32)> = lines.iter().map(|(line, _)| line_extents(line)).collect();
    let available = PAGE_HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
    // 第一页顶部让出标题区，可用高度相应减少（否则首页会按整页高度分配
    // 行距，导致内容越出下边距、行距虚大）
    let first_page_capacity = (available - title_area).max(MIN_LINE_GAP);

    // 1) 贪心分页：以最小行距累积高度，超限则换页
    let mut pages: Vec<Vec<usize>> = vec![Vec::new()];
    let mut page_top_sum = 0.0f32;
    let mut page_bottom_sum = 0.0f32;
    for i in 0..lines.len() {
        let (top, bottom) = extents[i];
        let n = pages.last().map(|p| p.len()).unwrap_or(0);
        // 首页容量扣除标题区，后续页为整页
        let capacity = if pages.len() == 1 { first_page_capacity } else { available };
        let used = page_top_sum + page_bottom_sum + (n as f32 - 1.0).max(0.0) * MIN_LINE_GAP;
        let would_be = used + top + bottom + if n > 0 { MIN_LINE_GAP } else { 0.0 };
        if n > 0 && would_be > capacity {
            pages.push(vec![i]);
            page_top_sum = top;
            page_bottom_sum = bottom;
        } else {
            pages.last_mut().unwrap().push(i);
            page_top_sum += top;
            page_bottom_sum += bottom;
        }
    }

    // 2) 页内行距：以"自然行距"（防重叠下限）为底，用剩余空间做水塘填充，
    //    使各行基线间距尽可能均匀。之前是"把剩余空间等额加到每个间隙"，
    //    结果带 tempo/dynamics 的高行与普通行之间出现 56~132pt 的落差。
    let mut line_ys: Vec<f32> = Vec::with_capacity(lines.len());
    for (page_idx, page) in pages.iter().enumerate() {
        let page_available = available - if page_idx == 0 { title_area } else { 0.0 };
        // 自然行距：上一行 bottom + 本行 top + 最小间距（内容不重叠的下限）
        let min_gaps: Vec<f32> = page
            .windows(2)
            .map(|w| extents[w[0]].1 + extents[w[1]].0 + MIN_LINE_GAP)
            .collect();
        // 行距总预算：页可用高度 - 首行 top - 末行 bottom
        let budget = (page_available - extents[page[0]].0 - extents[*page.last().unwrap()].1)
            .max(0.0);
        let gaps = even_gaps(&min_gaps, budget);

        // 第一页顶部让出标题区，首行音符整体下移
        let mut y = PAGE_HEIGHT - MARGIN_TOP - if page_idx == 0 { title_area } else { 0.0 };
        for idx in 0..page.len() {
            if idx > 0 {
                y -= gaps[idx - 1];
            }
            line_ys.push(y);
        }
    }

    let mut page_refs = Vec::new();
    for (page_idx, page) in pages.iter().enumerate() {
        let page_ref = Ref::new(next_ref);
        next_ref += 1;
        let content_ref = Ref::new(next_ref);
        next_ref += 1;
        page_refs.push(page_ref);

        let mut content = Content::new();
        content.set_line_width(STROKE_WIDTH);

        // 第一页顶部渲染标题区（#title / #subtitle / #credit，全部使用思源宋体）
        if page_idx == 0 {
            render_page_meta(&mut content, &meta, &mut fonts, fallback_font_name);
        }

        for &li in page {
            let y_base = line_ys[li];
            render_line(
                &mut content,
                &lines[li].0,
                y_base,
                &mut fonts,
                lines[li].1,
                fallback_font_name,
            );
        }

        let content_bytes = content.finish();
        pdf.stream(content_ref, &content_bytes);

        let mut page = pdf.page(page_ref);
        page.media_box(Rect::new(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT));
        page.parent(pages_id);
        page.contents(content_ref);
        let mut res = page.resources();
        let mut fonts_dict = res.fonts();
        for (name, id) in &font_resources {
            fonts_dict.pair(*name, *id);
        }
    }

    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id)
        .kids(page_refs.iter().copied())
        .count(page_refs.len() as i32);

    let buf = pdf.finish();
    if let Err(e) = std::fs::write(output_path, buf) {
        diag.error(format!("写入 PDF 失败：{}", e));
    }
}

// ============================================
// 页面标题区（#title / #subtitle / #credit）
// ============================================

/// 页面元数据：渲染于第一页顶部的文本信息。
struct PageMeta {
    title: Option<String>,
    subtitle: Option<String>,
    credit: Option<String>,
}

/// 从事件流中提取页面元数据（取第一次出现的 #title/#subtitle/#credit），
/// 其余事件原样返回（元数据不参与音符行内布局与分页）。
fn extract_page_meta(events: &[ScoreEvent]) -> (PageMeta, Vec<ScoreEvent>) {
    let mut meta = PageMeta {
        title: None,
        subtitle: None,
        credit: None,
    };
    let mut rest = Vec::with_capacity(events.len());
    for e in events {
        if let ScoreEvent::Control(name, value) = e {
            let slot = match name.as_str() {
                "title" => &mut meta.title,
                "subtitle" => &mut meta.subtitle,
                "credit" => &mut meta.credit,
                _ => {
                    rest.push(e.clone());
                    continue;
                }
            };
            if slot.is_none() {
                *slot = Some(value.clone());
            }
        } else {
            rest.push(e.clone());
        }
    }
    (meta, rest)
}

/// 标题区在页面顶部占用的高度（与 render_page_meta 的 y 排布一致）。
fn title_area_height(meta: &PageMeta) -> f32 {
    let mut h = 0.0;
    if meta.title.is_some() {
        h += TITLE_FONT_SIZE + TITLE_LINE_GAP;
    }
    if meta.subtitle.is_some() {
        h += SUBTITLE_FONT_SIZE + TITLE_LINE_GAP;
    }
    if meta.credit.is_some() {
        h += CREDIT_FONT_SIZE + TITLE_LINE_GAP;
    }
    if h > 0.0 {
        h += TITLE_AREA_GAP;
    }
    h
}

/// 渲染第一页顶部标题区：#title 居中（大号）、#subtitle 居中、#credit 居右。
/// 所有文本强制使用思源宋体（Source Han Serif SC）。
fn render_page_meta(
    content: &mut Content,
    meta: &PageMeta,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    // 标题区从内容区顶部向下排布：title → subtitle → credit（PDF y 向上）
    let mut y = PAGE_HEIGHT - MARGIN_TOP - TITLE_LINE_GAP;
    if let Some(t) = &meta.title {
        let w = measure_text_cjk(t, TITLE_FONT_SIZE, fonts);
        render_text_cjk(content, t, (PAGE_WIDTH - w) / 2.0, y, TITLE_FONT_SIZE, fonts, fallback);
        y -= TITLE_FONT_SIZE + TITLE_LINE_GAP;
    }
    if let Some(s) = &meta.subtitle {
        let w = measure_text_cjk(s, SUBTITLE_FONT_SIZE, fonts);
        render_text_cjk(content, s, (PAGE_WIDTH - w) / 2.0, y, SUBTITLE_FONT_SIZE, fonts, fallback);
        y -= SUBTITLE_FONT_SIZE + TITLE_LINE_GAP;
    }
    if let Some(c) = &meta.credit {
        let w = measure_text_cjk(c, CREDIT_FONT_SIZE, fonts);
        render_text_cjk(content, c, PAGE_WIDTH - MARGIN_X - w, y, CREDIT_FONT_SIZE, fonts, fallback);
    }
}

/// 强制思源宋体渲染文本，返回文本总宽度（PDF 点）。
fn render_text_cjk(
    content: &mut Content,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) -> f32 {
    let mut cursor_x = x;
    for ch in text.chars() {
        if ch == ' ' {
            cursor_x += size * 0.28;
            continue;
        }
        if let Some((f, em)) = &mut fonts.cjk {
            if let Some((gid, adv)) = f.glyph_entry(ch) {
                show_glyph(content, em.name, gid, cursor_x, y, size);
                cursor_x += adv * size / 1000.0;
                continue;
            }
        }
        // 回退：ASCII 用 Helvetica，其余按默认宽度占位
        if ch.is_ascii() {
            show_ascii(content, fallback, ch, cursor_x, y, size);
        }
        cursor_x += size * 0.5;
    }
    cursor_x - x
}

/// 测量思源宋体渲染的文本宽度（与 render_text_cjk 的 advance 一致）。
fn measure_text_cjk(text: &str, size: f32, fonts: &mut FontFamily) -> f32 {
    let mut w = 0.0;
    for ch in text.chars() {
        if ch == ' ' {
            w += size * 0.28;
            continue;
        }
        if let Some((f, _)) = &mut fonts.cjk {
            if let Some((_, adv)) = f.glyph_entry(ch) {
                w += adv * size / 1000.0;
                continue;
            }
        }
        w += size * 0.5;
    }
    w
}

/// 收集乐谱中所有控制指令出现的文本字符（含 SMuFL 映射），
/// 用于在字体嵌入前预热，确保 PDF 宽度表和 ToUnicode 完整。
fn collect_extra_chars(events: &[ScoreEvent]) -> String {
    let mut s = String::new();
    for e in events {
        match e {
            ScoreEvent::Control(name, value) => {
                s.push_str(name);
                s.push('=');
                s.push_str(value);
                // 控制函数特有的 SMuFL 字形预热（#icon / 力度 / 拍号数字等），由各实现提供
                if let Some(f) = super::control::lookup(name) {
                    for ch in f.prewarm_chars(value) {
                        s.push(ch);
                    }
                }
            }
            ScoreEvent::Note(note) => {
                if let Some(acc) = &note.accidental {
                    let ch = match acc {
                        Accidental::Sharp => accidental::SHARP,
                        Accidental::Flat => accidental::FLAT,
                        Accidental::Natural => accidental::NATURAL,
                    };
                    s.push(ch);
                }
            }
            ScoreEvent::Chord(notes) | ScoreEvent::Slur(notes) => {
                for note in notes {
                    if let Some(acc) = &note.accidental {
                        let ch = match acc {
                            Accidental::Sharp => accidental::SHARP,
                            Accidental::Flat => accidental::FLAT,
                            Accidental::Natural => accidental::NATURAL,
                        };
                        s.push(ch);
                    }
                }
            }
            ScoreEvent::Grace(elements) => {
                collect_beam_accidentals(elements, &mut s);
            }
            ScoreEvent::Beam(elements) => {
                collect_beam_accidentals(elements, &mut s);
            }
            _ => {}
        }
    }
    s
}

/// 递归收集 beam 内所有音符的升降号字符（用于字体预热）
fn collect_beam_accidentals(elements: &[BeamElement], s: &mut String) {
    for e in elements {
        match e {
            BeamElement::Note(n, _) => {
                if let Some(acc) = &n.accidental {
                    let ch = match acc {
                        Accidental::Sharp => accidental::SHARP,
                        Accidental::Flat => accidental::FLAT,
                        Accidental::Natural => accidental::NATURAL,
                    };
                    s.push(ch);
                }
            }
            BeamElement::Nested(inner, _) => collect_beam_accidentals(inner, s),
        }
    }
}

// ============================================
// 布局：将事件按行排列
// ============================================

type MusicLine = Vec<(ScoreEvent, f32)>;

/// 紧贴间距系数：未用空格隔开的音符间距 = 正常间距 × 此值
const TIGHT_SPACING_FACTOR: f32 = 0.5;
/// 自然间距：相邻事件之间的视觉空白（不含事件自身视觉宽度）
const NATURAL_GAP: f32 = NOTE_SPACING - NOTE_GLYPH_WIDTH;

/// 事件的视觉宽度（不含尾部间距）
fn event_visual_width(event: &ScoreEvent) -> f32 {
    match event {
        ScoreEvent::Beam(elements) => {
            let (first, last) =
                beam_bounds(elements, 0.0, NOTE_SPACING, NOTE_SPACING * TIGHT_SPACING_FACTOR);
            if first == f32::MIN {
                NOTE_GLYPH_WIDTH
            } else {
                (last - first).max(0.0) + NOTE_GLYPH_WIDTH
            }
        }
        ScoreEvent::Slur(notes) => {
            if notes.is_empty() {
                0.0
            } else {
                let mut w = (notes.len() as f32 - 1.0) * NOTE_SPACING + NOTE_GLYPH_WIDTH;
                if notes.first().map(|n| n.accidental.is_some()).unwrap_or(false) {
                    w += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
                }
                w
            }
        }
        ScoreEvent::Grace(elements) => {
            // 小号音符紧凑排列（含减时线组间距）+ 与主音符的间隙
            let (first, last) = beam_bounds(elements, 0.0, GRACE_SPACING, GRACE_TIGHT_SPACING);
            if first == f32::MIN {
                GRACE_FONT_SIZE * 0.54
            } else {
                (last - first).max(0.0) + GRACE_FONT_SIZE * 0.54 + GRACE_GAP
            }
        }
        ScoreEvent::Control(name, value) => {
            // 通过注册表查询控制函数的布局宽度（如 #timesig 占空间，其他零宽）
            super::control::lookup(name)
                .map(|f| f.width(value))
                .unwrap_or(0.0)
        }
        ScoreEvent::Chord(notes) => {
            let has_acc = notes.iter().any(|n| n.accidental.is_some());
            let mut w = NOTE_GLYPH_WIDTH;
            if has_acc {
                w += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
            }
            w
        }
        ScoreEvent::Barline(_) => 0.0,
        ScoreEvent::Note(note) => {
            let mut w = NOTE_GLYPH_WIDTH;
            if note.accidental.is_some() {
                w += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
            }
            w
        }
        ScoreEvent::Extend => NOTE_GLYPH_WIDTH,
    }
}

/// 单个音符相对 y_base 的垂直占用：(top_extent, bottom_extent)。
/// 与 render_note_body / draw_duration_lines 的 y 计算保持一致，供自适应行距防重叠。
fn note_extents(n: &Note) -> (f32, f32) {
    // 顶部：数字 cap，或高音八度点堆叠上沿
    let mut top = NOTE_CAP_HEIGHT;
    if n.octave > 0 {
        top = NOTE_CAP_HEIGHT + OCT_DOT_GAP + DOT_SIZE
            + (n.octave as f32 - 1.0) * (DOT_SIZE + OCT_DOT_VGAP);
    }
    // 底部：减时线，或低音八度点堆叠下沿
    let mut bottom = 0.0;
    let lc = duration_line_count(n.duration);
    if n.octave < 0 {
        let base_bottom = if lc > 0 {
            DURATION_LINE_Y + (lc as f32 - 1.0) * DURATION_LINE_GAP + OCT_DOT_GAP
        } else {
            OCT_DOT_GAP
        };
        bottom = base_bottom + DOT_SIZE + (-n.octave as f32 - 1.0) * (DOT_SIZE + OCT_DOT_VGAP);
    } else if lc > 0 {
        bottom = DURATION_LINE_Y + (lc as f32 - 1.0) * DURATION_LINE_GAP;
    }
    (top, bottom)
}

/// 递归收集 beam 元素的垂直占用；`max_depth` 记录最深的嵌套层（决定 beam 线底位置）。
fn beam_extents(
    elements: &[BeamElement],
    top: &mut f32,
    bottom: &mut f32,
    max_depth: &mut i32,
    depth: i32,
) {
    *max_depth = (*max_depth).max(depth);
    for e in elements {
        match e {
            BeamElement::Note(n, _) => {
                let (t, b) = note_extents(n);
                *top = top.max(t);
                *bottom = bottom.max(b);
            }
            BeamElement::Nested(inner, _) => {
                beam_extents(inner, top, bottom, max_depth, depth + 1);
            }
        }
    }
}

/// 事件相对 y_base 的垂直占用：(top_extent, bottom_extent)。
fn event_extents(event: &ScoreEvent) -> (f32, f32) {
    match event {
        ScoreEvent::Note(n) => note_extents(n),
        ScoreEvent::Chord(notes) => {
            // 和弦：n 个音符以 CHORD_STACK_HEIGHT 堆叠，上下各扩展 (n-1)*half
            let spread = (notes.len() as f32 - 1.0) * CHORD_STACK_HEIGHT / 2.0;
            let mut top = 0.0f32;
            let mut bottom = 0.0f32;
            for n in notes {
                let (t, b) = note_extents(n);
                top = top.max(t);
                bottom = bottom.max(b);
            }
            (top + spread, bottom + spread)
        }
        ScoreEvent::Beam(elements) => {
            let mut top = 0.0f32;
            let mut bottom = 0.0f32;
            let mut max_depth = 0;
            beam_extents(elements, &mut top, &mut bottom, &mut max_depth, 0);
            // beam 线：y_base - DURATION_LINE_Y - depth*DURATION_LINE_GAP（depth 从 0 起）
            let beam_line_bottom = DURATION_LINE_Y + (max_depth as f32) * DURATION_LINE_GAP;
            (top, bottom.max(beam_line_bottom))
        }
        ScoreEvent::Slur(notes) => {
            // 弧线最高点：slur_base(=cap+4) + peak(9) + thickness(1)
            let mut top = NOTE_CAP_HEIGHT + 4.0 + 9.0 + 1.0;
            let mut bottom = 0.0f32;
            for n in notes {
                let (t, b) = note_extents(n);
                top = top.max(t);
                bottom = bottom.max(b);
            }
            (top, bottom)
        }
        ScoreEvent::Grace(elements) => {
            let notes = flatten_beam(elements);
            // 顶部：上抬量 + 数字 cap（若有高八度点则叠加）
            let mut top = GRACE_RAISE + GRACE_FONT_SIZE * 0.7;
            if let Some(max_oct) = notes.iter().map(|n| n.octave).max() {
                if max_oct > 0 {
                    top = GRACE_RAISE + GRACE_FONT_SIZE * 0.7 + OCT_DOT_GAP + DOT_SIZE
                        + (max_oct as f32 - 1.0) * (DOT_SIZE + OCT_DOT_VGAP);
                }
            }
            // 底部：装饰音上抬较高，数字/减时线/弧线均在基线上方，不占下方空间
            (top, 0.0)
        }
        ScoreEvent::Control(name, _) => {
            if name == "cresc" || name == "dim" {
                // 楔形渐强/渐弱位于覆盖音符最下层之下，由 line_extents 统一计算，此处不占
                (0.0, 0.0)
            } else {
                match super::control::lookup(name) {
                    Some(f) => (f.top_extent(), f.bottom_extent()),
                    // 未知控制函数走兜底渲染（name=value 于调号位置）
                    None => (super::control::default_control_extent(), 0.0),
                }
            }
        }
        ScoreEvent::Barline(_) => (BARLINE_TOP, BARLINE_BOTTOM),
        ScoreEvent::Extend => (NOTE_CAP_HEIGHT, 0.0),
    }
}

/// 整行相对 y_base 的垂直占用：(top_extent, bottom_extent) = 行内各事件的最大值。
fn line_extents(line: &MusicLine) -> (f32, f32) {
    let mut top = 0.0f32;
    let mut bottom = 0.0f32;
    for (event, _) in line {
        let (t, b) = event_extents(event);
        top = top.max(t);
        bottom = bottom.max(b);
    }
    // 楔形渐强/渐弱位于覆盖音符最下层之下：底部再延伸 最小深度 + 间隙 + 开口半高
    if line
        .iter()
        .any(|(e, _)| matches!(e, ScoreEvent::Control(n, _) if n == "cresc" || n == "dim"))
    {
        bottom = bottom.max(HAIRPIN_MIN_DEPTH) + HAIRPIN_GAP + HAIRPIN_HALF;
    }
    (top, bottom)
}

/// 水塘填充（water-filling）：把行距预算尽可能均匀地分配到各行距上。
///
/// `min_gaps` 是各相邻行避免内容重叠所需的最小基线间距，`budget` 是这一页
/// 所有行距之和的预算。返回的行距满足 `gap_i = max(min_gaps[i], level)`：
/// 所有低于水位 `level` 的行距都被抬到同一高度（基线间距一致），
/// 内容特别高的行（如带 tempo/dynamics）保持各自的自然下限、不强行压缩
/// 以免重叠。单个行距的上浮量设上限，避免行数很少的页面行距失控，
/// 剩余空间留在页底。
fn even_gaps(min_gaps: &[f32], budget: f32) -> Vec<f32> {
    let n = min_gaps.len();
    let mut gaps = min_gaps.to_vec();
    if n == 0 {
        return gaps;
    }
    let base_sum: f32 = gaps.iter().sum();
    let mut extra = (budget - base_sum).max(0.0);
    if extra <= 0.0 {
        return gaps;
    }

    // 按自然行距升序，逐层抬高到次小值（水位填充）
    let mut sorted = min_gaps.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut level = sorted[0];
    let mut i = 0;
    while i < n {
        let next = if i + 1 < n { sorted[i + 1] } else { f32::INFINITY };
        let cost = (next - level) * (i + 1) as f32;
        if cost <= extra {
            extra -= cost;
            level = next;
            i += 1;
        } else {
            level += extra / (i + 1) as f32;
            break;
        }
    }

    // 应用水位；单个行距上浮不超过上限
    let extra_cap = MAX_EXTRA_PER_GAP * 2.0;
    for g in gaps.iter_mut() {
        let target = (*g).max(level);
        *g += (target - *g).min(extra_cap);
    }
    gaps
}

/// 判断事件是否占 y_base 行的水平空间（需要推进 cursor）。
/// 小节线 visual_width=0 但渲染在 y_base 行，需要间距；
/// key/tempo/dynamics 等渲染在上方/下方，不占 y_base 行空间。
fn is_real_event(event: &ScoreEvent) -> bool {
    event_visual_width(event) > 0.0 || matches!(event, ScoreEvent::Barline(_))
}

/// 事件占用的总水平宽度（视觉宽度 + 自然间距），用于第一遍换行计算
/// 零宽控制指令（key/tempo/dynamics）不占间距，避免结构性空白
fn event_width(event: &ScoreEvent) -> f32 {
    if is_real_event(event) {
        // 装饰音紧贴其后主音符：不附加自然间距（GRACE_GAP 已含在视觉宽度内）
        if matches!(event, ScoreEvent::Grace(_)) {
            event_visual_width(event)
        } else {
            event_visual_width(event) + NATURAL_GAP
        }
    } else {
        0.0
    }
}

/// 将事件按小节分组（遇到小节线则结束当前组，小节线包含在组内）
fn group_measures(events: &[ScoreEvent]) -> Vec<Vec<ScoreEvent>> {
    let mut measures: Vec<Vec<ScoreEvent>> = Vec::new();
    let mut current: Vec<ScoreEvent> = Vec::new();

    for event in events {
        let is_barline = matches!(event, ScoreEvent::Barline(_));
        current.push(event.clone());
        if is_barline {
            measures.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        measures.push(current);
    }
    measures
}

/// 布局：返回 (每行事件，每行实际间距)
fn layout_lines(events: &[ScoreEvent]) -> Vec<(MusicLine, f32)> {
    let measures = group_measures(events);
    let max_x = PAGE_WIDTH - MARGIN_X;

    // 第一遍：用自然间距确定每行放哪些小节（末行位置直接保留）
    let mut raw_lines: Vec<MusicLine> = Vec::new();
    let mut current_line: MusicLine = Vec::new();
    let mut x = MARGIN_X;

    for measure in &measures {
        let measure_width: f32 = measure.iter().map(event_width).sum();

        if !current_line.is_empty() && x + measure_width > max_x {
            raw_lines.push(std::mem::take(&mut current_line));
            x = MARGIN_X;
        }

        for event in measure {
            current_line.push((event.clone(), x));
            x += event_width(event);
        }
    }
    if !current_line.is_empty() {
        raw_lines.push(current_line);
    }

    // 第二遍：对非末行拉伸间距，使最后一个小节线对齐右页边距
    let right_margin = PAGE_WIDTH - MARGIN_X;
    let last_idx = raw_lines.len().saturating_sub(1);
    let mut result = Vec::with_capacity(raw_lines.len());

    for (line_idx, line) in raw_lines.into_iter().enumerate() {
        // 只统计实事件之间的间隔数，零宽控制指令（key/tempo/dynamics）与
        // 紧贴型装饰音（#grace）不参与 gap 分配
        let real_count = line
            .iter()
            .filter(|(e, _)| is_real_event(e) && !matches!(e, ScoreEvent::Grace(_)))
            .count();
        let n_gaps = real_count.saturating_sub(1);

        if line_idx != last_idx && n_gaps > 0 {
            let total_visual: f32 = line.iter().map(|(e, _)| event_visual_width(e)).sum();
            let gap = (right_margin - MARGIN_X - total_visual) / n_gaps as f32;
            let slot = NOTE_GLYPH_WIDTH + gap;
            let mut cursor = MARGIN_X;
            let stretched: MusicLine = line
                .into_iter()
                .map(|(event, _)| {
                    let pos = cursor;
                    // 零宽控制指令不推进 cursor（附着到下一个实事件的同一 x 位置）；
                    // 装饰音紧贴主音符，只推进自身宽度、不参与 gap 拉伸
                    if is_real_event(&event) {
                        if matches!(event, ScoreEvent::Grace(_)) {
                            cursor += event_visual_width(&event);
                        } else {
                            cursor += event_visual_width(&event) + gap;
                        }
                    }
                    (event, pos)
                })
                .collect();
            result.push((stretched, slot));
        } else {
            result.push((line, NOTE_SPACING));
        }
    }

    result
}

// ============================================
// 逐行渲染
// ============================================

fn render_line(
    content: &mut Content,
    line: &MusicLine,
    y_base: f32,
    fonts: &mut FontFamily,
    spacing: f32,
    fallback: Name,
) {
    // header_cursor：用于将 key/timesig 在同一行并排排列（避免零宽度导致重叠）
    let mut header_cursor: Option<f32> = None;

    for (i, (event, x)) in line.iter().enumerate() {
        match event {
            ScoreEvent::Control(name, value) => {
                if name == "cresc" || name == "dim" {
                    // 楔形渐强/渐弱：跨事件的横跨标记（参数内音符已展开在前方事件流），
                    // 不参与 header 并排
                    header_cursor = None;
                    render_hairpin(content, line, i, name, value, y_base);
                } else {
                    match super::control::lookup(name) {
                        Some(f) => {
                            let render_x = if let Some(adv) = f.header_advance() {
                                // header 组（如连续调号 #key）：在 header_cursor 渲染并推进
                                let rx = header_cursor.unwrap_or(*x);
                                header_cursor = Some(rx + adv);
                                rx
                            } else {
                                // 占空间的控制指令（如 #timesig）重置 header_cursor；
                                // 零宽附着型（#tempo/#dynamics）保持 header_cursor 不变
                                if f.width(value) > 0.0 {
                                    header_cursor = None;
                                }
                                *x
                            };
                            let mut ctx = super::control::RenderCtx {
                                content: &mut *content,
                                fonts: &mut *fonts,
                                fallback,
                            };
                            f.render(&mut ctx, value, render_x, y_base);
                        }
                        None => {
                            // 未知控制函数：兜底渲染，不影响 header_cursor
                            let mut ctx = super::control::RenderCtx {
                                content: &mut *content,
                                fonts: &mut *fonts,
                                fallback,
                            };
                            super::control::render_default_control(&mut ctx, name, value, *x, y_base);
                        }
                    }
                }
            }
            ScoreEvent::Note(note) => {
                header_cursor = None;
                render_note(content, note, *x, y_base, fonts, fallback);
            }
            ScoreEvent::Chord(notes) => {
                header_cursor = None;
                render_chord(content, notes, *x, y_base, fonts, fallback);
            }
            ScoreEvent::Beam(notes) => {
                header_cursor = None;
                render_beam(content, notes, *x, y_base, fonts, fallback, spacing);
            }
            ScoreEvent::Slur(notes) => {
                header_cursor = None;
                render_slur(content, notes, *x, y_base, fonts, fallback);
            }
            ScoreEvent::Grace(elements) => {
                header_cursor = None;
                render_grace(content, elements, *x, y_base, fonts, fallback);
            }
            ScoreEvent::Barline(bt) => {
                header_cursor = None;
                render_barline(content, bt, *x, y_base);
            }
            ScoreEvent::Extend => {
                header_cursor = None;
                render_extend(content, *x, y_base, fonts, fallback);
            }
        }
    }
}

// ============================================
// 渲染单个音符
// ============================================

fn render_note(
    content: &mut Content,
    note: &Note,
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    // 升降号紧贴音符左侧：x 是块起点，升降号在 x，数字在 x + acc_offset
    // note: 推进用 ACCIDENTAL_ADVANCE 常量而非实际 advance，保持与 event_visual_width 一致
    let mut digit_x = x;
    if let Some(acc) = &note.accidental {
        render_accidental(content, acc, digit_x, y_base, fonts);
        digit_x += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
    }
    render_note_body(content, note, digit_x, y_base, fonts, fallback, NOTE_FONT_SIZE);
    draw_duration_lines(content, note.duration, digit_x, digit_x + NOTE_GLYPH_WIDTH, y_base);
}

/// 渲染音符主体（数字 + 八度点 + 附点），不含减时线。
/// 音符数字使用 **Source Serif Pro Bold（粗体衬线）** 渲染。
/// `size` 为字号；装饰音等小号音符传入 `GRACE_FONT_SIZE`。
fn render_note_body(
    content: &mut Content,
    note: &Note,
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
    size: f32,
) {
    let digit = if note.is_rest() { '0' } else {
        char::from_digit(note.pitch as u32, 10).unwrap_or('0')
    };

    // 1. 渲染数字（粗体字体 Source Serif Pro Bold，回退 Helvetica）
    let mut rendered = false;
    if let Some((f, em)) = &mut fonts.latin_bold {
        if let Some((gid, _)) = f.glyph_entry(digit) {
            show_glyph(content, em.name, gid, x, y_base, size);
            rendered = true;
        }
    }
    if !rendered {
        show_ascii(content, fallback, digit, x, y_base, size);
    }

    let cap = size * 0.7;
    let digit_w = size * 0.54;
    let dot_center_x = x + digit_w / 2.0;
    // 点/间距随字号缩放（装饰音小号 → 八度点更小、更紧凑）
    let scale = size / NOTE_FONT_SIZE;
    let dot_size = DOT_SIZE * scale;
    let oct_gap = OCT_DOT_GAP * scale;
    let oct_vgap = OCT_DOT_VGAP * scale;
    let dur_y = DURATION_LINE_Y * scale;
    let dur_gap = DURATION_LINE_GAP * scale;

    // 2. 高音八度点（音符视觉上沿之上，垂直堆叠）
    if note.octave > 0 {
        for i in 0..note.octave {
            let dot_y = y_base + cap + oct_gap + dot_size / 2.0
                + (i as f32) * (dot_size + oct_vgap);
            draw_dot(content, dot_center_x, dot_y, dot_size);
        }
    }

    // 3. 低音八度点（紧贴减时线/音符视觉下沿之下，垂直堆叠）
    if note.octave < 0 {
        let line_count = duration_line_count(note.duration);
        let base_bottom = if line_count > 0 {
            y_base - dur_y - (line_count as f32 - 1.0) * dur_gap - oct_gap
        } else {
            y_base - oct_gap
        };
        for i in 0..(-note.octave) {
            let dot_y = base_bottom - dot_size / 2.0 - (i as f32) * (dot_size + oct_vgap);
            draw_dot(content, dot_center_x, dot_y, dot_size);
        }
    }

    // 4. 附点（数字右侧，下端与音符下端对齐）
    if note.dotted {
        let dot_x = x + size * 0.62;
        let dot_y = y_base + dot_size / 2.0;
        draw_dot(content, dot_x, dot_y, dot_size);
    }
}

/// 绘制减时线（可指定起止 x 坐标实现连续连线）
fn draw_duration_lines(
    content: &mut Content,
    duration: u32,
    x_start: f32,
    x_end: f32,
    y_base: f32,
) {
    let line_count = duration_line_count(duration);
    for i in 0..line_count {
        let line_y = y_base - DURATION_LINE_Y - (i as f32) * DURATION_LINE_GAP;
        content.move_to(x_start, line_y);
        content.line_to(x_end, line_y);
        content.stroke();
    }
}

/// 根据 duration（倒数表示）计算减时线条数。
fn duration_line_count(duration: u32) -> i32 {
    if duration <= 4 {
        return 0;
    }
    let mut count = 0;
    let mut d = duration / 4;
    while d > 1 {
        count += 1;
        d /= 2;
    }
    count
}

// ============================================
// 渲染减时线组（Beam：连续减时线相连）
// ============================================

fn render_beam(
    content: &mut Content,
    elements: &[BeamElement],
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
    spacing: f32,
) {
    let tight_spacing = spacing * TIGHT_SPACING_FACTOR;

    let mut x_cursor = x;
    render_beam_notes(
        content,
        elements,
        &mut x_cursor,
        y_base,
        fonts,
        fallback,
        spacing,
        tight_spacing,
    );

    draw_beam_lines(
        content,
        elements,
        x,
        y_base,
        0,
        spacing,
        tight_spacing,
    );
}

fn render_beam_notes(
    content: &mut Content,
    elements: &[BeamElement],
    x_cursor: &mut f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
    spacing: f32,
    tight_spacing: f32,
) {
    for (i, element) in elements.iter().enumerate() {
        if i > 0 {
            *x_cursor += if element.is_tight() {
                tight_spacing
            } else {
                spacing
            };
        }
        match element {
            BeamElement::Note(note, _) => {
                // 升降号紧贴音符左侧：先渲染升降号再推进（推进值与 layout 常量一致）
                if let Some(acc) = &note.accidental {
                    render_accidental(content, acc, *x_cursor, y_base, fonts);
                    *x_cursor += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
                }
                render_note_body(content, note, *x_cursor, y_base, fonts, fallback, NOTE_FONT_SIZE);
            }
            BeamElement::Nested(inner, _) => {
                render_beam_notes(
                    content,
                    inner,
                    x_cursor,
                    y_base,
                    fonts,
                    fallback,
                    spacing,
                    tight_spacing,
                );
            }
        }
    }
}

fn draw_beam_lines(
    content: &mut Content,
    elements: &[BeamElement],
    x_start: f32,
    y_base: f32,
    depth: i32,
    spacing: f32,
    tight_spacing: f32,
) {
    let (first_x, last_x) = beam_bounds(elements, x_start, spacing, tight_spacing);
    if first_x == f32::MIN {
        return;
    }

    let line_y = y_base - DURATION_LINE_Y - (depth as f32) * DURATION_LINE_GAP;
    content.move_to(first_x, line_y);
    content.line_to(last_x + NOTE_GLYPH_WIDTH, line_y);
    content.stroke();

    let mut x_cursor = x_start;
    for (i, element) in elements.iter().enumerate() {
        if i > 0 {
            x_cursor += if element.is_tight() {
                tight_spacing
            } else {
                spacing
            };
        }
        match element {
            BeamElement::Nested(inner, _) => {
                draw_beam_lines(
                    content,
                    inner,
                    x_cursor,
                    y_base,
                    depth + 1,
                    spacing,
                    tight_spacing,
                );
                let (_, inner_last) = beam_bounds(inner, x_cursor, spacing, tight_spacing);
                x_cursor = inner_last;
            }
            BeamElement::Note(note, _) => {
                // 升降号占位推进，保持与 render_beam_notes 一致
                if note.accidental.is_some() {
                    x_cursor += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
                }
            }
        }
    }
}

fn beam_bounds(
    elements: &[BeamElement],
    x_start: f32,
    spacing: f32,
    tight_spacing: f32,
) -> (f32, f32) {
    let mut x_cursor = x_start;
    let mut first_x = f32::MIN;
    let mut last_x = x_start;

    for (i, element) in elements.iter().enumerate() {
        if i > 0 {
            x_cursor += if element.is_tight() {
                tight_spacing
            } else {
                spacing
            };
        }
        match element {
            BeamElement::Note(note, _) => {
                // 升降号占位：块的左边沿是升降号位置，推进 cursor 到数字位置
                let block_left = if note.accidental.is_some() {
                    let left = x_cursor;
                    x_cursor += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
                    left
                } else {
                    x_cursor
                };
                if first_x == f32::MIN {
                    first_x = block_left;
                }
                last_x = x_cursor;
            }
            BeamElement::Nested(inner, _) => {
                let (inner_first, inner_last) =
                    beam_bounds(inner, x_cursor, spacing, tight_spacing);
                if first_x == f32::MIN {
                    first_x = inner_first;
                }
                last_x = inner_last;
                x_cursor = inner_last;
            }
        }
    }
    (first_x, last_x)
}

// ============================================
// 渲染连线组（Slur：音符上方弧线）
// ============================================

fn render_slur(
    content: &mut Content,
    notes: &[Note],
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    // 连线组作为语义整体，内部音符间距固定为 NOTE_SPACING（与 event_visual_width 一致），
    // 不随行拉伸变化；否则 stretched 行中弧线渲染会超出布局分配的块，导致后续音符与之重叠。
    // 连线内第一个音符若有升降号，在起点渲染并整体偏移
    // 注意：offset 用 ACCIDENTAL_ADVANCE 常量而非实际 advance，保持与 event_visual_width 一致，避免后续音符错位
    let mut offset = 0.0;
    if let Some(acc) = notes.first().and_then(|n| n.accidental) {
        render_accidental(content, &acc, x, y_base, fonts);
        offset = ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
    }

    for (i, note) in notes.iter().enumerate() {
        let note_x = x + offset + (i as f32) * NOTE_SPACING;
        render_note_body(content, note, note_x, y_base, fonts, fallback, NOTE_FONT_SIZE);
        draw_duration_lines(content, note.duration, note_x, note_x + NOTE_GLYPH_WIDTH, y_base);
    }

    if notes.len() >= 2 {
        let half_glyph = NOTE_GLYPH_WIDTH / 2.0;
        let start_x = x + offset + half_glyph;
        let end_x = x + offset + (notes.len() as f32 - 1.0) * NOTE_SPACING + half_glyph;
        let slur_base = y_base + NOTE_CAP_HEIGHT + 4.0;
        let slur_peak = slur_base + 9.0;
        // 上下两条贝塞尔曲线，两端交汇于 slur_base 形成尖角，中间填充
        let thickness = 1.0;
        let ctrl_upper = slur_peak + thickness;
        let ctrl_lower = slur_peak - thickness;

        content.save_state();
        content.move_to(start_x, slur_base);
        content.cubic_to(start_x, ctrl_upper, end_x, ctrl_upper, end_x, slur_base);
        content.cubic_to(end_x, ctrl_lower, start_x, ctrl_lower, start_x, slur_base);
        content.fill_nonzero();
        content.restore_state();
    }
}

// ============================================
// 渲染装饰音（Grace：主音符前的小号音符组）
// ============================================

/// 渲染装饰音（`#grace<67>` / `#grace<[1[23]]>`）：小号音符上抬到基线上方、紧凑排列，
/// 同减时线组内的减时线连续相连（复用 beam 连线逻辑），
/// 在装饰音下方画一条 180°~270° 的 1/4 圆弧（圆心在装饰音基线，从组左端弯到组中心正下方）。
fn render_grace(
    content: &mut Content,
    elements: &[BeamElement],
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    let size = GRACE_FONT_SIZE;
    let note_y = y_base + GRACE_RAISE;

    // 1) 小号音符（数字 + 八度点 + 附点），复用 render_note_body
    let mut x_cursor = x;
    render_grace_notes(content, elements, &mut x_cursor, note_y, size, fonts, fallback);

    // 2) 减时线：同减时线组内连续相连
    draw_grace_lines(content, elements, x, note_y, 0, size);

    // 3) 小弧（180°~270° 的 1/4 圆弧）：位于装饰音最下层内容（低八度点/减时线）
    //    下方，留出小间隙。旧逻辑只以"高八度点/数字基线"为参考：低八度音符的
    //    点会与圆弧重叠（打架）；间隙又过大，让装饰音与圆弧脱节（浮起）。
    //    注意 PDF 坐标 y 向上：圆弧左端在 cy（最高），底部在 cy - r（最低）。
    let (first, last) = beam_bounds(elements, x, GRACE_SPACING, GRACE_TIGHT_SPACING);
    if first != f32::MIN {
        let scale = size / NOTE_FONT_SIZE;
        let deepest = grace_deepest(elements, size);
        // 圆心 x = 装饰音组中心向右平移两个半径（靠近主音符方向）
        let cx = (first + last) / 2.0 + 2.0 * GRACE_ARC_RADIUS;
        // 圆心 y：圆弧顶部（左端，y = cy）贴在最下层内容下方
        let cy = note_y - deepest - GRACE_ARC_GAP;
        let r = GRACE_ARC_RADIUS;
        // 三次贝塞尔近似 1/4 圆弧（k = 0.5523）：从 180°（正左）弯到 270°（正下）
        let p0 = (cx - r, cy);
        let p1 = (cx, cy - r);
        let k = 0.5523;
        let c1 = (p0.0, p0.1 - k * r);
        let c2 = (p1.0 - k * r, p1.1);
        content.save_state();
        content.set_line_width(STROKE_WIDTH * scale); // 弧线粗细随字号缩小
        content.move_to(p0.0, p0.1);
        content.cubic_to(c1.0, c1.1, c2.0, c2.1, p1.0, p1.1);
        content.stroke();
        content.restore_state();
    }
}

/// 装饰音组内最下层内容相对 note_y 的下探深度（低八度点、减时线）。
/// 与 render_note_body / draw_grace_lines 的 y 计算保持一致，
/// 用于定位装饰音下方的圆弧，避免圆弧与音符重叠。
fn grace_deepest(elements: &[BeamElement], size: f32) -> f32 {
    let scale = size / NOTE_FONT_SIZE;
    let dot_size = DOT_SIZE * scale;
    let oct_gap = OCT_DOT_GAP * scale;
    let oct_vgap = OCT_DOT_VGAP * scale;
    let dur_y = DURATION_LINE_Y * scale;
    let dur_gap = DURATION_LINE_GAP * scale;
    let mut deepest = 0.0f32;
    for n in flatten_beam(elements) {
        // 减时线：按该音符时值对应的线数，最深层线在 note_y - dur_y - (lc-1)*dur_gap
        let lc = duration_line_count(n.duration);
        let line_bottom = if lc > 0 { dur_y + (lc as f32 - 1.0) * dur_gap } else { 0.0 };
        // 低八度点：位于减时线（若有）下方，逐点堆叠
        let note_bottom = if n.octave < 0 {
            let base = if lc > 0 { line_bottom + oct_gap } else { oct_gap };
            base + (-n.octave as f32 - 1.0) * (dot_size + oct_vgap) + dot_size
        } else {
            line_bottom
        };
        deepest = deepest.max(note_bottom);
    }
    deepest
}

/// 递归渲染装饰音小号音符（间距与 draw_grace_lines 的游标推进保持一致）
fn render_grace_notes(
    content: &mut Content,
    elements: &[BeamElement],
    x_cursor: &mut f32,
    note_y: f32,
    size: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    for (i, e) in elements.iter().enumerate() {
        if i > 0 {
            *x_cursor += if e.is_tight() {
                GRACE_TIGHT_SPACING
            } else {
                GRACE_SPACING
            };
        }
        match e {
            BeamElement::Note(n, _) => {
                render_note_body(content, n, *x_cursor, note_y, fonts, fallback, size);
            }
            BeamElement::Nested(inner, _) => {
                render_grace_notes(content, inner, x_cursor, note_y, size, fonts, fallback);
            }
        }
    }
}

/// 装饰音减时线：bracket 组（Nested）画连续组线，组外独立音符画各自减时线。
/// 与 beam 连线逻辑一致：组线按嵌套深度递增。间距/空隙/线宽随字号缩放。
fn draw_grace_lines(
    content: &mut Content,
    elements: &[BeamElement],
    x_start: f32,
    note_y: f32,
    depth: i32,
    size: f32,
) {
    let scale = size / NOTE_FONT_SIZE;
    let digit_w = size * 0.54;
    let dur_y = DURATION_LINE_Y * scale;
    let dur_gap = DURATION_LINE_GAP * scale;
    let line_width = STROKE_WIDTH * scale;
    let mut x_cursor = x_start;
    for (i, e) in elements.iter().enumerate() {
        if i > 0 {
            x_cursor += if e.is_tight() {
                GRACE_TIGHT_SPACING
            } else {
                GRACE_SPACING
            };
        }
        match e {
            BeamElement::Nested(inner, _) => {
                // 组线：从组首到组尾连续相连
                let (inner_first, inner_last) =
                    beam_bounds(inner, x_cursor, GRACE_SPACING, GRACE_TIGHT_SPACING);
                if inner_first != f32::MIN {
                    let line_y = note_y - dur_y - (depth as f32) * dur_gap;
                    content.save_state();
                    content.set_line_width(line_width);
                    content.move_to(inner_first, line_y);
                    content.line_to(inner_last + digit_w, line_y);
                    content.stroke();
                    content.restore_state();
                }
                draw_grace_lines(content, inner, x_cursor, note_y, depth + 1, size);
                let (_, inner_last) = beam_bounds(inner, x_cursor, GRACE_SPACING, GRACE_TIGHT_SPACING);
                x_cursor = inner_last;
            }
            BeamElement::Note(n, _) => {
                if n.accidental.is_some() {
                    x_cursor += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
                }
                // 组外独立音符画各自的减时线（组内音符的线由组线覆盖）
                if depth == 0 {
                    let lc = duration_line_count(n.duration);
                    for li in 0..lc {
                        let line_y = note_y - dur_y - (li as f32) * dur_gap;
                        content.save_state();
                        content.set_line_width(line_width);
                        content.move_to(x_cursor, line_y);
                        content.line_to(x_cursor + digit_w, line_y);
                        content.stroke();
                        content.restore_state();
                    }
                }
            }
        }
    }
}

// ============================================
// 渲染和弦（音符垂直堆叠）
// ============================================

fn render_chord(
    content: &mut Content,
    notes: &[Note],
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    let mut sorted = notes.to_vec();
    sorted.sort_by(|a, b| b.pitch.cmp(&a.pitch));

    // 和弦内任一音符有升降号，统一在左侧渲染一次并整体偏移
    let acc = sorted.iter().find_map(|n| n.accidental);
    let mut digit_x = x;
    if let Some(acc) = acc {
        render_accidental(content, &acc, digit_x, y_base, fonts);
        digit_x += ACCIDENTAL_ADVANCE + ACCIDENTAL_NOTE_GAP;
    }

    let stack_height = CHORD_STACK_HEIGHT;
    let start_y = y_base + (sorted.len() as f32 - 1.0) * stack_height / 2.0;

    for (i, note) in sorted.iter().enumerate() {
        let y = start_y - (i as f32) * stack_height;
        render_note_body(content, note, digit_x, y, fonts, fallback, NOTE_FONT_SIZE);
        draw_duration_lines(content, note.duration, digit_x, digit_x + NOTE_GLYPH_WIDTH, y);
    }
}

// ============================================
// 渲染升降还原记号
// ============================================

/// 渲染升降还原记号（从 Leland SMuFL 字体直接取字形），返回实际视觉宽度（PDF 点）
/// SMuFL accidental 字形的 advance 远小于轮廓宽度，用 bbox x_max 推断视觉宽度。
fn render_accidental(
    content: &mut Content,
    acc: &Accidental,
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
) -> f32 {
    let ch = match acc {
        Accidental::Sharp => accidental::SHARP,
        Accidental::Flat => accidental::FLAT,
        Accidental::Natural => accidental::NATURAL,
    };
    let size = NOTE_FONT_SIZE * 0.9;
    if let Some((f, em)) = &mut fonts.leland {
        if let Some((gid, _)) = f.glyph_entry(ch) {
            // SMuFL accidental 的 advance 远小于轮廓宽度，用 bbox x_max 推断实际视觉宽度
            let visual_width = f
                .glyph_bbox(ch)
                .map(|(_, _, x_max, _)| x_max * size / 1000.0)
                .unwrap_or(ACCIDENTAL_ADVANCE);
            show_glyph(content, em.name, gid, x, y_base + ACCIDENTAL_Y_OFFSET, size);
            return visual_width;
        }
    }
    ACCIDENTAL_ADVANCE
}

/// 渲染 tempo。支持以下形式：
///   1. 节拍器形式：`#icon<note_4> = 120`（图标用 Leland SMuFL 节拍器记号）
///   2. 意大利术语："Moderato"、"Andante" 等（Source Serif Pro Regular 西文）
///   3. 中文速度："中速"、"快板"、"稍快" 等（思源宋体）
///   4. 混合：`#icon<note_4> = 120 Moderato / 中速`（多字体自动切换）
///
/// 通过扫描 `#icon<name>` 模式直接从 Leland 渲染音乐字符，
/// 彻底规避 Unicode 字符在多字体间的映射冲突。
pub(crate) fn render_tempo(
    content: &mut Content,
    value: &str,
    x: f32,
    y: f32,
    size: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    let mut cursor_x = x;
    let mut rest = value;

    while !rest.is_empty() {
        // 检测 #icon<name> 模式
        if rest.starts_with("#icon<") {
            if let Some(end) = rest.find('>') {
                let icon_name = &rest["#icon<".len()..end];
                if let Some(smufl_chars) = icon_to_smufl(icon_name) {
                    for &smufl_ch in smufl_chars {
                        if let Some((f, em)) = &mut fonts.leland {
                            if let Some((gid, adv)) = f.glyph_entry(smufl_ch) {
                                show_glyph(content, em.name, gid, cursor_x, y, size);
                                cursor_x += adv * size / 1000.0;
                            }
                        }
                    }
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }

        // 普通字符：按西文/非西文选择字体
        let ch = rest.chars().next().unwrap();
        if ch == ' ' {
            cursor_x += size * 0.28;
        } else {
            cursor_x += render_one_char(content, ch, cursor_x, y, size, fonts, fallback);
        }
        rest = &rest[ch.len_utf8()..];
    }
}

/// 渲染混合文本（西文→Source Serif Pro Regular；非西文→思源宋体；都没有→回退 Helvetica）
/// 每个字符独立 begin_text/end_text，避免字体冲突。
pub(crate) fn render_mixed_text(
    content: &mut Content,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if ch == ' ' {
            cursor_x += size * 0.28;
            continue;
        }
        cursor_x += render_one_char(content, ch, cursor_x, y, size, fonts, fallback);
    }
}

/// 渲染单个字符（独立文本对象），返回 advance（PDF 点单位）。
pub(crate) fn render_one_char(
    content: &mut Content,
    ch: char,
    x: f32,
    y: f32,
    size: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) -> f32 {
    let cls = char_class(ch);
    let default_adv = size * 0.5;

    match cls {
        CharClass::Ascii => {
            // 西文：优先 Source Serif Pro Regular
            if let Some((f, em)) = &mut fonts.latin {
                if let Some((gid, adv)) = f.glyph_entry(ch) {
                    show_glyph(content, em.name, gid, x, y, size);
                    return adv * size / 1000.0;
                }
            }
            // 回退：尝试粗体
            if let Some((f, em)) = &mut fonts.latin_bold {
                if let Some((gid, adv)) = f.glyph_entry(ch) {
                    show_glyph(content, em.name, gid, x, y, size);
                    return adv * size / 1000.0;
                }
            }
            // 最后回退：Helvetica 单字节文本
            if ch.is_ascii() {
                show_ascii(content, fallback, ch, x, y, size);
            }
            default_adv
        }
        CharClass::NonAscii => {
            // 非西文：优先思源宋体 CJK
            if let Some((f, em)) = &mut fonts.cjk {
                if let Some((gid, adv)) = f.glyph_entry(ch) {
                    show_glyph(content, em.name, gid, x, y, size);
                    return adv * size / 1000.0;
                }
            }
            // 回退：试试 Leland（可能是音乐符号）
            if let Some((f, em)) = &mut fonts.leland {
                if let Some((gid, adv)) = f.glyph_entry(ch) {
                    show_glyph(content, em.name, gid, x, y, size);
                    return adv * size / 1000.0;
                }
            }
            // 最后回退（非 ASCII 可能无法渲染，用占位宽度）
            default_adv
        }
    }
}

/// 以 Identity-H 编码渲染单个字形（独立 begin_text/end_text）
pub(crate) fn show_glyph(content: &mut Content, font_name: Name, gid: u16, x: f32, y: f32, size: f32) {
    let bytes = [(gid >> 8) as u8, (gid & 0xFF) as u8];
    content.begin_text();
    content.set_font(font_name, size);
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, y]);
    content.show(Str(&bytes));
    content.end_text();
}

/// 以单字节 ASCII 编码渲染单个字符（Helvetica 回退用）
pub(crate) fn show_ascii(content: &mut Content, font_name: Name, ch: char, x: f32, y: f32, size: f32) {
    let buf = [ch as u8];
    content.begin_text();
    content.set_font(font_name, size);
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, y]);
    content.show(Str(&buf));
    content.end_text();
}

/// 解析拍号字符串 "N/D" → (N, D)
pub(crate) fn parse_timesig(s: &str) -> Option<(u8, u8)> {
    let parts: Vec<&str> = s.trim().split('/').collect();
    if parts.len() == 2 {
        let num = parts[0].trim().parse().ok()?;
        let den = parts[1].trim().parse().ok()?;
        Some((num, den))
    } else {
        None
    }
}

// ============================================
// 渲染楔形渐强/渐弱（Hairpin）
// ============================================

/// 渲染楔形渐强/渐弱记号（`#cresc<...>` / `#dim<...>`）。
/// 参数内音符已由 parser 展开进主事件流；value 记录参数内实事件数量，
/// 此处从 `line[idx+1..]` 扫描这些事件的 x 范围作为楔形跨度。
fn render_hairpin(
    content: &mut Content,
    line: &MusicLine,
    idx: usize,
    name: &str,
    value: &str,
    y_base: f32,
) {
    let n: usize = value.trim().parse().unwrap_or(0);
    let mut first_start: Option<f32> = None;
    let mut last_end: Option<f32> = None;
    let mut max_bottom = 0.0f32;
    let mut found = 0;
    for (event, x) in &line[idx + 1..] {
        // 参数内的小节线（如 `1[2[34]] | 5671`）渲染为小节线，但不计入楔形覆盖跨度
        if is_real_event(event) && !matches!(event, ScoreEvent::Barline(_)) {
            if first_start.is_none() {
                first_start = Some(*x);
            }
            last_end = Some(*x + event_visual_width(event));
            // 记录覆盖音符的最下层（低八度点/减时线深度），楔形置于其下避免重叠
            max_bottom = max_bottom.max(event_extents(event).1);
            found += 1;
            if found >= n {
                break;
            }
        }
    }
    let (Some(start), Some(end)) = (first_start, last_end) else {
        return;
    };
    // 楔形位于覆盖音符的最下一层（低八度点/减时线）之下；
    // 无减时线/低八度点时也保留最小下移深度，避免紧贴音符基线
    let depth = max_bottom.max(HAIRPIN_MIN_DEPTH);
    let y = y_base - depth - HAIRPIN_GAP;
    let half = HAIRPIN_HALF;
    content.save_state();
    content.set_line_width(STROKE_WIDTH);
    if name == "cresc" {
        // 渐强：左端两条线汇于一点，向右张开（<）
        content.move_to(start, y);
        content.line_to(end, y + half);
        content.move_to(start, y);
        content.line_to(end, y - half);
    } else {
        // 渐弱：左端张开，右端两条线汇于一点（>）
        content.move_to(start, y + half);
        content.line_to(end, y);
        content.move_to(start, y - half);
        content.line_to(end, y);
    }
    content.stroke();
    content.restore_state();
}

// ============================================
// 渲染小节线
// ============================================

fn render_barline(content: &mut Content, bt: &BarlineType, x: f32, y_base: f32) {
    let top = y_base + BARLINE_TOP;
    let bottom = y_base - BARLINE_BOTTOM;
    let mid = (top + bottom) / 2.0;

    match bt {
        BarlineType::Single => {
            content.move_to(x, bottom);
            content.line_to(x, top);
            content.stroke();
        }
        BarlineType::Double => {
            content.move_to(x - 1.5, bottom);
            content.line_to(x - 1.5, top);
            content.stroke();
            content.move_to(x + 1.5, bottom);
            content.line_to(x + 1.5, top);
            content.stroke();
        }
        BarlineType::Final => {
            content.move_to(x - 2.0, bottom);
            content.line_to(x - 2.0, top);
            content.stroke();
            content.save_state();
            content.set_line_width(STROKE_WIDTH_BOLD);
            content.move_to(x + 1.5, bottom);
            content.line_to(x + 1.5, top);
            content.stroke();
            content.restore_state();
        }
        BarlineType::RepeatStart => {
            content.save_state();
            content.set_line_width(STROKE_WIDTH_BOLD);
            content.move_to(x - 2.0, bottom);
            content.line_to(x - 2.0, top);
            content.stroke();
            content.restore_state();
            content.move_to(x + 1.5, bottom);
            content.line_to(x + 1.5, top);
            content.stroke();
            draw_dot(content, x + 5.0, mid + 3.0, DOT_SIZE);
            draw_dot(content, x + 5.0, mid - 3.0, DOT_SIZE);
        }
        BarlineType::RepeatEnd => {
            draw_dot(content, x - 5.0, mid + 3.0, DOT_SIZE);
            draw_dot(content, x - 5.0, mid - 3.0, DOT_SIZE);
            content.move_to(x - 1.5, bottom);
            content.line_to(x - 1.5, top);
            content.stroke();
            content.save_state();
            content.set_line_width(STROKE_WIDTH_BOLD);
            content.move_to(x + 2.0, bottom);
            content.line_to(x + 2.0, top);
            content.stroke();
            content.restore_state();
        }
    }
}

// ============================================
// 渲染延长记号
// ============================================

fn render_extend(
    content: &mut Content,
    x: f32,
    y_base: f32,
    fonts: &mut FontFamily,
    fallback: Name,
) {
    // 延长记号 "-" 用粗体西文字体渲染，风格统一
    let mut rendered = false;
    if let Some((f, em)) = &mut fonts.latin_bold {
        if let Some((gid, _)) = f.glyph_entry('-') {
            show_glyph(content, em.name, gid, x, y_base, NOTE_FONT_SIZE);
            rendered = true;
        }
    }
    if !rendered {
        show_ascii(content, fallback, '-', x, y_base, NOTE_FONT_SIZE);
    }
}

// ============================================
// 绘制辅助：实心圆点（用贝塞尔曲线画圆）
// ============================================

fn draw_dot(content: &mut Content, x: f32, y: f32, size: f32) {
    let r = size / 2.0;
    let k = 0.5522847;
    let kr = k * r;
    content.move_to(x + r, y);
    content
        .cubic_to(x + r, y + kr, x + kr, y + r, x, y + r);
    content
        .cubic_to(x - kr, y + r, x - r, y + kr, x - r, y);
    content
        .cubic_to(x - r, y - kr, x - kr, y - r, x, y - r);
    content
        .cubic_to(x + kr, y - r, x + r, y - kr, x + r, y);
    content.close_path();
    content.fill_nonzero();
}
