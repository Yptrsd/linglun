use pest::Parser;
use pest_derive::Parser;

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
    Beam(Vec<BeamElement>),     // 减时线组：支持嵌套多层减时线
    Slur(Vec<Note>),            // 连线组：音符上方画弧线，不改变时值
    Grace(Vec<BeamElement>),    // 装饰音（倚音）：主音符前的小号音符组，保留减时线组结构
    Control(String, String),
    Barline(BarlineType),
    Extend,                    // 延长一拍
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
            format!("{}", "-".repeat((-self.octave) as usize))
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

pub fn parse_and_extract(score: &str) -> Result<Vec<ScoreEvent>, String> {
    match MusicParser::parse(Rule::file, score) {
        Ok(pairs) => {
            let mut events = Vec::new();
            for pair in pairs {
                for inner in pair.into_inner() {
                    extract_events(&inner, &mut events, 4);
                }
            }
            Ok(events)
        }
        Err(e) => Err(format!("解析失败：{}", e)),
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
                                    if !attached {
                                        if let Some(first) = notes.first_mut() {
                                            first.accidental = Some(acc);
                                            attached = true;
                                        }
                                    }
                                    events.push(ScoreEvent::Chord(notes));
                                }
                                ScoreEvent::Beam(mut elems) => {
                                    if !attached {
                                        if attach_accidental_to_beam(&mut elems, acc) {
                                            attached = true;
                                        }
                                    }
                                    events.push(ScoreEvent::Beam(elems));
                                }
                                ScoreEvent::Slur(mut notes) => {
                                    if !attached {
                                        if let Some(first) = notes.first_mut() {
                                            first.accidental = Some(acc);
                                            attached = true;
                                        }
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
                        let elements: Vec<BeamElement> = param_events
                            .into_iter()
                            .filter_map(|e| match e {
                                ScoreEvent::Note(n) => Some(BeamElement::Note(n, false)),
                                ScoreEvent::Beam(inner) => Some(BeamElement::Nested(inner, false)),
                                _ => None,
                            })
                            .collect();
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

/// 尝试将圆括号内容收集为 Slur（仅含音符时成功）。
fn try_extract_slur(pair: &pest::iterators::Pair<Rule>, duration: u32) -> Option<Vec<Note>> {
    let mut notes = Vec::new();
    for child in pair.clone().into_inner() {
        collect_group_notes(&child, &mut notes, duration)?;
    }
    if notes.is_empty() {
        None
    } else {
        Some(notes)
    }
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
    if let Some(first_char) = note_str.chars().next() {
        if first_char.is_ascii_digit() {
            pitch = first_char.to_digit(10).unwrap_or(0) as u8;
        }
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
    let num: i8 = if num_str.is_empty() { 1 } else { num_str.parse().unwrap_or(1) };

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

fn collect_notes_recursively(pair: &pest::iterators::Pair<Rule>, notes: &mut Vec<Note>, duration: u32) {
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
fn extract_param_events(control_pair: &pest::iterators::Pair<Rule>, events: &mut Vec<ScoreEvent>, duration: u32) {
    for child in control_pair.clone().into_inner() {
        if child.as_rule() == Rule::param {
            for inner in child.clone().into_inner() {
                extract_events(&inner, events, duration);
            }
        }
    }
}

pub fn print_parsed_events(score: &str) {
    match parse_and_extract(score) {
        Ok(events) => {
            crate::ui::success(&format!("Parsed {} events", events.len()));
            println!("========");
            for (i, event) in events.iter().enumerate() {
                println!("  [{:2}] {}", i + 1, event);
            }
        }
        Err(e) => {
            crate::ui::error(&e);
        }
    }
}
