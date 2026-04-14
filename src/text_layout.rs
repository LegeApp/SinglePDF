use std::{fs, path::{Path, PathBuf}};

use anyhow::Context;
use ttf_parser::{Face, GlyphId};

use crate::model::TextStyle;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum FontVariant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontVariant {
    pub fn from_style(style: &TextStyle) -> Self {
        match (style.bold, style.italic) {
            (true, true) => Self::BoldItalic,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (false, false) => Self::Regular,
        }
    }

    pub fn font_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Regular => &["segoeui.ttf", "arial.ttf"],
            Self::Bold => &["segoeuib.ttf", "arialbd.ttf"],
            Self::Italic => &["segoeuii.ttf", "ariali.ttf"],
            Self::BoldItalic => &["segoeuiz.ttf", "arialbi.ttf"],
        }
    }
}

pub fn resolve_font_path(variant: FontVariant) -> anyhow::Result<PathBuf> {
    let fonts_dir = Path::new(r"C:\Windows\Fonts");
    for candidate in variant.font_candidates() {
        let path = fonts_dir.join(candidate);
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "could not find a Unicode-capable system font for {:?} under {}",
        variant,
        fonts_dir.display()
    )
}

pub struct TextMeasurer {
    regular: MeasurementFont,
    bold: MeasurementFont,
    italic: MeasurementFont,
    bold_italic: MeasurementFont,
}

impl TextMeasurer {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            regular: MeasurementFont::load(FontVariant::Regular)?,
            bold: MeasurementFont::load(FontVariant::Bold)?,
            italic: MeasurementFont::load(FontVariant::Italic)?,
            bold_italic: MeasurementFont::load(FontVariant::BoldItalic)?,
        })
    }

    pub fn measure_text_width(&self, text: &str, style: &TextStyle) -> f32 {
        self.select(style).measure_text_width(text, style)
    }

    pub fn wrap_text(&self, text: &str, max_width: f32, style: &TextStyle) -> Vec<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return vec![String::new()];
        }

        let mut lines = Vec::new();
        let mut current = String::new();

        for word in trimmed.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };

            if !current.is_empty() && self.measure_text_width(&candidate, style) > max_width {
                lines.push(current);
                current = word.to_string();
            } else {
                current = candidate;
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        }
    }

    fn select(&self, style: &TextStyle) -> &MeasurementFont {
        match FontVariant::from_style(style) {
            FontVariant::Regular => &self.regular,
            FontVariant::Bold => &self.bold,
            FontVariant::Italic => &self.italic,
            FontVariant::BoldItalic => &self.bold_italic,
        }
    }
}

struct MeasurementFont {
    bytes: &'static [u8],
    face: Face<'static>,
    fallback_gid: u16,
}

impl MeasurementFont {
    fn load(variant: FontVariant) -> anyhow::Result<Self> {
        let path = resolve_font_path(variant)?;
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read font {}", path.display()))?;
        let leaked = Box::leak(bytes.into_boxed_slice());
        let face = Face::parse(leaked, 0)
            .with_context(|| format!("failed to parse font {}", path.display()))?;
        let fallback_gid = face.glyph_index('?').map(|gid| gid.0).unwrap_or(0);
        Ok(Self {
            bytes: leaked,
            face,
            fallback_gid,
        })
    }

    fn measure_text_width(&self, text: &str, style: &TextStyle) -> f32 {
        let units_per_em = self.face.units_per_em().max(1) as f32;
        let mut units = 0f32;
        for ch in text.chars() {
            let gid = self
                .face
                .glyph_index(ch)
                .map(|glyph| glyph.0)
                .unwrap_or(self.fallback_gid);
            let advance = self
                .face
                .glyph_hor_advance(GlyphId(gid))
                .or_else(|| self.face.glyph_hor_advance(GlyphId(self.fallback_gid)))
                .unwrap_or(self.face.units_per_em());
            units += f32::from(advance);
        }
        (units / units_per_em) * style.font_size_pt
    }

    #[allow(dead_code)]
    fn bytes(&self) -> &'static [u8] {
        self.bytes
    }
}
