use super::{Layout, Renderer, Rgb};
use std::cell::RefCell;
use std::ffi::c_void;
use std::slice;
use taffy::prelude::*;
use taffy::style::*;
use taffy::tree::{LayoutInput, LayoutOutput};

#[repr(C)]
pub struct Property {
    name: *const u8,
    name_len: usize,
    value: *const u8,
    value_len: usize,
}
#[repr(C)]
pub struct Input {
    parent: i32,
    display: u32,
    properties: *const Property,
    properties_len: usize,
    text: *const u8,
    text_len: usize,
    font_size: f32,
    intrinsic_width: f32,
    intrinsic_height: f32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Output {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    content_width: f32,
    content_height: f32,
    border_left: f32,
    border_top: f32,
    border_right: f32,
    border_bottom: f32,
    padding_left: f32,
    padding_top: f32,
    padding_right: f32,
    padding_bottom: f32,
    fixed: u32,
    grid_columns: *const u8,
    grid_columns_len: usize,
    grid_rows: *const u8,
    grid_rows_len: usize,
}
impl Default for Output {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[derive(Default)]
struct Expressions {
    values: std::collections::HashMap<String, Box<super::math::Expression>>,
    used: std::collections::HashSet<String>,
}
struct State {
    strings: Vec<Box<[u8]>>,
    tree: super::tree::Tree,
    ids: Vec<NodeId>,
    parents: Vec<i32>,
    modes: Vec<u32>,
    viewport_root: Option<NodeId>,
    expressions: Expressions,
}
impl State {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
            tree: super::tree::Tree::new(),
            ids: Vec::new(),
            parents: Vec::new(),
            modes: Vec::new(),
            viewport_root: None,
            expressions: Expressions::default(),
        }
    }
}
fn retain(strings: &mut Vec<Box<[u8]>>, value: String) -> (*const u8, usize) {
    let data = value.into_bytes().into_boxed_slice();
    let result = (data.as_ptr(), data.len());
    strings.push(data);
    result
}
#[no_mangle]
pub unsafe extern "C" fn lp_dom_layout_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut State));
    }
}
pub(super) struct Text {
    pub text: String,
    pub font: super::inline::TextFont,
    pub width: f32,
    pub height: f32,
    pub spans: Vec<super::inline::Span>,
    pub shaped: Option<Layout<Rgb>>,
}
impl Text {
    pub(super) fn empty() -> Self {
        Self {
            text: String::new(),
            font: super::inline::TextFont::default(),
            width: 0.0,
            height: 0.0,
            spans: Vec::new(),
            shaped: None,
        }
    }
}

thread_local! { pub(super) static FONTS: RefCell<Renderer> = RefCell::new(super::fonts::renderer()); }

unsafe fn string<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    std::str::from_utf8(slice::from_raw_parts(ptr, len)).unwrap_or("")
}
fn unit(value: &str, viewport: Size<f32>, font: f32, root: f32) -> String {
    for (suffix, scale) in [
        ("rem", root),
        ("em", font),
        ("vw", viewport.width / 100.0),
        ("vh", viewport.height / 100.0),
        ("vmin", viewport.width.min(viewport.height) / 100.0),
        ("vmax", viewport.width.max(viewport.height) / 100.0),
        ("pt", 4.0 / 3.0),
    ] {
        if let Some(number) = value
            .strip_suffix(suffix)
            .and_then(|s| s.trim().parse::<f32>().ok())
        {
            return format!("{}px", number * scale);
        }
    }
    if value == "0" {
        "0px".into()
    } else {
        value.into()
    }
}
trait LengthValue {
    fn fixed(value: f32) -> Self;
    fn expression(value: *const ()) -> Self;
}
impl LengthValue for Dimension {
    fn fixed(v: f32) -> Self {
        Self::length(v)
    }
    fn expression(p: *const ()) -> Self {
        Self::calc(p)
    }
}
impl LengthValue for LengthPercentage {
    fn fixed(v: f32) -> Self {
        Self::length(v)
    }
    fn expression(p: *const ()) -> Self {
        Self::calc(p)
    }
}
impl LengthValue for LengthPercentageAuto {
    fn fixed(v: f32) -> Self {
        Self::length(v)
    }
    fn expression(p: *const ()) -> Self {
        Self::calc(p)
    }
}
fn length<T: LengthValue + std::str::FromStr>(
    value: &str,
    viewport: Size<f32>,
    font: f32,
    root: f32,
    expressions: &mut Expressions,
) -> Option<T> {
    let normalized = unit(value.trim(), viewport, font, root);
    if let Ok(v) = normalized.parse::<T>() {
        return Some(v);
    }
    let key = format!(
        "{}:{}:{}:{}:{}",
        value,
        font.to_bits(),
        root.to_bits(),
        viewport.width.to_bits(),
        viewport.height.to_bits()
    );
    if !expressions.values.contains_key(&key) {
        expressions.values.insert(
            key.clone(),
            Box::new(super::math::parse(value, font, root, viewport.width, viewport.height).ok()?),
        );
    }
    expressions.used.insert(key.clone());
    let expression = expressions.values.get(&key)?;
    if expression.depends_on_basis() {
        Some(T::expression(expression.as_ref() as *const _ as *const ()))
    } else {
        Some(T::fixed(expression.evaluate(0.0) as f32))
    }
}
fn apply(
    style: &mut Style,
    name: &str,
    raw: &str,
    viewport: Size<f32>,
    font: f32,
    root: f32,
    expressions: &mut Expressions,
) {
    let value = unit(raw.trim(), viewport, font, root);
    macro_rules! parsed {
        ($field:expr) => {
            if let Ok(v) = value.parse() {
                $field = v;
            }
        };
    }
    macro_rules! distance {
        ($field:expr) => {
            if let Some(v) = length(raw, viewport, font, root, expressions) {
                $field = v;
            }
        };
    }
    match name {
        "display" => {
            style.display = match raw {
                "none" => Display::None,
                "flex" | "inline-flex" => Display::Flex,
                "grid" | "inline-grid" => Display::Grid,
                "flow-root" => Display::FlowRoot,
                _ => Display::Block,
            }
        }
        "position" => {
            style.position = match raw {
                "absolute" => Position::Absolute,
                "fixed" => Position::Fixed,
                "relative" => Position::Relative,
                "sticky" => Position::Sticky,
                _ => Position::Static,
            }
        }
        "box-sizing" => {
            style.box_sizing = if raw == "border-box" {
                BoxSizing::BorderBox
            } else {
                BoxSizing::ContentBox
            }
        }
        "width" => distance!(style.size.width),
        "height" => distance!(style.size.height),
        "min-width" => distance!(style.min_size.width),
        "min-height" => distance!(style.min_size.height),
        "max-width" => distance!(style.max_size.width),
        "max-height" => distance!(style.max_size.height),
        "top" => distance!(style.inset.top),
        "right" => distance!(style.inset.right),
        "bottom" => distance!(style.inset.bottom),
        "left" => distance!(style.inset.left),
        "flex-grow" => parsed!(style.flex_grow),
        "flex-shrink" => parsed!(style.flex_shrink),
        "flex-basis" => distance!(style.flex_basis),
        "flex-direction" => parsed!(style.flex_direction),
        "flex-wrap" => parsed!(style.flex_wrap),
        "align-items" => parsed!(style.align_items),
        "align-content" => parsed!(style.align_content),
        "justify-content" => parsed!(style.justify_content),
        "justify-items" => parsed!(style.justify_items),
        "align-self" => {
            if let Ok(v) = value.parse() {
                style.align_self = Some(v)
            }
        }
        "justify-self" => {
            if let Ok(v) = value.parse() {
                style.justify_self = Some(v)
            }
        }
        "row-gap" => distance!(style.gap.height),
        "column-gap" => distance!(style.gap.width),
        "gap" => {
            let mut parts = raw.split_whitespace();
            if let Some(a) = parts.next() {
                apply(style, "row-gap", a, viewport, font, root, expressions);
                apply(
                    style,
                    "column-gap",
                    parts.next().unwrap_or(a),
                    viewport,
                    font,
                    root,
                    expressions,
                )
            }
        }
        "flex" => {
            let parts: Vec<_> = raw.split_whitespace().collect();
            if raw == "none" {
                style.flex_grow = 0.0;
                style.flex_shrink = 0.0;
                style.flex_basis = Dimension::auto();
            } else if raw == "auto" {
                style.flex_grow = 1.0;
                style.flex_shrink = 1.0;
                style.flex_basis = Dimension::auto();
            } else if let Some(grow) = parts.first().and_then(|v| v.parse::<f32>().ok()) {
                style.flex_grow = grow;
                style.flex_shrink = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0);
                style.flex_basis = parts
                    .get(2)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(Dimension::percent(0.0));
            }
        }
        "grid-template-columns" | "grid-template-rows" => {
            if let Ok(tracks) =
                raw.parse::<GridTemplateTracks<String, GridTemplateComponent<String>>>()
            {
                if name.ends_with("columns") {
                    style.grid_template_columns = tracks.tracks;
                } else {
                    style.grid_template_rows = tracks.tracks;
                }
            }
        }
        "grid-auto-flow" => parsed!(style.grid_auto_flow),
        "grid-column-start" => parsed!(style.grid_column.start),
        "grid-column-end" => parsed!(style.grid_column.end),
        "grid-row-start" => parsed!(style.grid_row.start),
        "grid-row-end" => parsed!(style.grid_row.end),
        "overflow" | "overflow-x" | "overflow-y" => {
            let v = match raw {
                "hidden" => Overflow::Hidden,
                "clip" => Overflow::Clip,
                "auto" | "scroll" => Overflow::Scroll,
                _ => Overflow::Visible,
            };
            if name != "overflow-y" {
                style.overflow.x = v;
            }
            if name != "overflow-x" {
                style.overflow.y = v;
            }
        }
        "aspect-ratio" => {
            let mut parts = raw.split('/');
            let a = parts.next().and_then(|s| s.trim().parse::<f32>().ok());
            let b = parts
                .next()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .unwrap_or(1.0);
            style.aspect_ratio = a.filter(|v| *v > 0.0 && b > 0.0).map(|v| v / b);
        }
        _ if name == "margin" || name == "padding" || name == "border-width" => {
            let p: Vec<_> = raw.split_whitespace().collect();
            if !p.is_empty() && p.len() <= 4 {
                let values = [
                    p[0],
                    *p.get(1).unwrap_or(&p[0]),
                    *p.get(2).unwrap_or(&p[0]),
                    *p.get(3).unwrap_or(p.get(1).unwrap_or(&p[0])),
                ];
                let base = if name == "border-width" {
                    "border"
                } else {
                    name
                };
                for (edge, v) in ["top", "right", "bottom", "left"].iter().zip(values) {
                    let key = if base == "border" {
                        format!("border-{}-width", edge)
                    } else {
                        format!("{}-{}", base, edge)
                    };
                    apply(style, &key, v, viewport, font, root, expressions);
                }
            }
        }
        "margin-top" => distance!(style.margin.top),
        "margin-right" => distance!(style.margin.right),
        "margin-bottom" => distance!(style.margin.bottom),
        "margin-left" => distance!(style.margin.left),
        "padding-top" => distance!(style.padding.top),
        "padding-right" => distance!(style.padding.right),
        "padding-bottom" => distance!(style.padding.bottom),
        "padding-left" => distance!(style.padding.left),
        "border-top-width" => distance!(style.border.top),
        "border-right-width" => distance!(style.border.right),
        "border-bottom-width" => distance!(style.border.bottom),
        "border-left-width" => distance!(style.border.left),
        "border" => {
            if let Some(v) = raw.split_whitespace().next() {
                apply(style, "border-width", v, viewport, font, root, expressions)
            }
        }
        _ => {}
    }
}
pub(super) fn measure(
    input: LayoutInput,
    style: &Style,
    context: Option<&mut Text>,
) -> LayoutOutput {
    taffy::compute_leaf_layout(input, style, super::math::evaluate, |known, available| {
        let Some(context) = context else {
            return Size::ZERO;
        };
        if context.text.is_empty() {
            return Size {
                width: known.width.unwrap_or(context.width),
                height: known.height.unwrap_or(context.height),
            };
        }
        {
            if context.shaped.is_none() {
                context.shaped = Some(super::inline::shape(
                    &context.text,
                    &context.spans,
                    &context.font,
                ));
            }
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(v) => Some(v),
                AvailableSpace::MinContent => Some(0.0),
                _ => None,
            });
            let text_layout = context.shaped.as_mut().unwrap();
            text_layout.break_all_lines(width);
            Size {
                width: known.width.unwrap_or(text_layout.width()),
                height: known.height.unwrap_or(super::inline::height(
                    text_layout,
                    &context.font,
                    &context.spans,
                )),
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn lp_dom_layout(
    inputs: *const Input,
    count: usize,
    width: f32,
    height: f32,
    outputs: *mut Output,
    state: *mut *mut c_void,
) -> i32 {
    std::panic::catch_unwind(|| {
        if count == 0 {
            return Ok(());
        }
        let inputs = slice::from_raw_parts(inputs, count);
        let outputs = slice::from_raw_parts_mut(outputs, count);
        if (*state).is_null() {
            *state = Box::into_raw(Box::new(State::new())) as *mut c_void;
        }
        let saved = &mut *(*state as *mut State);
        let reusable = saved.parents.len() == count
            && saved
                .parents
                .iter()
                .zip(inputs)
                .all(|(parent, input)| *parent == input.parent)
            && saved
                .modes
                .iter()
                .zip(inputs)
                .all(|(mode, input)| *mode == input.display);
        if !reusable {
            saved.tree = super::tree::Tree::new();
            saved.ids.clear();
            saved.parents.clear();
            saved.modes.clear();
            saved.viewport_root = None;
        }
        saved.strings.clear();
        saved.expressions.used.clear();
        let viewport = Size { width, height };
        let mut children = vec![Vec::new(); count];
        let tree = &mut saved.tree;
        let ids = &mut saved.ids;
        for (index, input) in inputs.iter().enumerate() {
            let mut style = Style {
                display: match input.display {
                    1 => Display::Flex,
                    2 => Display::Grid,
                    3 => Display::None,
                    _ => Display::Block,
                },
                box_sizing: BoxSizing::ContentBox,
                ..Default::default()
            };
            let mut font = if input.parent >= 0 {
                tree.get_node_context(ids[input.parent as usize])
                    .map(|text| text.font.clone())
                    .unwrap_or_default()
            } else {
                super::inline::TextFont::default()
            };
            font.size = input.font_size;
            if input.properties_len > 0 {
                for p in slice::from_raw_parts(input.properties, input.properties_len) {
                    let name = string(p.name, p.name_len);
                    let value = string(p.value, p.value_len);
                    font.apply(name, value);
                    apply(
                        &mut style,
                        name,
                        value,
                        viewport,
                        input.font_size,
                        inputs[0].font_size,
                        &mut saved.expressions,
                    )
                }
            }
            let text = string(input.text, input.text_len);
            let id = if reusable {
                let id = ids[index];
                if tree.style(id).map_err(|_| ())? != &style {
                    tree.set_style(id, style).map_err(|_| ())?;
                }
                let old = tree.get_node_context(id);
                if old.is_none_or(|old| {
                    old.text != text
                        || old.font != font
                        || old.width != input.intrinsic_width
                        || old.height != input.intrinsic_height
                }) {
                    tree.set_node_context(
                        id,
                        Some(Text {
                            text: text.to_owned(),
                            font,
                            width: input.intrinsic_width,
                            height: input.intrinsic_height,
                            spans: Vec::new(),
                            shaped: None,
                        }),
                    )
                    .map_err(|_| ())?;
                }
                id
            } else {
                let id = tree
                    .new_leaf_with_context(
                        style,
                        Text {
                            text: text.to_owned(),
                            font,
                            width: input.intrinsic_width,
                            height: input.intrinsic_height,
                            spans: Vec::new(),
                            shaped: None,
                        },
                    )
                    .map_err(|_| ())?;
                ids.push(id);
                saved.parents.push(input.parent);
                saved.modes.push(input.display);
                id
            };
            tree.set_mode(id, input.display);
            if input.parent >= 0 {
                children[input.parent as usize].push(id);
            }
        }
        if !reusable {
            for (id, children) in ids.iter().zip(children) {
                tree.set_children(*id, &children).map_err(|_| ())?;
            }
            tree.format_inline_children(count);
        }
        let root_style = Style {
            display: Display::Block,
            position: Position::Relative,
            size: Size {
                width: Dimension::length(width),
                height: Dimension::length(height),
            },
            ..Default::default()
        };
        let viewport_root = if let Some(id) = saved.viewport_root {
            if tree.style(id).map_err(|_| ())? != &root_style {
                tree.set_style(id, root_style).map_err(|_| ())?;
            }
            id
        } else {
            let id = tree
                .new_with_children(root_style, &[ids[0]])
                .map_err(|_| ())?;
            saved.viewport_root = Some(id);
            id
        };
        tree.compute_layout(
            viewport_root,
            Size {
                width: AvailableSpace::Definite(width),
                height: AvailableSpace::Definite(height),
            },
        )
        .map_err(|_| ())?;
        for (index, id) in ids.iter().enumerate() {
            let mut columns = (std::ptr::null(), 0);
            let mut rows = (std::ptr::null(), 0);
            if let taffy::tree::DetailedLayoutInfo::Grid(info) = tree.detailed_layout_info(*id) {
                columns = retain(
                    &mut saved.strings,
                    info.columns
                        .positions
                        .iter()
                        .map(|p| format!("{}px", p.end - p.start))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                rows = retain(
                    &mut saved.strings,
                    info.rows
                        .positions
                        .iter()
                        .map(|p| format!("{}px", p.end - p.start))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            let l = tree.layout(*id).map_err(|_| ())?;
            let mut x = l.location.x;
            let mut y = l.location.y;
            let fixed = tree.style(*id).map_err(|_| ())?.position == Position::Fixed;
            if !fixed {
                let position = tree.absolute_position(*id);
                x = position.x;
                y = position.y;
            }
            let q = |v: f32| (v * 64.0).floor() / 64.0;
            outputs[index] = Output {
                x: q(x),
                y: q(y),
                width: q(l.size.width),
                height: q(l.size.height),
                content_width: l.scrollable_overflow_rect.right,
                content_height: l.scrollable_overflow_rect.bottom,
                border_left: l.border.left,
                border_top: l.border.top,
                border_right: l.border.right,
                border_bottom: l.border.bottom,
                padding_left: l.padding.left,
                padding_top: l.padding.top,
                padding_right: l.padding.right,
                padding_bottom: l.padding.bottom,
                fixed: fixed as u32,
                grid_columns: columns.0,
                grid_columns_len: columns.1,
                grid_rows: rows.0,
                grid_rows_len: rows.1,
            };
        }
        saved.expressions.values.retain(|key, _| saved.expressions.used.contains(key));
        Ok::<(), ()>(())
    })
    .map_or(-2, |result| if result.is_ok() { 0 } else { -1 })
}

#[no_mangle]
pub unsafe extern "C" fn lp_css_absolute_length(
    value: *const u8,
    len: usize,
    font: f32,
    root: f32,
    width: f32,
    height: f32,
    result: *mut f32,
) -> i32 {
    let Ok(expression) = super::math::parse(string(value, len), font, root, width, height) else {
        return 0;
    };
    if expression.depends_on_basis() {
        return 0;
    }
    let absolute = expression.evaluate(0.0) as f32;
    if !absolute.is_finite() || absolute < 0.0 {
        return 0;
    }
    *result = absolute;
    1
}
