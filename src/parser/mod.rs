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
}

impl Note {
    pub fn is_rest(&self) -> bool {
        self.pitch == 0
    }
}

#[derive(Debug, Clone)]
pub enum ScoreEvent {
    Note(Note),
    Chord(Vec<Note>),
    Control(String, String),
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_rest() {
            let dot = if self.dotted { "." } else { "" };
            return write!(f, "0{}", dot);
        }
        let octave_str = if self.octave > 0 {
            format!("^{}", "+".repeat(self.octave as usize))
        } else if self.octave < 0 {
            format!("{}", "-".repeat((-self.octave) as usize))
        } else {
            String::new()
        };
        let dot = if self.dotted { "." } else { "" };
        write!(f, "{}{}{}", self.pitch, octave_str, dot)
    }
}

impl std::fmt::Display for ScoreEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreEvent::Note(note) => {
                if note.is_rest() {
                    write!(f, "🔇 {} (1/{} 拍)", note, note.duration)
                } else {
                    write!(f, "🎵 {} (1/{} 拍)", note, note.duration)
                }
            }
            ScoreEvent::Chord(notes) => {
                let notes_str: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
                let dur = notes.first().map(|n| n.duration).unwrap_or(4);
                write!(f, "🎹 [{}] (1/{} 拍)", notes_str.join(", "), dur)
            }
            ScoreEvent::Control(name, value) => write!(f, "🎛️ #{}<{}>", name, value),
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
                events.push(ScoreEvent::Control(name, value));
            }
        }
        Rule::bracket => {
            let sub_duration = duration * 2;
            for child in pair.clone().into_inner() {
                extract_events(&child, events, sub_duration);
            }
        }
        Rule::tie => {
            let sub_duration = duration * 3;
            for child in pair.clone().into_inner() {
                extract_events(&child, events, sub_duration);
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
    })
}

fn parse_octave_modifier(pair: &pest::iterators::Pair<Rule>) -> i8 {
    let s = pair.as_str();
    let mut octave = 0i8;
    let mut chars = s.chars().skip(1);

    let sign = chars.next();
    let num_str: String = chars.collect();
    let num: i8 = num_str.parse().unwrap_or(1);

    match sign {
        Some('+') => octave = num,
        Some('-') => octave = -num,
        Some('=') => octave = num,
        _ => {}
    }

    octave
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

#[allow(dead_code)]
pub fn print_parsed_events(score: &str) {
    match parse_and_extract(score) {
        Ok(events) => {
            println!("✅ 解析成功！共 {} 个事件", events.len());
            println!("=");
            for (i, event) in events.iter().enumerate() {
                println!("  [{:2}] {}", i + 1, event);
            }
        }
        Err(e) => {
            println!("❌ {}", e);
        }
    }
}
