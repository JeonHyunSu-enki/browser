use super::layout::{measure, Text};
use taffy::compute::BlockContext;
use taffy::prelude::*;
use taffy::tree::{DetailedLayoutInfo, LayoutInput, LayoutOutput};
use taffy::Point;
use taffy::{Cache, CacheTree, LayoutPartialTree, TraversePartialTree, TraverseTree};

struct Node {
    style: Style,
    text: Option<Text>,
    children: Vec<NodeId>,
    parent: Option<NodeId>,
    cache: Cache,
    layout: taffy::Layout,
    hoisted: Vec<NodeId>,
    detailed: DetailedLayoutInfo,
    mode: u32,
    inline_sources: Vec<NodeId>,
}
pub struct Tree {
    nodes: Vec<Node>,
}
impl Tree {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }
    pub fn new_leaf_with_context(&mut self, style: Style, text: Text) -> Result<NodeId, ()> {
        let id = NodeId::from(self.nodes.len());
        self.nodes.push(Node {
            style,
            text: Some(text),
            children: Vec::new(),
            parent: None,
            cache: Cache::new(),
            layout: taffy::Layout::default(),
            hoisted: Vec::new(),
            detailed: DetailedLayoutInfo::None,
            mode: 0,
            inline_sources: Vec::new(),
        });
        Ok(id)
    }
    pub fn new_with_children(&mut self, style: Style, children: &[NodeId]) -> Result<NodeId, ()> {
        let id = self.new_leaf_with_context(style, Text::empty())?;
        self.set_children(id, children)?;
        Ok(id)
    }
    pub fn set_children(&mut self, id: NodeId, children: &[NodeId]) -> Result<(), ()> {
        self.nodes[usize::from(id)].children = children.to_vec();
        for child in children {
            self.nodes[usize::from(*child)].parent = Some(id);
        }
        self.mark_dirty(id);
        Ok(())
    }
    fn mark_dirty(&mut self, mut id: NodeId) {
        loop {
            let node = &mut self.nodes[usize::from(id)];
            node.cache.clear();
            if let Some(parent) = node.parent {
                id = parent
            } else {
                break;
            }
        }
    }
    pub fn style(&self, id: NodeId) -> Result<&Style, ()> {
        Ok(&self.nodes[usize::from(id)].style)
    }
    pub fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), ()> {
        self.nodes[usize::from(id)].style = style;
        self.mark_dirty(id);
        Ok(())
    }
    pub fn get_node_context(&self, id: NodeId) -> Option<&Text> {
        self.nodes[usize::from(id)].text.as_ref()
    }
    pub fn set_node_context(&mut self, id: NodeId, text: Option<Text>) -> Result<(), ()> {
        self.nodes[usize::from(id)].text = text;
        self.mark_dirty(id);
        Ok(())
    }
    pub fn layout(&self, id: NodeId) -> Result<&taffy::Layout, ()> {
        Ok(&self.nodes[usize::from(id)].layout)
    }
    pub fn detailed_layout_info(&self, id: NodeId) -> &DetailedLayoutInfo {
        &self.nodes[usize::from(id)].detailed
    }
    pub fn compute_layout(
        &mut self,
        id: NodeId,
        available: Size<AvailableSpace>,
    ) -> Result<(), ()> {
        taffy::compute_root_layout(self, id, available);
        Ok(())
    }
    pub fn set_mode(&mut self, id: NodeId, mode: u32) {
        self.nodes[usize::from(id)].mode = mode;
    }
    pub fn absolute_position(&self, mut id: NodeId) -> Point<f32> {
        let mut point = Point::ZERO;
        loop {
            let node = &self.nodes[usize::from(id)];
            point.x += node.layout.location.x;
            point.y += node.layout.location.y;
            if node.style.position == Position::Fixed {
                break;
            }
            if let Some(parent) = node.parent {
                id = parent
            } else {
                break;
            }
        }
        point
    }
    pub fn format_inline_children(&mut self, count: usize) {
        for index in 0..count {
            let original = self.nodes[index].children.clone();
            if self.nodes[index].mode == 4 || original.is_empty() {
                continue;
            }
            if !matches!(
                self.nodes[index].style.display,
                Display::Block | Display::FlowRoot
            ) {
                continue;
            }
            let mut children = Vec::new();
            let mut inline = Vec::new();
            for id in original {
                if matches!(self.nodes[usize::from(id)].mode, 4 | 5 | 6) {
                    inline.push(id)
                } else {
                    if !inline.is_empty() {
                        children.push(self.inline_node(std::mem::take(&mut inline)))
                    }
                    children.push(id)
                }
            }
            if !inline.is_empty() {
                children.push(self.inline_node(inline))
            }
            self.set_children(NodeId::from(index), &children).unwrap();
        }
    }
    fn inline_node(&mut self, sources: Vec<NodeId>) -> NodeId {
        let id = self
            .new_with_children(
                Style {
                    display: Display::Block,
                    ..Default::default()
                },
                &sources,
            )
            .unwrap();
        self.nodes[usize::from(id)].inline_sources = sources;
        id
    }
    fn inline_text(&self, id: NodeId, text: &mut String, spans: &mut Vec<super::inline::Span>) {
        let node = &self.nodes[usize::from(id)];
        let start = text.len();
        if let Some(value) = &node.text {
            super::inline::append(text, &value.text)
        }
        if node.mode == 6 {
            text.push('\n')
        }
        for child in &node.children {
            self.inline_text(*child, text, spans)
        }
        let end = text.len();
        if end > start && node.mode == 5 {
            let span = super::inline::Span {
                node: usize::from(id),
                range: start..end,
                font: node
                    .text
                    .as_ref()
                    .map(|v| v.font.clone())
                    .unwrap_or_default(),
            };
            spans.push(span);
        }
    }
    fn refresh_inline(&mut self, id: NodeId) {
        let index = usize::from(id);
        let mut text = String::new();
        let mut spans = Vec::new();
        for source in &self.nodes[index].inline_sources {
            self.inline_text(*source, &mut text, &mut spans)
        }
        let root = self.nodes[index]
            .parent
            .and_then(|p| self.nodes[usize::from(p)].text.as_ref())
            .map(|v| v.font.clone())
            .unwrap_or_default();
        let context = self.nodes[index].text.as_mut().unwrap();
        if context.text != text || context.spans != spans || context.font != root {
            context.text = text;
            context.spans = spans;
            context.font = root;
            context.shaped = None
        }
    }
    fn inline_union(
        &self,
        id: NodeId,
        rects: &mut std::collections::HashMap<usize, taffy::Layout>,
    ) {
        let index = usize::from(id);
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        for child in &self.nodes[index].children {
            self.inline_union(*child, rects);
            if let Some(rect) = rects.get(&usize::from(*child)) {
                let l = rect.location.x;
                let t = rect.location.y;
                let r = l + rect.size.width;
                let b = t + rect.size.height;
                bounds = Some(if let Some(old) = bounds {
                    (old.0.min(l), old.1.min(t), old.2.max(r), old.3.max(b))
                } else {
                    (l, t, r, b)
                });
            }
        }
        if let Some((x, y, right, bottom)) = bounds {
            let mut layout = taffy::Layout::default();
            layout.location = Point { x, y };
            layout.size = Size {
                width: right - x,
                height: bottom - y,
            };
            rects.insert(index, layout);
        }
    }
    fn assign_inline(&mut self, group: NodeId, rects: Vec<(usize, taffy::Layout)>) {
        let mut rectangles: std::collections::HashMap<_, _> = rects.into_iter().collect();
        let sources = self.nodes[usize::from(group)].inline_sources.clone();
        for source in sources {
            self.inline_union(source, &mut rectangles);
        }

        for (&id, &layout) in &rectangles {
            let parent = self.nodes[id].parent;
            let origin = parent
                .filter(|p| *p != group)
                .and_then(|p| rectangles.get(&usize::from(p)))
                .map(|l| l.location)
                .unwrap_or(Point::ZERO);
            self.nodes[id].layout = layout;
            self.nodes[id].layout.location.x -= origin.x;
            self.nodes[id].layout.location.y -= origin.y;
        }
    }
    fn compute_node(
        &mut self,
        id: NodeId,
        input: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if input.run_mode == taffy::RunMode::PerformHiddenLayout {
            return taffy::compute_hidden_layout(self, id);
        }
        taffy::compute_cached_layout(self, id, input, |tree, id, input| {
            let index = usize::from(id);
            let display = tree.nodes[index].style.display;
            let children = !tree.nodes[index].children.is_empty();
            if !tree.nodes[index].inline_sources.is_empty() {
                tree.refresh_inline(id);
                let node = &mut tree.nodes[index];
                let output = measure(input, &node.style, node.text.as_mut());
                if input.run_mode == taffy::RunMode::PerformLayout {
                    let text = tree.nodes[index].text.as_ref().unwrap();
                    if let Some(layout) = &text.shaped {
                        let rects = super::inline::rectangles(layout, &text.spans, &text.text);
                        tree.assign_inline(id, rects);
                    }
                }
                return output;
            }
            let mut output = match (display, children) {
                (Display::None, _) => taffy::compute_hidden_layout(tree, id),
                (Display::Block, true) => taffy::compute_block_layout(tree, id, input, block),
                (Display::FlowRoot, true) => taffy::compute_block_layout(tree, id, input, None),
                (Display::Flex, true) => taffy::compute_flexbox_layout(tree, id, input),
                (Display::Grid, true) => taffy::compute_grid_layout(tree, id, input),
                _ => {
                    let node = &mut tree.nodes[index];
                    measure(input, &node.style, node.text.as_mut())
                }
            };
            if input.run_mode == taffy::RunMode::PerformLayout {
                taffy::compute_oof_layout(tree, id, &mut output)
            }
            output
        })
    }
}
impl TraversePartialTree for Tree {
    type ChildIter<'a> = std::iter::Copied<std::slice::Iter<'a, NodeId>>;
    fn child_ids(&self, id: NodeId) -> Self::ChildIter<'_> {
        self.nodes[usize::from(id)].children.iter().copied()
    }
    fn child_count(&self, id: NodeId) -> usize {
        self.nodes[usize::from(id)].children.len()
    }
    fn get_child_id(&self, id: NodeId, index: usize) -> NodeId {
        self.nodes[usize::from(id)].children[index]
    }
}
impl TraverseTree for Tree {}
impl LayoutPartialTree for Tree {
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type CustomIdent = String;
    fn get_core_container_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn set_unrounded_layout(&mut self, id: NodeId, layout: &taffy::Layout) {
        self.nodes[usize::from(id)].layout = *layout
    }
    fn resolve_calc_value(&self, value: *const (), basis: f32) -> f32 {
        super::math::evaluate(value, basis)
    }
    fn compute_child_layout(&mut self, id: NodeId, input: LayoutInput) -> LayoutOutput {
        self.compute_node(id, input, None)
    }
}
impl taffy::LayoutContainingBlock for Tree {
    type OofItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_oof_item_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn clear_hoisted_children(&mut self, id: NodeId) {
        self.nodes[usize::from(id)].hoisted.clear()
    }
    fn add_hoisted_children(&mut self, id: NodeId, children: &[NodeId]) {
        self.nodes[usize::from(id)]
            .hoisted
            .extend_from_slice(children)
    }
    fn get_detailed_layout_info(&self, id: NodeId) -> &DetailedLayoutInfo {
        &self.nodes[usize::from(id)].detailed
    }
}
impl CacheTree for Tree {
    fn cache_get(&mut self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.nodes[usize::from(id)].cache.get(input)
    }
    fn cache_store(&mut self, id: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.nodes[usize::from(id)].cache.store(input, output)
    }
    fn cache_clear(&mut self, id: NodeId) {
        self.nodes[usize::from(id)].cache.clear();
    }
}
impl taffy::LayoutBlockContainer for Tree {
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_block_container_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn get_block_child_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn compute_block_child_layout(
        &mut self,
        id: NodeId,
        input: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_node(id, input, block)
    }
}
impl taffy::LayoutFlexboxContainer for Tree {
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_flexbox_container_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn get_flexbox_child_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
}
impl taffy::LayoutGridContainer for Tree {
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;
    fn get_grid_container_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn get_grid_child_style(&self, id: NodeId) -> &Style {
        &self.nodes[usize::from(id)].style
    }
    fn set_detailed_grid_info(&mut self, id: NodeId, info: taffy::DetailedGridInfo) {
        self.nodes[usize::from(id)].detailed = DetailedLayoutInfo::Grid(Box::new(info))
    }
}
