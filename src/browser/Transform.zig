const std = @import("std");
const Element = @import("webapi/Element.zig");
const Frame = @import("Frame.zig");
const DOMRect = @import("webapi/DOMRect.zig");

pub const Matrix = struct {
    a: f64 = 1,
    b: f64 = 0,
    c: f64 = 0,
    d: f64 = 1,
    x: f64 = 0,
    y: f64 = 0,

    fn multiply(self: Matrix, rhs: Matrix) Matrix {
        return .{ .a = self.a * rhs.a + self.c * rhs.b, .b = self.b * rhs.a + self.d * rhs.b, .c = self.a * rhs.c + self.c * rhs.d, .d = self.b * rhs.c + self.d * rhs.d, .x = self.a * rhs.x + self.c * rhs.y + self.x, .y = self.b * rhs.x + self.d * rhs.y + self.y };
    }
};

fn number(value: []const u8, basis: f64) f64 {
    if (std.mem.endsWith(u8, value, "%")) return (std.fmt.parseFloat(f64, value[0 .. value.len - 1]) catch 0) * basis / 100;
    const text = if (std.mem.endsWith(u8, value, "px")) value[0 .. value.len - 2] else value;
    return std.fmt.parseFloat(f64, text) catch 0;
}

pub fn parse(value: []const u8, width: f64, height: f64) Matrix {
    var rest = value;
    var result: Matrix = .{};
    while (std.mem.indexOfScalar(u8, rest, '(')) |open| {
        const close = std.mem.indexOfScalarPos(u8, rest, open + 1, ')') orelse break;
        const name = std.mem.trim(u8, rest[0..open], &std.ascii.whitespace);
        var tokens = std.mem.tokenizeAny(u8, rest[open + 1 .. close], ", \t\n");
        var raw: [6][]const u8 = @splat("0");
        var n: usize = 0;
        while (tokens.next()) |token| {
            if (n == 6) break;
            raw[n] = token;
            n += 1;
        }
        const first = number(raw[0], width);
        const second = number(raw[1], height);
        var next: Matrix = .{};
        if (std.ascii.eqlIgnoreCase(name, "translateX")) next.x = first else if (std.ascii.eqlIgnoreCase(name, "translateY")) next.y = number(raw[0], height) else if (std.ascii.eqlIgnoreCase(name, "translate")) {
            next.x = first;
            next.y = second;
        } else if (std.ascii.eqlIgnoreCase(name, "scale")) {
            next.a = first;
            next.d = if (n > 1) second else first;
        } else if (std.ascii.eqlIgnoreCase(name, "scaleX")) next.a = first else if (std.ascii.eqlIgnoreCase(name, "scaleY")) next.d = first else if (std.ascii.eqlIgnoreCase(name, "matrix") and n == 6) {
            next = .{ .a = first, .b = second, .c = number(raw[2], 0), .d = number(raw[3], 0), .x = number(raw[4], 0), .y = number(raw[5], 0) };
        } else if (std.ascii.eqlIgnoreCase(name, "rotate")) {
            var angle: f64 = 0;
            if (std.mem.endsWith(u8, raw[0], "deg")) angle = (std.fmt.parseFloat(f64, raw[0][0 .. raw[0].len - 3]) catch 0) * std.math.pi / 180 else if (std.mem.endsWith(u8, raw[0], "rad")) angle = std.fmt.parseFloat(f64, raw[0][0 .. raw[0].len - 3]) catch 0;
            next = .{ .a = @cos(angle), .b = @sin(angle), .c = -@sin(angle), .d = @cos(angle) };
        }
        result = result.multiply(next);
        rest = rest[close + 1 ..];
    }
    return result;
}

pub fn apply(rect: DOMRect.Data, element: *Element, frame: *Frame) DOMRect.Data {
    const value = frame._style_manager.cascadedProperty(element, "transform") orelse return rect;
    const box = frame.layout.box(element.asNode(), frame);
    const matrix = parse(value, box.width, box.height);
    var ox: f64 = box.x + box.width / 2;
    var oy: f64 = box.y + box.height / 2;
    if (frame._style_manager.cascadedProperty(element, "transform-origin")) |origin| {
        var parts = std.mem.tokenizeAny(u8, origin, &std.ascii.whitespace);
        if (parts.next()) |x| ox = box.x + number(x, box.width);
        if (parts.next()) |y| oy = box.y + number(y, box.height);
    }
    var min_x: f64 = std.math.inf(f64);
    var min_y: f64 = std.math.inf(f64);
    var max_x: f64 = -std.math.inf(f64);
    var max_y: f64 = -std.math.inf(f64);
    for ([_]f64{ rect.x, rect.x + rect.width }) |x| {
        for ([_]f64{ rect.y, rect.y + rect.height }) |y| {
            const tx = matrix.a * (x - ox) + matrix.c * (y - oy) + matrix.x + ox;
            const ty = matrix.b * (x - ox) + matrix.d * (y - oy) + matrix.y + oy;
            min_x = @min(min_x, tx);
            min_y = @min(min_y, ty);
            max_x = @max(max_x, tx);
            max_y = @max(max_y, ty);
        }
    }
    return .{ .x = min_x, .y = min_y, .width = max_x - min_x, .height = max_y - min_y };
}
