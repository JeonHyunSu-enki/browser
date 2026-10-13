const lp = @import("lightpanda");
const js = @import("../../js/js.zig");
const Frame = @import("../../Frame.zig");
const Page = @import("../../Page.zig");
const Element = @import("../Element.zig");
const Animation = @import("Animation.zig");
const KeyframeEffect = @This();

_rc: lp.RC = .{},
_target: ?*Element,
_keyframes: ?js.Object.Global = null,
_animation: ?*Animation = null,
_duration: f64 = 0,
_fill: []const u8 = "none",

pub fn init(target: ?*Element, keyframes: ?js.Object, options: ?js.Value, frame: *Frame) !*KeyframeEffect {
    const effect = try frame._factory.create(KeyframeEffect{ ._target = target, ._keyframes = if (keyframes) |value| try value.persist() else null });
    if (options) |value| {
        if (value.isNumber()) effect._duration = try value.toF64() else if (value.isObject()) {
            const object = value.toObject();
            const duration = try object.get("duration");
            if (duration.isNumber()) effect._duration = try duration.toF64();
            const fill = try object.get("fill");
            if (fill.isString() != null) effect._fill = try frame.page_arena.dupe(u8, try fill.toZig([]const u8));
        }
    }
    return effect;
}
pub fn acquireRef(self: *KeyframeEffect) void {
    self._rc.acquire();
}
pub fn releaseRef(self: *KeyframeEffect, page: *Page) void {
    self._rc.release(self, page);
}
pub fn deinit(self: *KeyframeEffect, page: *Page) void {
    if (self._keyframes) |value| value.release();
    page.factory.destroy(self);
}
fn getTarget(self: *KeyframeEffect) ?*Element {
    return self._target;
}
fn setTarget(self: *KeyframeEffect, target: ?*Element) void {
    self._target = target;
}
const Timing = struct { delay: f64 = 0, endDelay: f64 = 0, fill: []const u8, iterationStart: f64 = 0, iterations: f64 = 1, duration: f64, direction: []const u8 = "normal", easing: []const u8 = "linear" };
fn getTiming(self: *KeyframeEffect) Timing {
    return .{ .fill = self._fill, .duration = self._duration };
}
fn getComputedTiming(self: *KeyframeEffect) struct { delay: f64, endDelay: f64, fill: []const u8, iterationStart: f64, iterations: f64, duration: f64, direction: []const u8, easing: []const u8, endTime: f64, activeDuration: f64, localTime: ?f64, progress: ?f64, currentIteration: ?f64 } {
    const active = self._animation != null;
    const remaining = if (self._animation) |animation| animation._deadline -| lp.datetime.milliTimestamp(.boot) else 0;
    const elapsed = @max(0, self._duration - @as(f64, @floatFromInt(remaining)));
    return .{ .delay = 0, .endDelay = 0, .fill = self._fill, .iterationStart = 0, .iterations = 1, .duration = self._duration, .direction = "normal", .easing = "linear", .endTime = self._duration, .activeDuration = self._duration, .localTime = if (active) elapsed else null, .progress = if (active) if (self._duration > 0) @min(1, elapsed / self._duration) else 1 else null, .currentIteration = if (active) 0 else null };
}
fn getKeyframes(self: *KeyframeEffect, frame: *Frame) ![]js.Value {
    if (self._keyframes) |global| {
        const object = frame.js.toLocal(global);
        if (object.toValue().isArray()) {
            const array: js.Array = .{ .local = object.local, .handle = @ptrCast(object.handle) };
            const result = try frame.call_arena.alloc(js.Value, array.len());
            for (result, 0..) |*value, i| value.* = try array.get(@intCast(i));
            return result;
        }
    }
    return &.{};
}
fn updateTiming(self: *KeyframeEffect, timing: ?struct { duration: ?f64 = null, fill: ?[]const u8 = null }, frame: *Frame) !void {
    if (timing) |value| {
        if (value.duration) |duration| {
            self._duration = duration;
            if (self._animation) |animation| animation._duration_ms = @intFromFloat(@max(0, duration));
        }
        if (value.fill) |fill| self._fill = try frame.page_arena.dupe(u8, fill);
    }
}
pub const JsApi = struct {
    pub const bridge = js.Bridge(KeyframeEffect);
    pub const Meta = struct {
        pub const name = "KeyframeEffect";
        pub const prototype_chain = bridge.prototypeChain();
        pub var class_id: bridge.ClassId = undefined;
    };
    pub const constructor = bridge.constructor(KeyframeEffect.init, .{});
    pub const target = bridge.accessor(KeyframeEffect.getTarget, KeyframeEffect.setTarget, .{});
    pub const getTiming = bridge.function(KeyframeEffect.getTiming, .{});
    pub const getComputedTiming = bridge.function(KeyframeEffect.getComputedTiming, .{});
    pub const getKeyframes = bridge.function(KeyframeEffect.getKeyframes, .{});
    pub const updateTiming = bridge.function(KeyframeEffect.updateTiming, .{});
};
