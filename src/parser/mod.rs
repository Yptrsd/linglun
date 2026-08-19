use pest::Parser;
use pest_derive::Parser;

use crate::diagnostics::Diagnostics;

#[derive(Parser)]
#[grammar = "./src/parser/score.pest"]
pub struct MusicParser;

#[derive(Debug, Clone)]
pub struct Note {
    pub pitch: u8,
    pub octave: i8,
    pub duration: u32,
    pub dotted: bool,
    /// 升降还原记号（由 #sharp/#flat/#nat 合并到音符，渲染时紧贴音符左侧）
    pub accidental: Option<Accidental>,
}

impl Note {
    pub fn is_rest(&self) -> bool {
        self.pitch == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BarlineType {
    Single,      // |
    Double,      // ||
    Final,       // |||
    RepeatStart, // ||:
    RepeatEnd,   // :||
}

/// 升降还原记号类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Accidental {
    Sharp,
    Flat,
    Natural,
}

/// 减时线组元素：音符或嵌套减时线组
/// tight = true 表示前方无空格，应紧贴前一个元素
#[derive(Debug, Clone)]
pub enum BeamElement {
    Note(Note, bool),
    Nested(Vec<BeamElement>, bool),
}

impl BeamElement {
    pub fn is_tight(&self) -> bool {
        match self {
            BeamElement::Note(_, t) | BeamElement::Nested(_, t) => *t,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ScoreEvent {
    Note(Note),
    Chord(Vec<Note>),
    Beam(Vec<BeamElement>),  // 减时线组：支持嵌套多层减时线
    Slur(Vec<Note>),         // 连线组：音符上方画弧线，不改变时值
    Grace(Vec<BeamElement>), // 装饰音（倚音）：主音符前的小号音符组，保留减时线组结构
    Control(String, String),
    Barline(BarlineType),
    Extend, // 延长一拍
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_rest() {
            let dot = if self.dotted { "." } else { "" };
            return write!(f, "0{}", dot);
        }
        let acc = match self.accidental {
            Some(Accidental::Sharp) => "#",
            Some(Accidental::Flat) => "b",
            Some(Accidental::Natural) => "n",
            None => "",
        };
        let octave_str = if self.octave > 0 {
            format!("^{}", "+".repeat(self.octave as usize))
        } else if self.octave < 0 {
            "-".repeat((-self.octave) as usize).to_string()
        } else {
            String::new()
        };
        let dot = if self.dotted { "." } else { "" };
        write!(f, "{}{}{}{}", acc, self.pitch, octave_str, dot)
    }
}

impl std::fmt::Display for ScoreEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreEvent::Note(note) => {
                if note.is_rest() {
                    write!(f, "[REST] {} (1/{})", note, note.duration)
                } else {
                    write!(f, "[NOTE] {} (1/{})", note, note.duration)
                }
            }
            ScoreEvent::Chord(notes) => {
                let notes_str: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
                let dur = notes.first().map(|n| n.duration).unwrap_or(4);
                write!(f, "[CHORD] [{}] (1/{})", notes_str.join(", "), dur)
            }
            ScoreEvent::Beam(elements) => {
                let notes = flatten_beam(elements);
                let notes_str: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
                let dur = notes.first().map(|n| n.duration).unwrap_or(4);
                write!(f, "[BEAM] [{}] (1/{})", notes_str.join(", "), dur)
            }
            ScoreEvent::Slur(notes) => {
                let notes_str: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
                let dur = notes.first().map(|n| n.duration).unwrap_or(4);
                write!(f, "[SLUR] [{}] (1/{})", notes_str.join(", "), dur)
            }
            ScoreEvent::Grace(elements) => {
                let notes = flatten_beam(elements);
                let notes_str: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
                let dur = notes.first().map(|n| n.duration).unwrap_or(4);
                write!(f, "[GRACE] [{}] (1/{})", notes_str.join(", "), dur)
            }
            ScoreEvent::Control(name, value) => write!(f, "[CONTROL] #{}<{}>", name, value),
            ScoreEvent::Barline(bt) => {
                let s = match bt {
                    BarlineType::Single => "|",
                    BarlineType::Double => "||",
                    BarlineType::Final => "|||",
                    BarlineType::RepeatStart => "||:",
                    BarlineType::RepeatEnd => ":||",
                };
                write!(f, "[BARLINE] {}", s)
            }
            ScoreEvent::Extend => write!(f, "[EXTEND] -"),
        }
    }
}

/// 解析乐谱并提取事件。
///
/// 错误不再"遇错即止"：整文件解析失败时，会**逐行二次解析**把每一行的
/// 问题都记录到 `diag`（而不是只报第一个错误），能继续提取的部分照常
/// 返回事件。调用方通过 `diag.has_error()` 判断是否整体失败。
pub fn parse_and_extract(score: &str, diag: &mut Diagnostics) -> Vec<ScoreEvent> {
    match MusicParser::parse(Rule::file, score) {
        Ok(pairs) => {
            let mut events = Vec::new();
            for pair in pairs {
                for inner in pair.into_inner() {
                    extract_events(&inner, &mut events, 4);
                }
            }
            events
        }
        Err(_) => {
            // 整文件解析失败：逐行二次解析，坏行记入收集器（带外层行号），
            // 好行的事件照常合并，做到"能继续的部分继续"。
            let mut events = Vec::new();
            let mut saw_error = false;
            for (i, line) in score.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match MusicParser::parse(Rule::file, line) {
                    Ok(pairs) => {
                        for pair in pairs {
                            for inner in pair.into_inner() {
                                extract_events(&inner, &mut events, 4);
                            }
                        }
                    }
                    Err(le) => {
                        saw_error = true;
                        // pest 2.8 的 Error 用公开字段 line_col 暴露行列（1-based，相对该行）
                        let col = match le.line_col {
                            pest::error::LineColLocation::Pos((_, c)) => c,
                            pest::error::LineColLocation::Span((_, sc), _) => sc,
                        };
                        diag.error_at(i + 1, col, format!("该行解析失败：{}", le));
                    }
                }
            }
            // 逐行诊断一个都没记到（理论上不会）时，兜底记录主错误
            if !saw_error {
                diag.error("乐谱解析失败");
            }
            events
        }
    }
}

fn extract_events(pair: &pest::iterators::Pair<Rule>, events: &mut Vec<ScoreEvent>, duration: u32) {
    match pair.as_rule() {
        Rule::note => {
            if let Some(note) = extract_note(pair, duration) {
                events.push(ScoreEvent::Note(note));
            }
        }
        Rule::chord => {
            let chord_notes = extract_chord(pair, duration);
            if !chord_notes.is_empty() {
                events.push(ScoreEvent::Chord(chord_notes));
            }
        }
        Rule::control => {
            if let Some((name, value)) = extract_control(pair) {
                match name.as_str() {
                    "sharp" | "flat" | "nat" | "natural" => {
                        let acc = match name.as_str() {
                            "sharp" => Accidental::Sharp,
                            "flat" => Accidental::Flat,
                            _ => Accidental::Natural,
                        };
                        // 解析参数内事件，把升降号合并到第一个音符
                        // （合并后升降号在渲染时紧贴音符左侧，不再作为独立事件）
                        let mut param_events = Vec::new();
                        extract_param_events(pair, &mut param_events, duration);
                        let mut attached = false;
                        for event in param_events {
                            match event {
                                ScoreEvent::Note(mut n) => {
                                    if !attached {
                                        n.accidental = Some(acc);
                                        attached = true;
                                    }
                                    events.push(ScoreEvent::Note(n));
                                }
                                ScoreEvent::Chord(mut notes) => {
                                    if !attached && let Some(first) = notes.first_mut() {
                                        first.accidental = Some(acc);
                                        attached = true;
                                    }
                                    events.push(ScoreEvent::Chord(notes));
                                }
                                ScoreEvent::Beam(mut elems) => {
                                    if !attached && attach_accidental_to_beam(&mut elems, acc) {
                                        attached = true;
                                    }
                                    events.push(ScoreEvent::Beam(elems));
                                }
                                ScoreEvent::Slur(mut notes) => {
                                    if !attached && let Some(first) = notes.first_mut() {
                                        first.accidental = Some(acc);
                                        attached = true;
                                    }
                                    events.push(ScoreEvent::Slur(notes));
                                }
                                other => events.push(other),
                            }
                        }
                    }
                    "cresc" | "dim" => {
                        // 楔形渐强/渐弱：参数内音符照常演奏（展开进主事件流），
                        // 在此之前插入 Control 标记（value = 覆盖的音乐事件数量），
                        // 渲染时据此从后续事件扫描跨度并绘制楔形。
                        // 参数内的小节线（如 `1[2[34]] | 5671`）渲染为小节线，
                        // 但楔形覆盖参数内全部音符（可跨小节线），不把小节线计入跨度。
                        let mut param_events = Vec::new();
                        extract_param_events(pair, &mut param_events, duration);
                        let n = param_events
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e,
                                    ScoreEvent::Note(_)
                                        | ScoreEvent::Chord(_)
                                        | ScoreEvent::Beam(_)
                                        | ScoreEvent::Slur(_)
                                )
                            })
                            .count();
                        events.push(ScoreEvent::Control(name, n.to_string()));
                        events.extend(param_events);
                    }
                    "grace" => {
                        // 装饰音（倚音）：参数内音符（支持减时线组如 `#grace<[1[23]]>`）
                        // 作为小号装饰音组，紧跟其后的主音符。保留减时线组结构以便减时线连线。
                        let mut param_events = Vec::new();
                        extract_param_events(pair, &mut param_events, duration);
                        let mut elements: Vec<BeamElement> = param_events
                            .into_iter()
                            .filter_map(|e| match e {
                                ScoreEvent::Note(n) => Some(BeamElement::Note(n, false)),
                                ScoreEvent::Beam(inner) => Some(BeamElement::Nested(inner, false)),
                                _ => None,
                            })
                            .collect();
                        // 方括号组内的空格不改变装饰音的紧凑排列（与普通减时线组
                        // 不同）：组内除首个元素外一律紧贴，避免
                        // `#grace<[1[2^- 3]]>` 中 2^- 与 3 因源文本里的空格被拆开。
                        for e in elements.iter_mut() {
                            if let BeamElement::Nested(inner, _) = e {
                                make_beam_compact(inner);
                            }
                        }
                        if !elements.is_empty() {
                            events.push(ScoreEvent::Grace(elements));
                        }
                    }
                    _ => {
                        events.push(ScoreEvent::Control(name, value));
                    }
                }
            }
        }
        Rule::barline => {
            let bt = match pair.as_str() {
                "|" => BarlineType::Single,
                "||" => BarlineType::Double,
                "|||" => BarlineType::Final,
                "||:" => BarlineType::RepeatStart,
                ":||" => BarlineType::RepeatEnd,
                _ => return,
            };
            events.push(ScoreEvent::Barline(bt));
        }
        Rule::extend => {
            events.push(ScoreEvent::Extend);
        }
        Rule::bracket => {
            let sub_duration = duration * 2;
            // 尝试收集为 Beam（连续减时线组）
            if let Some(beam_notes) = try_extract_beam(pair, sub_duration) {
                events.push(ScoreEvent::Beam(beam_notes));
            } else {
                // 包含和弦/连音等复杂元素，回退为独立事件
                for child in pair.clone().into_inner() {
                    extract_events(&child, events, sub_duration);
                }
            }
        }
        Rule::tie => {
            // 圆括号 = 连线（slur），不改变时值
            if let Some(slur_notes) = try_extract_slur(pair, duration) {
                events.push(ScoreEvent::Slur(slur_notes));
            } else {
                // 包含和弦/方括号等复杂元素，回退为独立事件
                for child in pair.clone().into_inner() {
                    extract_events(&child, events, duration);
                }
            }
        }
        Rule::content => {
            for child in pair.clone().into_inner() {
                extract_events(&child, events, duration);
            }
        }
        _ => {
            for child in pair.clone().into_inner() {
                extract_events(&child, events, duration);
            }
        }
    }
}

/// 尝试将方括号内容收集为 Beam（支持嵌套方括号，检测空格区分紧贴/松散）。
/// 遇到和弦/连线等非音符非方括号元素时返回 None。
fn try_extract_beam(pair: &pest::iterators::Pair<Rule>, duration: u32) -> Option<Vec<BeamElement>> {
    let mut elements = Vec::new();
    let mut prev_was_space = true; // 第一个元素不紧贴

    for child in pair.clone().into_inner() {
        // child 是 bracket_inner，内部恰好一个子节点
        for inner in child.clone().into_inner() {
            match inner.as_rule() {
                Rule::whitespace => {
                    prev_was_space = true;
                }
                Rule::note => {
                    let tight = !prev_was_space;
                    if let Some(note) = extract_note(&inner, duration) {
                        elements.push(BeamElement::Note(note, tight));
                    }
                    prev_was_space = false;
                }
                Rule::bracket => {
                    let tight = !prev_was_space;
                    let sub_duration = duration * 2;
                    let nested = try_extract_beam(&inner, sub_duration)?;
                    elements.push(BeamElement::Nested(nested, tight));
                    prev_was_space = false;
                }
                Rule::control => {
                    let tight = !prev_was_space;
                    if let Some((name, _)) = extract_control(&inner) {
                        if name == "sharp" || name == "flat" || name == "nat" || name == "natural" {
                            let acc = match name.as_str() {
                                "sharp" => Accidental::Sharp,
                                "flat" => Accidental::Flat,
                                _ => Accidental::Natural,
                            };
                            // 解析参数内事件，把升降号合并到第一个音符
                            let mut param_events = Vec::new();
                            extract_param_events(&inner, &mut param_events, duration);
                            let mut attached = false;
                            for event in param_events {
                                match event {
                                    ScoreEvent::Note(mut n) => {
                                        // 第一个音符继承升降号的 tight（紧贴前一个元素）
                                        let note_tight = if !attached { tight } else { false };
                                        if !attached {
                                            n.accidental = Some(acc);
                                            attached = true;
                                        }
                                        elements.push(BeamElement::Note(n, note_tight));
                                    }
                                    ScoreEvent::Beam(mut nested) => {
                                        let nested_tight = if !attached { tight } else { false };
                                        if !attached {
                                            attach_accidental_to_beam(&mut nested, acc);
                                            attached = true;
                                        }
                                        elements.push(BeamElement::Nested(nested, nested_tight));
                                    }
                                    _ => {}
                                }
                            }
                            prev_was_space = false;
                        } else {
                            return None; // 其他控制指令不支持在 beam 内
                        }
                    }
                }
                Rule::dot => { /* 独立附点，忽略 */ }
                _ => return None, // chord / tie 等 → 无法构成 beam
            }
        }
    }

    if elements.is_empty() {
        None
    } else {
        Some(elements)
    }
}

/// 将嵌套 Beam 结构展平为音符列表（用于 Display / 装饰音高度计算）
pub fn flatten_beam(elements: &[BeamElement]) -> Vec<Note> {
    let mut notes = Vec::new();
    for e in elements {
        match e {
            BeamElement::Note(n, _) => notes.push(n.clone()),
            BeamElement::Nested(inner, _) => notes.extend(flatten_beam(inner)),
        }
    }
    notes
}

/// 递归地把升降号附加到 beam 内第一个音符，返回是否成功附加。
fn attach_accidental_to_beam(elements: &mut [BeamElement], acc: Accidental) -> bool {
    for e in elements.iter_mut() {
        match e {
            BeamElement::Note(n, _) => {
                n.accidental = Some(acc);
                return true;
            }
            BeamElement::Nested(inner, _) => {
                if attach_accidental_to_beam(inner, acc) {
                    return true;
                }
            }
        }
    }
    false
}

/// 递归地把减时线组内所有元素标记为"首个不紧贴、其余一律紧贴"，
/// 忽略方括号内的空格（用于装饰音：组内音符始终紧凑排列）。
fn make_beam_compact(elements: &mut [BeamElement]) {
    for (i, e) in elements.iter_mut().enumerate() {
        match e {
            BeamElement::Note(_, tight) => *tight = i > 0,
            BeamElement::Nested(inner, tight) => {
                *tight = i > 0;
                make_beam_compact(inner);
            }
        }
    }
}

/// 尝试将圆括号内容收集为 Slur（仅含音符时成功）。
fn try_extract_slur(pair: &pest::iterators::Pair<Rule>, duration: u32) -> Option<Vec<Note>> {
    let mut notes = Vec::new();
    for child in pair.clone().into_inner() {
        collect_group_notes(&child, &mut notes, duration)?;
    }
    if notes.is_empty() { None } else { Some(notes) }
}

/// 递归收集音符。遇到非音符元素（和弦/嵌套方括号/嵌套连线）返回 None。
/// 同时处理 bracket_inner 和 tie_inner 包装层。
fn collect_group_notes(
    pair: &pest::iterators::Pair<Rule>,
    notes: &mut Vec<Note>,
    duration: u32,
) -> Option<()> {
    match pair.as_rule() {
        Rule::note => {
            if let Some(note) = extract_note(pair, duration) {
                notes.push(note);
            }
            Some(())
        }
        Rule::whitespace | Rule::dot | Rule::bracket_inner | Rule::tie_inner => {
            // 递归展开包装层
            for child in pair.clone().into_inner() {
                collect_group_notes(&child, notes, duration)?;
            }
            Some(())
        }
        _ => None,
    }
}

fn extract_note(pair: &pest::iterators::Pair<Rule>, duration: u32) -> Option<Note> {
    let mut pitch = 0u8;
    let mut octave = 0i8;
    let mut dotted = false;

    for child in pair.clone().into_inner() {
        match child.as_rule() {
            Rule::octave_modifier => {
                octave = parse_octave_modifier(&child);
            }
            Rule::duration_modifier => {
                dotted = true;
            }
            _ => {}
        }
    }

    let note_str = pair.as_str();
    if let Some(first_char) = note_str.chars().next()
        && first_char.is_ascii_digit()
    {
        pitch = first_char.to_digit(10).unwrap_or(0) as u8;
    }

    Some(Note {
        pitch,
        octave,
        duration,
        dotted,
        accidental: None,
    })
}

fn parse_octave_modifier(pair: &pest::iterators::Pair<Rule>) -> i8 {
    let s = pair.as_str();
    // 去掉开头的 ^
    let rest: String = s.chars().skip(1).collect();
    if rest.is_empty() {
        // 只有 ^，表示升 1 个八度（简谱默认音符上方一个点=高八度）
        return 1;
    }
    let mut chars = rest.chars();
    let sign = chars.next();
    let num_str: String = chars.collect();
    let num: i8 = if num_str.is_empty() {
        1
    } else {
        num_str.parse().unwrap_or(1)
    };

    match sign {
        Some('+') => num,
        Some('-') => -num,
        Some('=') => num,
        _ => 1, // 没有符号也默认升
    }
}

fn extract_chord(pair: &pest::iterators::Pair<Rule>, duration: u32) -> Vec<Note> {
    let mut notes = Vec::new();
    collect_notes_recursively(pair, &mut notes, duration);
    notes
}

fn collect_notes_recursively(
    pair: &pest::iterators::Pair<Rule>,
    notes: &mut Vec<Note>,
    duration: u32,
) {
    if pair.as_rule() == Rule::note {
        if let Some(note) = extract_note(pair, duration) {
            notes.push(note);
        }
    } else {
        for child in pair.clone().into_inner() {
            collect_notes_recursively(&child, notes, duration);
        }
    }
}

fn extract_control(pair: &pest::iterators::Pair<Rule>) -> Option<(String, String)> {
    let mut name = String::new();
    let mut value = String::new();

    for child in pair.clone().into_inner() {
        match child.as_rule() {
            Rule::ident => {
                name = child.as_str().to_string();
            }
            Rule::param => {
                value = child.as_str().trim().to_string();
            }
            _ => {}
        }
    }

    if name.is_empty() {
        return None;
    }

    Some((name, value))
}

/// 将控制指令参数中的音符/减时线等解析为事件（用于 #sharp/#flat/#nat）
fn extract_param_events(
    control_pair: &pest::iterators::Pair<Rule>,
    events: &mut Vec<ScoreEvent>,
    duration: u32,
) {
    for child in control_pair.clone().into_inner() {
        if child.as_rule() == Rule::param {
            for inner in child.clone().into_inner() {
                extract_events(&inner, events, duration);
            }
        }
    }
}

pub fn print_parsed_events(score: &str) {
    let mut diag = Diagnostics::new();
    let events = parse_and_extract(score, &mut diag);
    if diag.has_error() {
        diag.report();
        return;
    }
    crate::ui::success(&format!("Parsed {} events", events.len()));
    println!("========");
    for (i, event) in events.iter().enumerate() {
        println!("  [{:2}] {}", i + 1, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;

    /// 解析成功的辅助函数：失败时直接 panic（诊断收集器累积错误后断言）
    fn parse(src: &str) -> Vec<ScoreEvent> {
        let mut diag = Diagnostics::new();
        let events = parse_and_extract(src, &mut diag);
        assert!(!diag.has_error(), "解析应当成功，诊断: {:?}", diag.items);
        events
    }

    /// 解析但不要求成功，返回诊断收集器与事件（用于失败用例）
    fn parse_with_diag(src: &str) -> (Diagnostics, Vec<ScoreEvent>) {
        let mut diag = Diagnostics::new();
        let events = parse_and_extract(src, &mut diag);
        (diag, events)
    }

    /// 断言事件是音符且各字段符合预期
    fn assert_note(
        event: &ScoreEvent,
        pitch: u8,
        octave: i8,
        duration: u32,
        dotted: bool,
        accidental: Option<Accidental>,
    ) {
        match event {
            ScoreEvent::Note(n) => {
                assert_eq!(
                    (n.pitch, n.octave, n.duration, n.dotted, n.accidental),
                    (pitch, octave, duration, dotted, accidental),
                    "音符字段不符: {:?}",
                    n
                );
            }
            other => panic!("预期 Note，实际 {:?}", other),
        }
    }

    // ---------- 音符 ----------

    #[test]
    fn plain_note() {
        let events = parse("1");
        assert_eq!(events.len(), 1);
        assert_note(&events[0], 1, 0, 4, false, None);
    }

    #[test]
    fn rest_note() {
        let events = parse("0");
        assert_eq!(events.len(), 1);
        assert_note(&events[0], 0, 0, 4, false, None);
        assert!(matches!(&events[0], ScoreEvent::Note(n) if n.is_rest()));
    }

    #[test]
    fn all_pitches() {
        let events = parse("1234567");
        assert_eq!(events.len(), 7);
        for (i, e) in events.iter().enumerate() {
            assert_note(e, (i + 1) as u8, 0, 4, false, None);
        }
    }

    #[test]
    fn octave_up_forms() {
        // ^ 单独 → 升 1；^+1 → +1；^+2 → +2；^= → +1；^=2 → +2
        let events = parse("1^ 2^+1 3^+2 4^= 5^=2");
        assert_eq!(events.len(), 5);
        assert_note(&events[0], 1, 1, 4, false, None);
        assert_note(&events[1], 2, 1, 4, false, None);
        assert_note(&events[2], 3, 2, 4, false, None);
        assert_note(&events[3], 4, 1, 4, false, None);
        assert_note(&events[4], 5, 2, 4, false, None);
    }

    #[test]
    fn octave_down_forms() {
        // ^- → -1；^-1 → -1；^-2 → -2
        let events = parse("1^- 2^-1 3^-2");
        assert_eq!(events.len(), 3);
        assert_note(&events[0], 1, -1, 4, false, None);
        assert_note(&events[1], 2, -1, 4, false, None);
        assert_note(&events[2], 3, -2, 4, false, None);
    }

    #[test]
    fn dotted_note() {
        let events = parse("1.");
        assert_note(&events[0], 1, 0, 4, true, None);
    }

    #[test]
    fn dotted_note_with_space() {
        // 非原子规则 note 会跨空格吸收附点（"1 ." 与 "1." 等价）
        let events = parse("1 .");
        assert_note(&events[0], 1, 0, 4, true, None);
    }

    #[test]
    fn dash_is_extend_not_low_octave() {
        // 低八度必须写 1^-；单独的 "-" 是延长记号
        let events = parse("1-");
        assert_eq!(events.len(), 2);
        assert_note(&events[0], 1, 0, 4, false, None);
        assert!(matches!(events[1], ScoreEvent::Extend));
    }

    #[test]
    fn standalone_octave_change_produces_nothing() {
        // "^+1" 脱离音符单独出现时是 octave_change，不产生事件
        assert!(parse("^+1").is_empty());
    }

    #[test]
    fn note_display() {
        let n = Note {
            pitch: 1,
            octave: 1,
            duration: 4,
            dotted: false,
            accidental: None,
        };
        assert_eq!(n.to_string(), "1^+");
        let n = Note {
            pitch: 2,
            octave: -2,
            duration: 8,
            dotted: true,
            accidental: Some(Accidental::Flat),
        };
        assert_eq!(n.to_string(), "b2--.");
    }

    // ---------- 和弦 ----------

    #[test]
    fn chord() {
        let events = parse("{1 3 5}");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScoreEvent::Chord(notes) => {
                assert_eq!(
                    notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
                    vec![1, 3, 5]
                );
                assert!(notes.iter().all(|n| n.duration == 4));
            }
            other => panic!("预期 Chord，实际 {:?}", other),
        }
    }

    #[test]
    fn nested_chord_flattens() {
        let events = parse("{1 {2 3}}");
        match &events[0] {
            ScoreEvent::Chord(notes) => {
                assert_eq!(
                    notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
                    vec![1, 2, 3]
                );
            }
            other => panic!("预期 Chord，实际 {:?}", other),
        }
    }

    // ---------- 减时线组 ----------

    #[test]
    fn beam_doubles_duration_and_tight() {
        let events = parse("[12]");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                assert_eq!(elements.len(), 2);
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, false) if n.pitch == 1 && n.duration == 8)
                );
                // 无空格 → 第二个音符紧贴
                assert!(
                    matches!(&elements[1], BeamElement::Note(n, true) if n.pitch == 2 && n.duration == 8)
                );
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn beam_space_means_not_tight() {
        // bracket 是原子规则，方括号内空格可见：[1 2] 中第二个音符不紧贴
        let events = parse("[1 2]");
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                assert!(matches!(&elements[1], BeamElement::Note(n, false) if n.pitch == 2));
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn space_makes_beam_loose() {
        // 核心差异：[12] 紧贴（true），[1 2] 松散（false）
        let tight = parse("[12]");
        let loose = parse("[1 2]");
        match (&tight[0], &loose[0]) {
            (ScoreEvent::Beam(a), ScoreEvent::Beam(b)) => {
                assert!(matches!(&a[1], BeamElement::Note(_, true)));
                assert!(matches!(&b[1], BeamElement::Note(_, false)));
            }
            other => panic!("预期两个 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn beam_nested_doubles_duration_again() {
        let events = parse("[5 [67]]");
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                assert_eq!(elements.len(), 2);
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, false) if n.pitch == 5 && n.duration == 8)
                );
                match &elements[1] {
                    BeamElement::Nested(inner, tight) => {
                        // [5 [67]] 中间有空格 → Nested 不紧贴（bracket 原子规则下空格可见）
                        assert!(!tight);
                        assert_eq!(inner.len(), 2);
                        assert!(
                            matches!(&inner[0], BeamElement::Note(n, false) if n.pitch == 6 && n.duration == 16)
                        );
                        assert!(
                            matches!(&inner[1], BeamElement::Note(n, true) if n.pitch == 7 && n.duration == 16)
                        );
                    }
                    other => panic!("预期 Nested，实际 {:?}", other),
                }
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn beam_tight_nested() {
        // [2[34]] 方括号间无空格 → Nested 紧贴
        let events = parse("[2[34]]");
        match &events[0] {
            ScoreEvent::Beam(elements) => match &elements[1] {
                BeamElement::Nested(_, tight) => assert!(*tight),
                other => panic!("预期 Nested，实际 {:?}", other),
            },
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn beam_with_accidental() {
        let events = parse("[#sharp<1> 2 3]");
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                assert_eq!(elements.len(), 3);
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, false) if n.pitch == 1 && n.accidental == Some(Accidental::Sharp))
                );
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    #[test]
    fn flatten_beam_unwraps_nested() {
        let events = parse("[5 [67]]");
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                let pitches = flatten_beam(elements)
                    .iter()
                    .map(|n| n.pitch)
                    .collect::<Vec<_>>();
                assert_eq!(pitches, vec![5, 6, 7]);
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
    }

    // ---------- 连线 ----------

    #[test]
    fn slur() {
        let events = parse("(1 2 3 4)");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScoreEvent::Slur(notes) => {
                assert_eq!(
                    notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
                    vec![1, 2, 3, 4]
                );
                assert!(notes.iter().all(|n| n.duration == 4));
            }
            other => panic!("预期 Slur，实际 {:?}", other),
        }
    }

    #[test]
    fn slur_with_dotted() {
        let events = parse("(1 2.)");
        match &events[0] {
            ScoreEvent::Slur(notes) => {
                assert_eq!(notes.len(), 2);
                assert!(notes[1].dotted);
            }
            other => panic!("预期 Slur，实际 {:?}", other),
        }
    }

    // ---------- 控制指令 ----------

    #[test]
    fn key_control() {
        assert!(
            matches!(&parse("#key<C>")[0], ScoreEvent::Control(n, v) if n == "key" && v == "C")
        );
    }

    #[test]
    fn tempo_control_keeps_icon_markup() {
        assert!(matches!(
            &parse("#tempo<#icon<note_4> = 120>")[0],
            ScoreEvent::Control(n, v) if n == "tempo" && v == "#icon<note_4> = 120"
        ));
    }

    #[test]
    fn tempo_control_chinese() {
        assert!(
            matches!(&parse("#tempo<中速>")[0], ScoreEvent::Control(n, v) if n == "tempo" && v == "中速")
        );
    }

    #[test]
    fn timesig_control() {
        assert!(
            matches!(&parse("#timesig<4/4>")[0], ScoreEvent::Control(n, v) if n == "timesig" && v == "4/4")
        );
    }

    #[test]
    fn dynamics_control() {
        assert!(
            matches!(&parse("#dynamics<ff>")[0], ScoreEvent::Control(n, v) if n == "dynamics" && v == "ff")
        );
    }

    #[test]
    fn title_control_chinese() {
        assert!(
            matches!(&parse("#title<我的歌>")[0], ScoreEvent::Control(n, v) if n == "title" && v == "我的歌")
        );
    }

    #[test]
    fn unknown_control() {
        assert!(
            matches!(&parse("#foo<bar>")[0], ScoreEvent::Control(n, v) if n == "foo" && v == "bar")
        );
    }

    #[test]
    fn control_without_param() {
        assert!(
            matches!(&parse("#foo")[0], ScoreEvent::Control(n, v) if n == "foo" && v.is_empty())
        );
    }

    // ---------- 变音记号（合并进音符） ----------

    #[test]
    fn sharp_flat_natural_merge() {
        let events = parse("#sharp<1> 2 #flat<3> 4 #nat<5>");
        assert_eq!(events.len(), 5);
        assert_note(&events[0], 1, 0, 4, false, Some(Accidental::Sharp));
        assert_note(&events[1], 2, 0, 4, false, None);
        assert_note(&events[2], 3, 0, 4, false, Some(Accidental::Flat));
        assert_note(&events[3], 4, 0, 4, false, None);
        assert_note(&events[4], 5, 0, 4, false, Some(Accidental::Natural));
    }

    #[test]
    fn sharp_attaches_to_first_beam_note() {
        let events = parse("#sharp<[12] 3>");
        assert_eq!(events.len(), 2);
        match &events[0] {
            ScoreEvent::Beam(elements) => {
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, _) if n.accidental == Some(Accidental::Sharp))
                );
                assert!(matches!(&elements[1], BeamElement::Note(n, _) if n.accidental.is_none()));
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
        assert_note(&events[1], 3, 0, 4, false, None);
    }

    // ---------- 装饰音 ----------

    #[test]
    fn grace_basic() {
        let events = parse("#grace<5 6>");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScoreEvent::Grace(elements) => {
                assert_eq!(elements.len(), 2);
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, _) if n.pitch == 5 && n.duration == 4)
                );
                assert!(
                    matches!(&elements[1], BeamElement::Note(n, _) if n.pitch == 6 && n.duration == 4)
                );
            }
            other => panic!("预期 Grace，实际 {:?}", other),
        }
    }

    #[test]
    fn grace_keeps_beam_structure() {
        // #grace<[1[2^- 3]]>：整体是一个 beam → 被包成单个 Nested，
        // 内部保留嵌套减时线结构与时值（1/8 → 内层 1/16）。
        // 装饰音组内空格被忽略：2^- 与 3 之间的空格不会拆开它们（仍紧贴）。
        let events = parse("#grace<[1[2^- 3]]>");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScoreEvent::Grace(elements) => {
                assert_eq!(elements.len(), 1);
                match &elements[0] {
                    BeamElement::Nested(inner, tight) => {
                        assert!(!tight); // grace 包裹层固定不紧贴
                        assert_eq!(inner.len(), 2);
                        assert!(
                            matches!(&inner[0], BeamElement::Note(n, false) if n.pitch == 1 && n.duration == 8)
                        );
                        match &inner[1] {
                            BeamElement::Nested(deep, deep_tight) => {
                                assert!(*deep_tight); // 1[ 之间无空格 → 紧贴
                                assert_eq!(deep.len(), 2);
                                assert!(
                                    matches!(&deep[0], BeamElement::Note(n, false) if n.pitch == 2 && n.octave == -1 && n.duration == 16)
                                );
                                // 2^- 与 3 之间有空格，但装饰音组内空格被忽略 → 仍紧贴
                                assert!(
                                    matches!(&deep[1], BeamElement::Note(n, true) if n.pitch == 3 && n.duration == 16)
                                );
                            }
                            other => panic!("预期 Nested，实际 {:?}", other),
                        }
                    }
                    other => panic!("预期 Nested，实际 {:?}", other),
                }
            }
            other => panic!("预期 Grace，实际 {:?}", other),
        }
    }

    #[test]
    fn grace_ignores_internal_spaces() {
        // 装饰音组内空格与无空格结构完全一致（[1 2 3] ≡ [123]），
        // 保证渲染时不会把组内音符拆开。
        // 注：不能用 [2^- 3] vs [2^-3] 对照——无空格时 ^-3 会被解析为八度 -3。
        let with_space = parse("#grace<[1 2 3]>");
        let without_space = parse("#grace<[123]>");
        assert_eq!(with_space.len(), 1);
        assert_eq!(without_space.len(), 1);
        match (&with_space[0], &without_space[0]) {
            (ScoreEvent::Grace(a), ScoreEvent::Grace(b)) => {
                let notes_a = flatten_beam(a);
                let notes_b = flatten_beam(b);
                assert_eq!(notes_a.len(), notes_b.len());
                for (na, nb) in notes_a.iter().zip(&notes_b) {
                    assert_eq!(
                        (na.pitch, na.octave, na.duration),
                        (nb.pitch, nb.octave, nb.duration)
                    );
                }
                // 逐元素对比 tight 标记（递归）
                fn tight_flags(elements: &[BeamElement]) -> Vec<bool> {
                    let mut v = Vec::new();
                    for e in elements {
                        match e {
                            BeamElement::Note(_, t) => v.push(*t),
                            BeamElement::Nested(inner, t) => {
                                v.push(*t);
                                v.extend(tight_flags(inner));
                            }
                        }
                    }
                    v
                }
                assert_eq!(tight_flags(a), tight_flags(b));
            }
            other => panic!("预期两个 Grace，实际 {:?}", other),
        }
    }

    // ---------- 渐强渐弱 ----------

    #[test]
    fn cresc_expands_notes_and_counts() {
        // #cresc 参数内音符展开进主事件流；value 记录音乐事件数（小节线不计）
        let events = parse("#cresc<1[2[34]] | 5671>");
        assert_eq!(events.len(), 8);
        assert!(matches!(&events[0], ScoreEvent::Control(n, v) if n == "cresc" && v == "6"));
        assert_note(&events[1], 1, 0, 4, false, None);
        match &events[2] {
            ScoreEvent::Beam(elements) => {
                assert!(
                    matches!(&elements[0], BeamElement::Note(n, false) if n.pitch == 2 && n.duration == 8)
                );
                match &elements[1] {
                    BeamElement::Nested(inner, tight) => {
                        assert!(*tight); // 2[ 之间无空格 → 紧贴
                        assert!(
                            matches!(&inner[0], BeamElement::Note(n, false) if n.pitch == 3 && n.duration == 16)
                        );
                        assert!(
                            matches!(&inner[1], BeamElement::Note(n, true) if n.pitch == 4 && n.duration == 16)
                        );
                    }
                    other => panic!("预期 Nested，实际 {:?}", other),
                }
            }
            other => panic!("预期 Beam，实际 {:?}", other),
        }
        assert!(matches!(
            &events[3],
            ScoreEvent::Barline(BarlineType::Single)
        ));
        assert_note(&events[4], 5, 0, 4, false, None);
        assert_note(&events[5], 6, 0, 4, false, None);
        assert_note(&events[6], 7, 0, 4, false, None);
        assert_note(&events[7], 1, 0, 4, false, None);
    }

    #[test]
    fn dim_expands_notes_and_counts() {
        let events = parse("#dim<1 2 3>");
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], ScoreEvent::Control(n, v) if n == "dim" && v == "3"));
        assert_note(&events[1], 1, 0, 4, false, None);
        assert_note(&events[2], 2, 0, 4, false, None);
        assert_note(&events[3], 3, 0, 4, false, None);
    }

    // ---------- 小节线 ----------

    #[test]
    fn barline_types() {
        assert!(matches!(
            &parse("|")[0],
            ScoreEvent::Barline(BarlineType::Single)
        ));
        assert!(matches!(
            &parse("||")[0],
            ScoreEvent::Barline(BarlineType::Double)
        ));
        assert!(matches!(
            &parse("|||")[0],
            ScoreEvent::Barline(BarlineType::Final)
        ));
        assert!(matches!(
            &parse("||:")[0],
            ScoreEvent::Barline(BarlineType::RepeatStart)
        ));
        assert!(matches!(
            &parse(":||")[0],
            ScoreEvent::Barline(BarlineType::RepeatEnd)
        ));
    }

    #[test]
    fn barline_longest_match_wins() {
        // ||: 不会被拆成 | 和 |:
        let events = parse("1 ||: 2");
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[1],
            ScoreEvent::Barline(BarlineType::RepeatStart)
        ));
    }

    // ---------- 延长记号 ----------

    #[test]
    fn extend() {
        let events = parse("-");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ScoreEvent::Extend));
    }

    // ---------- 综合 ----------

    #[test]
    fn mixed_line() {
        let events = parse("1 2 3 4 | 5 6 7 1^ |");
        assert_eq!(events.len(), 10);
        for (i, e) in events[0..4].iter().enumerate() {
            assert_note(e, (i + 1) as u8, 0, 4, false, None);
        }
        assert!(matches!(
            &events[4],
            ScoreEvent::Barline(BarlineType::Single)
        ));
        for (i, e) in events[5..8].iter().enumerate() {
            assert_note(e, (5 + i) as u8, 0, 4, false, None);
        }
        assert_note(&events[8], 1, 1, 4, false, None);
        assert!(matches!(
            &events[9],
            ScoreEvent::Barline(BarlineType::Single)
        ));
    }

    #[test]
    fn newlines_are_ignored() {
        let events = parse("1 2\n3 4\r\n5");
        assert_eq!(events.len(), 5);
        assert_note(&events[4], 5, 0, 4, false, None);
    }

    #[test]
    fn complex_score() {
        let score = "1 2 3 4 | [12] (3 4) {1 3 5} | #sharp<1> 2 - ||: 5 6 :|| 7 1^ |||";
        let events = parse(score);
        assert_eq!(events.len(), 19);
        assert!(matches!(
            &events[4],
            ScoreEvent::Barline(BarlineType::Single)
        ));
        assert!(matches!(&events[5], ScoreEvent::Beam(_)));
        assert!(matches!(&events[6], ScoreEvent::Slur(_)));
        assert!(matches!(&events[7], ScoreEvent::Chord(_)));
        assert!(matches!(&events[11], ScoreEvent::Extend));
        assert!(matches!(
            &events[12],
            ScoreEvent::Barline(BarlineType::RepeatStart)
        ));
        assert!(matches!(
            &events[15],
            ScoreEvent::Barline(BarlineType::RepeatEnd)
        ));
        assert!(matches!(
            &events[18],
            ScoreEvent::Barline(BarlineType::Final)
        ));
    }

    #[test]
    fn event_display() {
        let events = parse("1 2 3 4");
        assert_eq!(events[0].to_string(), "[NOTE] 1 (1/4)");
        assert_eq!(parse("|||")[0].to_string(), "[BARLINE] |||");
    }

    // ---------- 失败与边界 ----------

    #[test]
    fn empty_input_ok() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn invalid_input_errors() {
        let (diag, events) = parse_with_diag("abc");
        assert!(diag.has_error());
        assert!(events.is_empty());
    }

    #[test]
    fn multi_line_errors_all_collected() {
        // 两行都坏 → 收集器应同时记录两个错误（而不是只报第一个）
        let (diag, _) = parse_with_diag("abc\n1 2 3\ndef");
        let errs = diag
            .items
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        assert_eq!(errs, 2, "两行错误都应被收集: {:?}", diag.items);
        // 错误带行列位置
        for d in diag.items.iter().filter(|d| d.severity == Severity::Error) {
            assert!(d.line.is_some() && d.col.is_some(), "应带行列: {:?}", d);
        }
    }

    #[test]
    fn good_and_bad_lines_mixed() {
        // 好行照常出事件，坏行记入收集器（不互相阻断）
        let (diag, events) = parse_with_diag("1 2 3\nxxx\n4 5");
        assert!(diag.has_error());
        assert_eq!(events.len(), 5, "好行事件应保留: {:?}", events);
    }
}
