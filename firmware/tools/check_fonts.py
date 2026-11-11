# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools==4.68.0", "brotli==1.2.0"]
# ///
"""验证烧录镜像可以无损定位并打开每个源字体码位。"""

import argparse
import io
import struct
from pathlib import Path

from fontTools.ttLib import TTFont


def verify(source: Path, image: Path) -> None:
    with TTFont(source) as font:
        expected = {cp: font["hmtx"][glyph] for cp, glyph in font.getBestCmap().items()}
        units_per_em = font["head"].unitsPerEm
    blob = image.read_bytes()
    magic, shards, count, size = struct.unpack_from("<4sIII", blob)
    assert magic in (b"TDF1", b"TDF2") and size == len(blob) <= 0x640000
    assert 0 < shards <= 256 and count == len(expected)
    directory = 16 + 8 * shards
    index = struct.unpack_from(f"<{count}I", blob, directory)
    def codepoint(entry):
        return (entry & 0x3FFFFFFF) >> 8

    assert [codepoint(entry) for entry in index] == sorted(expected)
    if magic == b"TDF2":
        assert units_per_em == 1000
        assert all((entry >> 30) * 600 == expected[codepoint(entry)][0] for entry in index)

    by_shard = [set() for _ in range(shards)]
    for entry in index:
        by_shard[entry & 0xFF].add(codepoint(entry))
    end = directory + 4 * count
    for shard in range(shards):
        offset, length = struct.unpack_from("<II", blob, 16 + 8 * shard)
        assert end <= offset and offset % 4 == 0 and length > 0
        end = offset + length
        assert end <= size
        with TTFont(io.BytesIO(blob[offset:end])) as font:
            assert font.flavor == "woff2"
            assert font["head"].unitsPerEm == units_per_em
            metrics = {cp: font["hmtx"][glyph] for cp, glyph in font.getBestCmap().items()}
            assert metrics == {cp: expected[cp] for cp in by_shard[shard]}
    print(f"校验通过：{count} 个码位、{shards} 片、{size:,} 字节")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("image", type=Path)
    args = parser.parse_args()
    verify(args.source, args.image)
