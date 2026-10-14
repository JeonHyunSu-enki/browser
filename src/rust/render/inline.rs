use super::layout::{Text, FONTS};
use super::{FontFamily, FontStyle, FontWeight, Layout, LineHeight, Rgb, StyleProperty};
use std::ops::Range;

#[derive(Clone, PartialEq)]
pub struct Span {
    pub node: usize,
    pub range: Range<usize>,
    pub font: TextFont,
}
#[derive(Clone, PartialEq)]
pub struct TextFont {
    pub size: f32,
    pub family: String,
    pub weight: f32,
    pub italic: bool,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
}
impl Default for TextFont {
    fn default() -> Self {
        Self {
            size: 16.0,
            family: "serif".into(),
            weight: 400.0,
            italic: false,
            line_height: 0.0,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }
}
impl TextFont {
    pub fn apply(&mut self, name: &str, value: &str) {
        match name {
            "font-family" => self.family = super::fonts::family(value),
            "font-weight" => {
                self.weight = match value {
                    "bold" => 700.0,
                    "normal" => 400.0,
                    _ => value.parse().unwrap_or(self.weight),
                }
            }
            "font-style" => self.italic = value == "italic" || value.starts_with("oblique"),
            "line-height" => {
                self.line_height = if value == "normal" {
                    0.0
                } else if let Ok(v) = value.parse::<f32>() {
                    v * self.size
                } else if let Some(v) = value.strip_suffix("px") {
                    v.parse().unwrap_or(0.0)
                } else if let Some(v) = value.strip_suffix('%') {
                    v.parse::<f32>().unwrap_or(0.0) * self.size / 100.0
                } else {
                    0.0
                }
            }
            "letter-spacing" => {
                self.letter_spacing = value
                    .strip_suffix("px")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0)
            }
            "word-spacing" => {
                self.word_spacing = value
                    .strip_suffix("px")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0)
            }
            "font" => {
                self.weight=400.0;self.italic=false;self.line_height=0.0;
                let parts: Vec<_> = value.split_whitespace().collect();
                for (index, part) in parts.iter().enumerate() {
                    let size = part.split('/').next().unwrap_or("");
                    if size.ends_with("px") || size.ends_with("em") || size.ends_with('%') {
                        self.family = super::fonts::family(&parts[index + 1..].join(" "));
                        if let Some(height) = part.split('/').nth(1) {
                            self.apply("line-height", height)
                        }
                        break;
                    }
                    if *part == "bold" {
                        self.weight = 700.0
                    }
                    if *part == "italic" {
                        self.italic = true
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn shape(text: &str, spans: &[Span], root: &TextFont) -> Layout<Rgb> {
    FONTS.with(|fonts| {
        let mut renderer = fonts.borrow_mut();
        let super::Renderer { lcx, fcx, .. } = &mut *renderer;
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
        builder.push_default(StyleProperty::FontSize(root.size));
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            std::borrow::Cow::Borrowed(&root.family),
        )));
        for span in spans {
            let f = &span.font;
            let range = span.range.clone();
            builder.push(StyleProperty::FontSize(f.size), range.clone());
            builder.push(
                StyleProperty::FontFamily(FontFamily::Source(std::borrow::Cow::Borrowed(
                    &f.family,
                ))),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontWeight(FontWeight::new(f.weight)),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontStyle(if f.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                }),
                range.clone(),
            );
            if f.line_height > 0.0 {
                builder.push(
                    StyleProperty::LineHeight(LineHeight::Absolute(f.line_height)),
                    range.clone(),
                );
            }
            builder.push(
                StyleProperty::LetterSpacing(f.letter_spacing),
                range.clone(),
            );
            builder.push(StyleProperty::WordSpacing(f.word_spacing), range);
        }
        builder.build(text)
    })
}

pub fn append(text: &mut String, value: &str) {
    for c in value.chars() {
        if matches!(c, ' ' | '\n' | '\r' | '\t' | '\u{000c}') {
            if !text.is_empty() && !text.ends_with(' ') {
                text.push(' ')
            }
        } else {
            text.push(c)
        }
    }
}

pub fn rectangles(layout: &Layout<Rgb>, spans: &[Span], text: &str) -> Vec<(usize, taffy::Layout)> {
    let mut rects = std::collections::HashMap::<usize, (f32, f32, f32, f32)>::new();
    let mut ordered: Vec<_> = spans.iter().collect();
    ordered.sort_by_key(|span| span.range.start);
    let mut cursor_y = 0.0;
    for line in layout.lines() {
        let line_y = line.metrics().baseline;
        let mut x = line.metrics().offset;
        for run in line.runs() {
            let metrics = run.metrics();
            for cluster in run.visual_clusters() {
                let range = cluster.text_range();
                let width = cluster.advance();
                let visible_advance = line.metrics().advance - line.metrics().trailing_whitespace;
                if text[range.clone()].bytes().all(|c|c.is_ascii_whitespace()) && x >= visible_advance {
                    x += width;
                    continue;
                }
                let start = ordered.partition_point(|span| span.range.end <= range.start);
                for span in ordered[start..]
                    .iter()
                    .take_while(|span| span.range.start < range.end)
                {
                    if span.range.start < range.end && range.start < span.range.end {
                        let top = cursor_y
                            + (line_y - line.metrics().block_min_coord - metrics.ascent).round();
                        let bottom = top + metrics.ascent.round() + metrics.descent.round();
                        rects
                            .entry(span.node)
                            .and_modify(|r| {
                                r.0 = r.0.min(x);
                                r.1 = r.1.min(top);
                                r.2 = r.2.max(x + width);
                                r.3 = r.3.max(bottom)
                            })
                            .or_insert((x, top, x + width, bottom));
                    }
                }
                x += width;
            }
        }
        let normal = spans
            .iter()
            .filter(|span| {
                span.range.start < line.text_range().end && line.text_range().start < span.range.end
            })
            .all(|span| span.font.line_height == 0.0);
        cursor_y += if normal {
            line.metrics().line_height.round()
        } else {
            line.metrics().line_height
        };
    }
    rects
        .into_iter()
        .map(|(id, (x, y, right, bottom))| {
            let mut result = taffy::Layout::default();
            result.location = taffy::Point { x, y };
            result.size = taffy::Size {
                width: right - x,
                height: bottom - y,
            };
            (id, result)
        })
        .collect()
}

pub fn height(layout: &Layout<Rgb>, font: &TextFont, spans: &[Span]) -> f32 {
    if font.line_height != 0.0 || spans.iter().any(|span| span.font.line_height != 0.0) {
        return layout.height();
    }
    layout
        .lines()
        .map(|line| line.metrics().line_height.round())
        .sum()
}
