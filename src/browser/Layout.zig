const std = @import("std");
const lp = @import("lightpanda");
const Frame = @import("Frame.zig");
const Node = @import("webapi/Node.zig");
const Element = @import("webapi/Element.zig");
const DOMRect = @import("webapi/DOMRect.zig");
const Layout = @This();

const Property = extern struct { name: [*]const u8, name_len: usize, value: [*]const u8, value_len: usize };
const Input = extern struct {
    parent: i32,
    display: u32,
    properties: [*]const Property,
    properties_len: usize,
    text: [*]const u8,
    text_len: usize,
    font_size: f32,
    intrinsic_width: f32,
    intrinsic_height: f32,
};
pub const Box = extern struct {
    x: f32 = 0,
    y: f32 = 0,
    width: f32 = 0,
    height: f32 = 0,
    content_width: f32 = 0,
    content_height: f32 = 0,
    border_left: f32 = 0,
    border_top: f32 = 0,
    border_right: f32 = 0,
    border_bottom: f32 = 0,
    padding_left: f32 = 0,
    padding_top: f32 = 0,
    padding_right: f32 = 0,
    padding_bottom: f32 = 0,
    fixed: u32 = 0,
    grid_columns: ?[*]const u8 = null,
    grid_columns_len: usize = 0,
    grid_rows: ?[*]const u8 = null,
    grid_rows_len: usize = 0,
};
extern fn lp_dom_layout(inputs: [*]const Input, count: usize, width: f32, height: f32, outputs: [*]Box, state: *?*anyopaque) c_int;
extern fn lp_dom_layout_free(state: ?*anyopaque) void;

arena: ?*lp.Arena = null,
rust_state: ?*anyopaque = null,
boxes: std.AutoHashMapUnmanaged(*Node, Box) = .empty,
dom_version: usize = std.math.maxInt(usize),
style_version: usize = std.math.maxInt(usize),
viewport_width: u32 = 0,
viewport_height: u32 = 0,

pub fn deinit(self: *Layout) void {
    lp_dom_layout_free(self.rust_state);
    if (self.arena) |arena| arena.release();
}

pub fn ensure(self: *Layout, frame: *Frame) !void {
    const viewport = frame.page.getViewport();
    if (self.dom_version == frame.page.dom_version and self.style_version == frame.page.style_version and
        self.viewport_width == viewport.width and self.viewport_height == viewport.height) return;
    const arena = self.arena orelse try frame.getArena(.medium, "DOM layout");
    self.arena = arena;
    arena.resetRetain();
    self.boxes = .empty;
    const allocator = arena.allocator();
    var nodes: std.ArrayList(*Node) = .empty;
    var inputs: std.ArrayList(Input) = .empty;
    const root = frame.document.getDocumentElement() orelse return;
    const Entry = struct { node: *Node, parent: i32 };
    var stack: std.ArrayList(Entry) = .empty;
    try stack.append(allocator, .{ .node = root.asNode(), .parent = -1 });
    while (stack.pop()) |entry| {
        const node = entry.node;
        if (node.is(Element)) |element| {
            if (frame._style_manager.hasDisplayNone(element)) continue;
        }
        var properties: std.ArrayList(Property) = .empty;
        var display: u32 = 0;
        var text: []const u8 = "";
        var font_size: f32 = 16;
        var intrinsic_width: f32 = 0;
        var intrinsic_height: f32 = 0;
        if (node.is(Element)) |element| {
            display = switch (frame._style_manager.display(element)) {
                .none => 3,
                .flex => 1,
                .grid => 2,
                .other => 0,
            };
            if (frame._style_manager.isInline(element)) display = 4;
            if (element.getTag() == .br) display = 6;
            if (frame._style_manager.hasDisplayNone(element)) display = 3;
            switch (element.getTag()) {
                .input => {
                    const input = element.is(Element.Html.Input).?;
                    if (input._input_type == .checkbox or input._input_type == .radio) {
                        intrinsic_width = 13;
                        intrinsic_height = 13;
                    } else {
                        intrinsic_width = 185;
                        intrinsic_height = 21;
                    }
                    try properties.append(allocator, property("width", if (intrinsic_width == 13) "13px" else "185px"));
                    try properties.append(allocator, property("height", if (intrinsic_height == 13) "13px" else "21px"));
                    try properties.append(allocator, property("box-sizing", "border-box"));
                },
                .button, .select => {
                    try properties.append(allocator, property("width", "max-content"));
                    try properties.append(allocator, property("min-height", "21px"));
                    try properties.append(allocator, property("padding", "2px 6px"));
                },
                .textarea => {
                    intrinsic_width = 182;
                    intrinsic_height = 36;
                    try properties.append(allocator, property("width", "182px"));
                    try properties.append(allocator, property("height", "36px"));
                },
                else => {},
            }
            if (element.getTag() == .body) try properties.append(allocator, property("margin", "8px"));
            for (try frame._style_manager.layoutProperties(element, allocator)) |item| {
                const resolved = try frame._style_manager.resolveProperty(element, item.value, allocator) orelse continue;
                try properties.append(allocator, property(item.name, resolved));
            }
            font_size = @floatCast(frame._style_manager.computedFontSize(element));
        } else if (node.is(Node.CData)) |data| {
            if (data._type != .text) continue;
            display = 5;
            text = data._data.str();
            if (text.len == 0) continue;
            font_size = @floatCast(frame._style_manager.computedFontSize(node.parentElement()));
        } else continue;
        const parent = entry.parent;
        const index: i32 = @intCast(nodes.items.len);
        try nodes.append(allocator, node);
        try inputs.append(allocator, .{ .parent = parent, .display = display, .properties = properties.items.ptr, .properties_len = properties.items.len, .text = text.ptr, .text_len = text.len, .font_size = font_size, .intrinsic_width = intrinsic_width, .intrinsic_height = intrinsic_height });
        var container = node;
        if (node.is(Element)) |element| {
            if (element.hostedShadowRoot(frame)) |shadow| container = shadow.asNode();
            if (node.is(Element.Html.Slot)) |slot| {
                const assigned = try slot.assignedNodes(.{ .flatten = true }, frame);
                if (assigned.len > 0) {
                    var i = assigned.len;
                    while (i > 0) {
                        i -= 1;
                        try stack.append(allocator, .{ .node = assigned[i], .parent = index });
                    }
                    continue;
                }
            }
        }
        var child = container.lastChild();
        while (child) |value| : (child = value.previousSibling()) {
            try stack.append(allocator, .{ .node = value, .parent = index });
        }
    }
    const boxes = try allocator.alloc(Box, inputs.items.len);
    if (lp_dom_layout(inputs.items.ptr, inputs.items.len, @floatFromInt(viewport.width), @floatFromInt(viewport.height), boxes.ptr, &self.rust_state) != 0) return error.LayoutFailed;
    for (nodes.items, boxes) |node, value| try self.boxes.put(allocator, node, value);
    self.dom_version = frame.page.dom_version;
    self.style_version = frame.page.style_version;
    self.viewport_width = viewport.width;
    self.viewport_height = viewport.height;
}

fn property(name: []const u8, value: []const u8) Property {
    return .{ .name = name.ptr, .name_len = name.len, .value = value.ptr, .value_len = value.len };
}

pub fn box(self: *Layout, node: *Node, frame: *Frame) Box {
    if (!node.isConnected()) return .{};
    self.ensure(frame) catch |err| {
        frame.page.recordJsError(err);
        lp.log.err(.browser, "DOM layout failed", .{ .err = err });
        return .{};
    };
    return self.boxes.get(node) orelse .{};
}

pub fn rect(self: *Layout, node: *Node, frame: *Frame) DOMRect.Data {
    const value = self.box(node, frame);
    var bounds: DOMRect.Data = .{ .x = value.x, .y = value.y, .width = value.width, .height = value.height };
    if (node.is(Element)) |element| {
        var transformed: ?*Element = element;
        while (transformed) |ancestor| : (transformed = ancestor.asNode().flatTreeParentElement(frame)) {
            bounds = @import("Transform.zig").apply(bounds, ancestor, frame);
        }
    }
    var dx: f64 = 0;
    var dy: f64 = 0;
    if (value.fixed == 0) {
        var ancestor = node.flatTreeParentElement(frame);
        while (ancestor) |element| : (ancestor = element.asNode().flatTreeParentElement(frame)) {
            if (!element.isRootContainer()) {
                dx += @floatFromInt(element.getScrollLeft(frame));
                dy += @floatFromInt(element.getScrollTop(frame));
            }
        }
    }
    return .{ .x = bounds.x - dx - if (value.fixed == 0) @as(f64, @floatFromInt(frame.window._scroll_pos.x)) else 0, .y = bounds.y - dy - if (value.fixed == 0) @as(f64, @floatFromInt(frame.window._scroll_pos.y)) else 0, .width = bounds.width, .height = bounds.height };
}
