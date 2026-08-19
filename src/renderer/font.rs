//! 字体加载与 PDF 嵌入模块。
//!
//! 包含以下字体：
//! - **Leland**: SMuFL 标准音乐符号字体（力度记号、拍号数字等）
//! - **Source Serif Pro Bold**: 音符数字（1-7、休止符 0）使用粗体衬线西文
//! - **Source Serif Pro Regular**: 西文说明文字（tempo 意大利术语等）
//! - **Source Han Serif SC Regular**: 简体中文 / 非西文（中速 / 快速等）
//!
//! 所有字体为 OTF（CFF 轮廓）或 TTC（取第 N 个 face），统一使用 Identity-H / CIDFontType0 / FontFile3 方式嵌入 PDF。

use std::collections::HashMap;

use owned_ttf_parser::{AsFaceRef, GlyphId, OwnedFace, Tag};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Name, Pdf, Rect, Ref, Str};

// ============================================
// SMuFL 码位常量（Private Use Area: U+E000–U+F8FF）
// ============================================

/// 力度记号（SMuFL Dynamics range: U+E520–U+E54F）
/// 注意：U+E520–U+E526 是单个字母（p/m/f/r/s/z/n），
/// 组合力度记号（pp/mp/mf/ff 等）在 U+E527–U+E53D。
pub mod dynamics {
    pub const P: char = '\u{E520}'; // dynamicPiano
    pub const M: char = '\u{E521}'; // dynamicMezzo
    pub const F: char = '\u{E522}'; // dynamicForte
    pub const R: char = '\u{E523}'; // dynamicRinforzando
    pub const S: char = '\u{E524}'; // dynamicSforzando
    pub const Z: char = '\u{E525}'; // dynamicZ
    pub const N: char = '\u{E526}'; // dynamicNiente
    pub const PP: char = '\u{E52B}';
    pub const MP: char = '\u{E52C}';
    pub const MF: char = '\u{E52D}';
    pub const PF: char = '\u{E52E}';
    pub const FF: char = '\u{E52F}';
    pub const FFF: char = '\u{E530}';
    pub const FFFF: char = '\u{E531}';
    pub const FP: char = '\u{E534}'; // Forte-piano
    pub const FZ: char = '\u{E535}'; // Forzando
    pub const SF: char = '\u{E536}';
    pub const SFP: char = '\u{E537}';
    pub const SFPP: char = '\u{E538}';
    pub const SFZ: char = '\u{E539}';
    pub const SFZP: char = '\u{E53A}';
    pub const SFFZ: char = '\u{E53B}';
    pub const RF: char = '\u{E53C}';
    pub const RFZ: char = '\u{E53D}';
}

/// 拍号数字 0–9
pub mod time_sig {
    pub const fn digit(n: u8) -> char {
        // U+E080 = 拍号 0, U+E081 = 拍号 1, ...
        char::from_u32(0xE080 + n as u32).unwrap()
    }
}

pub const REPEAT_DOT: char = '\u{E044}';

/// SMuFL Metronome marks 范围 (U+ECA0–U+ECBF)
/// 专为文本内联设计的节拍器记号（tempo 标记如 ♩=120 中的时值音符）
pub mod metro {
    pub const WHOLE: char = '\u{ECA2}'; // metNoteWhole
    pub const HALF: char = '\u{ECA3}'; // metNoteHalfUp
    pub const QUARTER: char = '\u{ECA5}'; // metNoteQuarterUp
    pub const EIGHTH: char = '\u{ECA7}'; // metNote8thUp
    pub const SIXTEENTH: char = '\u{ECA9}'; // metNote16thUp
    pub const THIRTY_SECOND: char = '\u{ECAB}'; // metNote32ndUp
    pub const DOT: char = '\u{ECB7}'; // metAugmentationDot
}

/// SMuFL 升降还原记号 (U+E260–U+E262)
pub mod accidental {
    pub const SHARP: char = '\u{E262}'; // accidentalgSharp
    pub const FLAT: char = '\u{E260}'; // accidentalgFlat
    pub const NATURAL: char = '\u{E261}'; // accidentalgNatural
}

/// 将图标名映射为 SMuFL 码位序列（用于 #icon<name> 渲染）
/// 附点音符返回两个字符：[音符，附点]
pub fn icon_to_smufl(name: &str) -> Option<&'static [char]> {
    match name {
        "note_1" | "whole" => Some(&[metro::WHOLE]),
        "note_2" | "half" => Some(&[metro::HALF]),
        "note_4" | "quarter" => Some(&[metro::QUARTER]),
        "note_8" | "eighth" => Some(&[metro::EIGHTH]),
        "note_16" | "sixteenth" => Some(&[metro::SIXTEENTH]),
        "note_32" | "thirtysecond" => Some(&[metro::THIRTY_SECOND]),
        "note_4d" | "dotted_quarter" => Some(&[metro::QUARTER, metro::DOT]),
        "note_8d" | "dotted_eighth" => Some(&[metro::EIGHTH, metro::DOT]),
        "note_16d" | "dotted_sixteenth" => Some(&[metro::SIXTEENTH, metro::DOT]),
        "note_2d" | "dotted_half" => Some(&[metro::HALF, metro::DOT]),
        "sharp" => Some(&[accidental::SHARP]),
        "flat" => Some(&[accidental::FLAT]),
        "natural" | "nat" => Some(&[accidental::NATURAL]),
        _ => None,
    }
}

/// 将力度字符串映射到 SMuFL 码位
pub fn dynamics_char(s: &str) -> Option<char> {
    match s.to_lowercase().as_str() {
        "pp" => Some(dynamics::PP),
        "p" => Some(dynamics::P),
        "mp" => Some(dynamics::MP),
        "mf" => Some(dynamics::MF),
        "pf" => Some(dynamics::PF),
        "f" => Some(dynamics::F),
        "ff" => Some(dynamics::FF),
        "fff" => Some(dynamics::FFF),
        "ffff" => Some(dynamics::FFFF),
        "fp" => Some(dynamics::FP),
        "fz" => Some(dynamics::FZ),
        "sf" => Some(dynamics::SF),
        "sfp" => Some(dynamics::SFP),
        "sfpp" => Some(dynamics::SFPP),
        "sfz" => Some(dynamics::SFZ),
        "sfzp" => Some(dynamics::SFZP),
        "sffz" => Some(dynamics::SFFZ),
        "rf" => Some(dynamics::RF),
        "rfz" => Some(dynamics::RFZ),
        _ => None,
    }
}

// ============================================
// 通用 CFF 字体封装（支持 OTF / TTC）
// ============================================

/// 已加载字体 + 嵌入所需信息。
pub struct CffFont {
    face: OwnedFace,
    cff_data: Vec<u8>,
    pub ascender: f32,
    pub descender: f32,
    pub bbox: Rect,
    pub italic_angle: f32,
    /// 码位 → (GID, advance 1000-em) 缓存
    pub glyph_cache: HashMap<char, Option<(u16, f32)>>,
    /// 是否为符号字体（Leland）：/Flags 用 SYMBOLIC；普通文字用 NONSYMBOLIC
    is_symbolic: bool,
}

/// 嵌入结果：PDF 资源名 + 字体 Ref
pub struct EmbeddedFont {
    pub name: Name<'static>,
    pub id: Ref,
}

impl CffFont {
    /// 从文件路径加载 OTF / TTC 字体。
    pub fn load(path: &str, face_index: u32) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("读取字体失败 {}: {}", path, e))?;
        Self::from_bytes(&data, face_index)
    }

    /// 从字节加载 OTF / TTC 字体，TTC 时按 face_index 选择。
    pub fn from_bytes(data: &[u8], face_index: u32) -> Result<Self, String> {
        let face = OwnedFace::from_vec(data.to_vec(), face_index)
            .map_err(|e| format!("解析字体失败 (face={face_index}): {:?}", e))?;
        let face_ref = face.as_face_ref();

        let cff_tag = Tag::from_bytes(b"CFF ");
        let cff_data = face_ref
            .raw_face()
            .table(cff_tag)
            .ok_or("字体不含 CFF 表，请使用 OTF 格式")?
            .to_vec();

        let units_per_em = face_ref.units_per_em();
        let scale = 1000.0 / units_per_em as f32;
        let ascender = face_ref.ascender() as f32 * scale;
        let descender = face_ref.descender() as f32 * scale;
        let italic_angle = face_ref.italic_angle();

        let b = face_ref.global_bounding_box();
        let bbox = Rect::new(
            b.x_min as f32 * scale,
            b.y_min as f32 * scale,
            b.x_max as f32 * scale,
            b.y_max as f32 * scale,
        );

        Ok(Self {
            face,
            cff_data,
            ascender,
            descender,
            bbox,
            italic_angle,
            glyph_cache: HashMap::new(),
            is_symbolic: false,
        })
    }

    /// 设置为符号字体（/Flags = SYMBOLIC），否则 NONSYMBOLIC。
    pub fn set_symbolic(&mut self, symbolic: bool) {
        self.is_symbolic = symbolic;
    }

    /// 预填充一组码位到 glyph_cache（性能 + 宽度表 / ToUnicode 使用）。
    pub fn prewarm(&mut self, chars: impl IntoIterator<Item = char>) {
        let face_ref = self.face.as_face_ref();
        let units_per_em = face_ref.units_per_em();
        let scale = 1000.0 / units_per_em as f32;
        for ch in chars {
            let entry = face_ref.glyph_index(ch).map(|gid| {
                let advance = face_ref.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale;
                (gid.0, advance)
            });
            self.glyph_cache.insert(ch, entry);
        }
    }

    /// 按需查询单个码位的 (GID, advance 1000-em)，缓存之。
    pub fn glyph_entry(&mut self, ch: char) -> Option<(u16, f32)> {
        if let Some(entry) = self.glyph_cache.get(&ch) {
            return *entry;
        }
        let face_ref = self.face.as_face_ref();
        let scale = 1000.0 / face_ref.units_per_em() as f32;
        let entry = face_ref.glyph_index(ch).map(|gid| {
            let advance = face_ref.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale;
            (gid.0, advance)
        });
        self.glyph_cache.insert(ch, entry);
        entry
    }

    /// 查询单个码位的字形 bbox（1000-em 单位），返回 (x_min, y_min, x_max, y_max)。
    /// SMuFL 符号字形的 advance 通常远小于实际轮廓宽度，需要用 bbox 推断视觉宽度。
    pub fn glyph_bbox(&self, ch: char) -> Option<(f32, f32, f32, f32)> {
        let face_ref = self.face.as_face_ref();
        let scale = 1000.0 / face_ref.units_per_em() as f32;
        let gid = face_ref.glyph_index(ch)?;
        let b = face_ref.glyph_bounding_box(GlyphId(gid.0))?;
        Some((
            b.x_min as f32 * scale,
            b.y_min as f32 * scale,
            b.x_max as f32 * scale,
            b.y_max as f32 * scale,
        ))
    }

    /// 将字体嵌入 PDF（Identity-H + CIDFontType0 + FontFile3）。
    pub fn embed(
        &self,
        pdf: &mut Pdf,
        next_ref: &mut i32,
        font_name: Name<'static>,
    ) -> EmbeddedFont {
        let font_id = Ref::new(*next_ref);
        *next_ref += 1;
        let cid_font_id = Ref::new(*next_ref);
        *next_ref += 1;
        let descriptor_id = Ref::new(*next_ref);
        *next_ref += 1;
        let cff_stream_id = Ref::new(*next_ref);
        *next_ref += 1;
        let to_unicode_id = Ref::new(*next_ref);
        *next_ref += 1;

        // 1. CFF 流
        pdf.stream(cff_stream_id, &self.cff_data)
            .pair(Name(b"Subtype"), Name(b"CIDFontType0C"));

        // 2. FontDescriptor
        {
            let mut desc = pdf.font_descriptor(descriptor_id);
            desc.name(font_name);
            let flags = if self.is_symbolic {
                FontFlags::SYMBOLIC
            } else {
                FontFlags::NON_SYMBOLIC
            };
            desc.flags(flags);
            desc.bbox(self.bbox);
            desc.italic_angle(self.italic_angle);
            desc.ascent(self.ascender);
            desc.descent(self.descender);
            desc.cap_height(self.ascender * 0.7);
            desc.stem_v(80.0);
            desc.font_file3(cff_stream_id);
        }

        // 3. CIDFont
        {
            let mut cid = pdf.cid_font(cid_font_id);
            cid.subtype(CidFontType::Type0);
            cid.base_font(font_name);
            cid.system_info(SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"Identity"),
                supplement: 0,
            });
            cid.font_descriptor(descriptor_id);
            cid.cid_to_gid_map_predefined(Name(b"Identity"));
            cid.default_width(500.0);

            {
                let mut widths = cid.widths();
                for entry in self.glyph_cache.values().flatten() {
                    let (gid, adv) = *entry;
                    widths.same(gid, gid, adv);
                }
            }
        }

        // 4. Type0 父字体 /Encoding Identity-H
        {
            let mut t0 = pdf.type0_font(font_id);
            t0.base_font(font_name);
            t0.encoding_predefined(Name(b"Identity-H"));
            t0.descendant_font(cid_font_id);
            t0.to_unicode(to_unicode_id);
        }

        // 5. ToUnicode CMap（便于文本提取/复制）
        let mut cmap = UnicodeCmap::new(
            Name(b"Unicodes"),
            SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"UCS"),
                supplement: 0,
            },
        );
        for (ch, entry) in &self.glyph_cache {
            if let Some((gid, _)) = entry {
                cmap.pair(*gid, *ch);
            }
        }
        let cmap_buf = cmap.finish();
        pdf.stream(to_unicode_id, &cmap_buf);

        EmbeddedFont {
            name: font_name,
            id: font_id,
        }
    }
}

// ============================================
// 高层：一组已加载 / 已嵌入的字体
// ============================================

pub struct FontFamily {
    pub leland: Option<(CffFont, EmbeddedFont)>,
    pub latin_bold: Option<(CffFont, EmbeddedFont)>, // Source Serif Pro Bold（音符数字）
    pub latin: Option<(CffFont, EmbeddedFont)>,      // Source Serif Pro Regular（西文）
    pub cjk: Option<(CffFont, EmbeddedFont)>,        // Source Han Serif SC（中文/非西文）
}

impl FontFamily {
    /// 加载系统字体并嵌入 PDF。失败时打印警告，返回 None。
    /// `extra_chars` 为乐谱中实际出现的文本字符，在嵌入前一并预热，确保宽度表完整。
    ///
    /// `font_dirs`：可选的字体查找目录列表，按顺序在目录中匹配候选文件名后用
    /// `CffFont::load` 加载；目录中都找不到时回退到内置的系统字体路径
    /// （macOS 默认路径 / `/System/Library/Fonts` 等）。
    pub fn load_and_embed(
        pdf: &mut Pdf,
        next_ref: &mut i32,
        extra_chars: &str,
        font_dirs: &[Option<std::path::PathBuf>],
    ) -> Self {
        let mut me = Self {
            leland: None,
            latin_bold: None,
            latin: None,
            cjk: None,
        };

        // --- Leland ---
        if let Some(mut f) = find_and_load(font_dirs, "Leland.otf", 0) {
            f.set_symbolic(true);
            let smufl_chars = vec![
                dynamics::PP,
                dynamics::P,
                dynamics::M,
                dynamics::F,
                dynamics::R,
                dynamics::S,
                dynamics::Z,
                dynamics::N,
                dynamics::MP,
                dynamics::MF,
                dynamics::PF,
                dynamics::FF,
                dynamics::FFF,
                dynamics::FFFF,
                dynamics::FP,
                dynamics::FZ,
                dynamics::SF,
                dynamics::SFP,
                dynamics::SFPP,
                dynamics::SFZ,
                dynamics::SFZP,
                dynamics::SFFZ,
                dynamics::RF,
                dynamics::RFZ,
                REPEAT_DOT,
                metro::WHOLE,
                metro::HALF,
                metro::QUARTER,
                metro::EIGHTH,
                metro::SIXTEENTH,
                metro::THIRTY_SECOND,
                metro::DOT,
                accidental::SHARP,
                accidental::FLAT,
                accidental::NATURAL,
            ]
            .into_iter()
            .chain((0..=9u8).map(time_sig::digit));
            f.prewarm(smufl_chars);
            let em = f.embed(pdf, next_ref, Name(b"F2"));
            me.leland = Some((f, em));
        }

        // --- Source Serif Pro Bold（音符数字粗体） ---
        if let Some(mut f) = find_and_load(font_dirs, "SourceSerifPro-Bold.otf", 0) {
            let common: Vec<char> = "0123456789+-=()<>[]{}/,:;.".chars().collect();
            let alpha: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
                .chars()
                .collect();
            f.prewarm(common.into_iter().chain(alpha).chain(extra_chars.chars()));
            let em = f.embed(pdf, next_ref, Name(b"F3"));
            me.latin_bold = Some((f, em));
        }

        // --- Source Serif Pro Regular（西文文本） ---
        if let Some(mut f) = find_and_load(font_dirs, "SourceSerifPro-Regular.otf", 0) {
            let common: Vec<char> =
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 =.,;:()[]{}<>/+-*&%$#@!?".chars().collect();
            let terms = "ModeratoAndanteAllegroAdagioLargoVivacePrestoLentoAllegrettoGraveAssaiConTenutoMarcatoStaccatoEspressivoSostenutoLegatoSforzandoRinforzandoCrescendoDiminuendo".chars();
            f.prewarm(common.into_iter().chain(terms).chain(extra_chars.chars()));
            let em = f.embed(pdf, next_ref, Name(b"F4"));
            me.latin = Some((f, em));
        }

        // --- Source Han Serif SC（简体中文/非西文），TTC 第 3 个字体 = SC ---
        if let Some(mut f) = find_and_load(font_dirs, "SourceHanSerif-Regular.ttc", 2) {
            let common = "调中快速慢板行广庄庄急渐强弱连断重保持延长记号分拍反复起始终止段落速度排号调式术语".chars();
            let terms = "ModeratoAdagioAllegroLargoVivace 快板慢板行板广板庄板急板渐快渐慢中速快速慢速稍快稍慢非常很有表情如歌的".chars();
            let keys = "CDEFGAB".chars();
            f.prewarm(common.chain(terms).chain(keys).chain(extra_chars.chars()));
            let em = f.embed(pdf, next_ref, Name(b"F5"));
            me.cjk = Some((f, em));
        }

        me
    }
}

/// 在用户指定的字体目录中查找 `file_name`，找不到时回退到内置的系统字体路径。
/// 返回加载成功的 `CffFont`（未找到/加载失败返回 None）。
fn find_and_load(
    font_dirs: &[Option<std::path::PathBuf>],
    file_name: &str,
    face_index: u32,
) -> Option<CffFont> {
    // 1) 用户指定目录优先
    for dir in font_dirs.iter().flatten() {
        let path = dir.join(file_name);
        if let Ok(f) = CffFont::load(path.to_str()?, face_index) {
            return Some(f);
        }
    }
    // 2) 回退：常见系统字体目录（macOS 默认 + 本机 Homebrew）
    const FALLBACK_DIRS: &[&str] = &[
        "/Users/pantaoran/Library/Fonts",
        "/opt/homebrew/share/fonts",
        "/usr/local/share/fonts",
        "/Library/Fonts",
        "/System/Library/Fonts",
    ];
    for dir in FALLBACK_DIRS {
        let path = format!("{}/{}", dir, file_name);
        if let Ok(f) = CffFont::load(&path, face_index) {
            return Some(f);
        }
    }
    None
}
// ============================================

/// 字符类别：影响字体选择
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    /// ASCII 范围内的字符（字母、数字、标点）
    Ascii,
    /// 非 ASCII（主要是 CJK，也包括希腊、西里尔等）
    NonAscii,
}

pub fn char_class(c: char) -> CharClass {
    if (c as u32) < 0x80 {
        CharClass::Ascii
    } else {
        CharClass::NonAscii
    }
}
