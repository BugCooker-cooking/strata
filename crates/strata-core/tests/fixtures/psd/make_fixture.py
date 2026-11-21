"""Regenerate Strata's small original 16-bit RGB PSD for command and native tests."""
from pathlib import Path
from struct import pack
from zlib import compress


def section(data):
    return pack(">I", len(data)) + data


def tag(key, data):
    return b"8BIM" + key + section(data) + bytes(-len(data) % 2)


def record(name, bounds, planes=(), tags=b"", mask=b"", hidden=False):
    utf16 = name.encode("utf-16-be")
    extra = section(mask) + section(b"") + b"\x00" * 4
    extra += tag(b"luni", pack(">I", len(utf16) // 2) + utf16) + tags
    table = b"".join(pack(">hI", channel, len(data)) for channel, data in planes)
    header = pack(">4iH", *bounds, len(planes)) + table + b"8BIMnorm"
    return header + bytes([255, 0, 2 if hidden else 0, 0]) + section(extra), b"".join(data for _, data in planes)


def raw(values):
    return b"\x00\x00" + pack(f">{len(values)}H", *values)


def predicted(values, width):
    deltas = [v if i % width == 0 else (v - values[i - 1]) % 65536 for i, v in enumerate(values)]
    return b"\x00\x03" + compress(raw(deltas)[2:])


def folder(kind):
    return tag(b"lsct", pack(">I", kind) + (b"8BIMpass" if kind == 1 else b""))


def main():
    base = [(i, raw([v] * 12)) for i, v in enumerate([10001, 20002, 30003])]
    patch = [(i, predicted(values, 3)) for i, values in [
        (0, [12345, 33333, 22222] * 2), (1, [0] * 6), (2, [45678] * 6),
        (-1, [0, 65535, 32769] * 2)]]
    # Two PackBits literal rows of two 16-bit mask samples, including soft coverage.
    rows = [b"\x03" + pack(">2H", *values) for values in [(0, 65535), (10001, 40003)]]
    patch.append((-2, pack(">3H", 1, len(rows[0]), len(rows[1])) + b"".join(rows)))
    mask = pack(">4iBBxx", 1, 0, 3, 2, 255, 0)
    empty = (0, 0, 0, 0)
    records = [
        record("Base", (0, 0, 3, 4), base),
        record("</group>", empty, tags=folder(3)),
        record("细节", (1, -1, 3, 2), patch, mask=mask),
        record("Exposure", empty, tags=tag(b"expA", pack(">Hfff", 1, 1.5, 0, 1)), hidden=True),
        record("Invert", empty, tags=tag(b"nvrt", b""), hidden=True),
        record("Group", empty, tags=folder(1)),
    ]
    table = pack(">h", len(records)) + b"".join(r for r, _ in records) + b"".join(p for _, p in records)
    layers = section(b"") + section(b"") + b"8BIMLr16" + section(table) + bytes(-len(table) % 4)
    dpi = pack(">IHHIHH", 300 << 16, 1, 1, 300 << 16, 1, 1)
    resources = b"8BIM" + pack(">H", 1005) + b"\x00\x00" + section(dpi)
    header = b"8BPS" + pack(">H6xHIIHH", 1, 3, 3, 4, 16, 3)
    # Layer data is authoritative. Use the PSD writer convention for unavailable composite data.
    compatibility = b"8BIM" + pack(">H", 1057) + b"\x00\x00" + section(pack(">IBIII", 1, 0, 0, 0, 1)) + bytes(1)
    preview = raw([0] * 36)
    Path(__file__).with_name("group-mask.psd").write_bytes(
        header + section(b"") + section(resources + compatibility) + section(layers) + preview)


if __name__ == "__main__":
    main()
