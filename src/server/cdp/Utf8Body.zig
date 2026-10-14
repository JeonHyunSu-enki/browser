// Copyright (C) 2023-2026  Lightpanda (Selecy SAS)
//
// Francis Bouvier <francis@lightpanda.io>
// Pierre Tachoire <pierre@lightpanda.io>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

const std = @import("std");
const Utf8Body = @This();

bytes: []const u8,

// The response path validates UTF-8 before constructing this wrapper.
pub fn jsonStringify(self: Utf8Body, jws: *std.json.Stringify) std.Io.Writer.Error!void {
    try jws.beginWriteRaw();
    try jws.writer.writeByte('"');
    if (jws.options.escape_unicode) {
        try std.json.Stringify.encodeJsonStringChars(self.bytes, jws.options, jws.writer);
    } else {
        var start: usize = 0;
        var i: usize = 0;
        while (i < self.bytes.len) {
            if (self.bytes.len - i >= 32) {
                const block: @Vector(32, u8) = self.bytes[i..][0..32].*;
                const control = block < @as(@Vector(32, u8), @splat(0x20));
                const quote = block == @as(@Vector(32, u8), @splat('"'));
                const slash = block == @as(@Vector(32, u8), @splat('\\'));
                if (!@reduce(.Or, control | quote | slash)) {
                    i += 32;
                    continue;
                }
            }
            const end = @min(i + 32, self.bytes.len);
            while (i < end) : (i += 1) {
                const byte = self.bytes[i];
                if (byte < 0x20 or byte == '"' or byte == '\\') {
                    try jws.writer.writeAll(self.bytes[start..i]);
                    try std.json.Stringify.encodeJsonStringChars(self.bytes[i..][0..1], jws.options, jws.writer);
                    start = i + 1;
                }
            }
        }
        try jws.writer.writeAll(self.bytes[start..]);
    }
    try jws.writer.writeByte('"');
    jws.endWriteRaw();
}
