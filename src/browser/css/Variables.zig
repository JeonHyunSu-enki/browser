const std = @import("std");
const Element = @import("../webapi/Element.zig");
const StyleManager = @import("../StyleManager.zig");
const Allocator = std.mem.Allocator;
const MAX_DEPTH = 256;
const MAX_EXPANSION = 1024 * 1024;

fn varAt(value: []const u8, at: usize) bool {
    if (at > 0) {
        const previous = value[at - 1];
        if (std.ascii.isAlphanumeric(previous) or previous == '-' or previous == '_' or previous == '\\' or previous >= 0x80) return false;
    }
    return at + 4 <= value.len and std.ascii.eqlIgnoreCase(value[at..][0..4], "var(");
}
pub fn containsReference(value: []const u8) bool {
    for (0..value.len) |i| if (varAt(value, i)) return true;
    return false;
}
fn removeComments(value: []const u8, allocator: Allocator) ![]const u8 {
    if (std.mem.indexOf(u8, value, "/*") == null) return value;
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < value.len) {
        if (value[i] == '\'' or value[i] == '"') {
            const end = quotedEnd(value, i);
            try result.appendSlice(allocator, value[i..end]);
            i = end;
        } else if (i + 1 < value.len and std.mem.eql(u8, value[i..][0..2], "/*")) {
            const end = std.mem.indexOfPos(u8, value, i + 2, "*/") orelse return error.InvalidCssVariable;
            i = end + 2;
        } else {
            try result.append(allocator, value[i]);
            i += 1;
        }
    }
    return result.items;
}
fn quotedEnd(value: []const u8, at: usize) usize {
    const quote = value[at];
    var i = at + 1;
    while (i < value.len) : (i += 1) {
        if (value[i] == '\\') {
            i += 1;
            continue;
        }
        if (value[i] == quote) return i + 1;
    }
    return value.len;
}
const Arguments = struct { name: []const u8, fallback: ?[]const u8, end: usize };
fn arguments(value: []const u8, at: usize) !Arguments {
    var depth: usize = 1;
    var comma: ?usize = null;
    var i = at + 4;
    while (i < value.len) : (i += 1) {
        if (value[i] == '\'' or value[i] == '"') {
            i = quotedEnd(value, i) - 1;
            continue;
        }
        if (value[i] == '(') depth += 1;
        if (value[i] == ')') {
            depth -= 1;
            if (depth == 0) {
                const name = std.mem.trim(u8, value[at + 4 .. comma orelse i], &std.ascii.whitespace);
                if (!std.mem.startsWith(u8, name, "--")) return error.InvalidCssVariable;
                return .{ .name = name, .fallback = if (comma) |split| value[split + 1 .. i] else null, .end = i + 1 };
            }
        }
        if (value[i] == ',' and depth == 1 and comma == null) comma = i;
    }
    return error.InvalidCssVariable;
}
fn hasCycle(sm: *StyleManager, element: *Element, root: []const u8, name: []const u8, seen: *std.ArrayList([]const u8), allocator: Allocator) !bool {
    if (seen.items.len >= MAX_DEPTH) return error.CssVariableDepth;
    for (seen.items) |visited| if (std.mem.eql(u8, visited, name)) return false;
    const declared = try sm.rawLayoutProperty(element, name) orelse return false;
    const raw = try removeComments(declared, allocator);
    try seen.append(allocator, name);
    var i: usize = 0;
    while (i < raw.len) : (i += 1) {
        if (raw[i] == '\'' or raw[i] == '"') {
            i = quotedEnd(raw, i) - 1;
            continue;
        }
        if (varAt(raw, i)) {
            const arg = try arguments(raw, i);
            if (std.mem.eql(u8, arg.name, root)) return true;
            if (try hasCycle(sm, element, root, arg.name, seen, allocator)) return true;
        }
    }
    return false;
}
pub fn custom(sm: *StyleManager, element: *Element, name: []const u8, allocator: Allocator, depth: usize) anyerror!?[]const u8 {
    if (depth > MAX_DEPTH) return error.CssVariableDepth;
    if (try sm.rawLayoutProperty(element, name)) |raw| {
        const value = std.mem.trim(u8, raw, &std.ascii.whitespace);
        if (std.ascii.eqlIgnoreCase(value, "initial")) return null;
        if (!std.ascii.eqlIgnoreCase(value, "inherit") and !std.ascii.eqlIgnoreCase(value, "unset")) {
            var seen: std.ArrayList([]const u8) = .empty;
            if (try hasCycle(sm, element, name, name, &seen, allocator)) return null;
            return substitute(sm, element, value, allocator, depth + 1) catch |err| switch (err) {
                error.InvalidCssVariable => null,
                else => return err,
            };
        }
    }
    const parent = element.asNode().flatTreeParentElement(sm.frame) orelse return null;
    return custom(sm, parent, name, allocator, depth + 1);
}
pub fn substitute(sm: *StyleManager, element: *Element, input: []const u8, allocator: Allocator, depth: usize) anyerror![]const u8 {
    if (depth > MAX_DEPTH) return error.CssVariableDepth;
    const value = try removeComments(input, allocator);
    var result: std.ArrayList(u8) = .empty;
    var i: usize = 0;
    while (i < value.len) {
        if (value[i] == '\'' or value[i] == '"') {
            const end = quotedEnd(value, i);
            try result.appendSlice(allocator, value[i..end]);
            i = end;
            continue;
        }
        if (varAt(value, i)) {
            const arg = try arguments(value, i);
            const resolved = try custom(sm, element, arg.name, allocator, depth + 1);
            const replacement = resolved orelse if (arg.fallback) |fallback| try substitute(sm, element, fallback, allocator, depth + 1) else return error.InvalidCssVariable;
            try result.appendSlice(allocator, replacement);
            i = arg.end;
        } else {
            try result.append(allocator, value[i]);
            i += 1;
        }
        if (result.items.len > MAX_EXPANSION) return error.CssVariableExpansion;
    }
    return result.items;
}
