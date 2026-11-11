# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools==4.68.0", "brotli==1.2.0"]
# ///
"""从桌面使用的 Maple Mono CN 构建固件 TDF2 分区镜像。"""

import argparse
import hashlib
import io
import struct
import tempfile
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

from check_fonts import verify

ROOT = Path(__file__).resolve().parents[2]
DESKTOP_FONT = ROOT / "src/assets/MapleMono-CN-Regular.woff2"
PARTITION_SIZE = 0x640000
SHARD_CODEPOINTS = 1024
UPSTREAM_TTF_SHA256 = "0fcc5139bfd5ab14ff989bb7dfd5236d0cafc10e60b9828569e6883613ab4085"
UI_CHARACTERS = (
    "…—↑↓→×·：％，。！？（）"
    "等待连接同步失败已断开运行中完成中断异常未知剩余额度速度重置卡张"
    "任务输出开机时长内存占用系统电脑数据无电池供电正在充本地外部"
    "未命名对话软件更新率页秒时天分指标倒计刷新响应速趋势区间"
    "设备设置屏幕亮度设备信息返回仪表盘用户键选择确认长按退出保存型号固件重试"
)


def import_regular(source: Path) -> None:
    if hashlib.sha256(source.read_bytes()).hexdigest() != UPSTREAM_TTF_SHA256:
        raise ValueError("TTF 校验失败，需要上游 v7.9 CN-unhinted ZIP 中的 Regular 字体")
    with TTFont(source, recalcTimestamp=False) as font:
        if font["name"].getDebugName(6) != "MapleMono-CN-Regular":
            raise ValueError("需要 Maple Mono CN Regular 静态字体（无 NF）")
        font.flavor = "woff2"
        buf = io.BytesIO()
        font.save(buf)
        atomic_write(DESKTOP_FONT, buf.getvalue())


def subset_woff2(sfnt: bytes, codepoints: list[int]) -> bytes:
    opts = subset.Options()
    opts.hinting = False
    opts.layout_features = []
    opts.drop_tables += ["GSUB", "GPOS", "GDEF", "meta"]
    with TTFont(io.BytesIO(sfnt), recalcTimestamp=False) as font:
        subsetter = subset.Subsetter(options=opts)
        subsetter.populate(unicodes=codepoints)
        subsetter.subset(font)
        font.flavor = "woff2"
        buf = io.BytesIO()
        font.save(buf)
        return buf.getvalue()


def build(source: Path, output: Path) -> None:
    with TTFont(source, recalcTimestamp=False) as font:
        if font["name"].getDebugName(6) != "MapleMono-CN-Regular":
            raise ValueError("源字体不是 Maple Mono CN Regular")
        advances = {cp: font["hmtx"][glyph][0] for cp, glyph in font.getBestCmap().items()}
        if font["head"].unitsPerEm != 1000 or set(advances.values()) - {0, 600, 1200}:
            raise ValueError("字体度量与 TDF2 码位索引不匹配")
        coverage = set(advances)
        font.flavor = None
        uncompressed = io.BytesIO()
        font.save(uncompressed)
    sfnt = uncompressed.getvalue()
    hot = coverage & (set(range(0x100)) | set(map(ord, UI_CHARACTERS)))
    rest = sorted(coverage - hot)
    # 保留小型常驻片，避免基础数字占用大分片的解码缓冲。
    groups = [sorted(hot)] + [rest[i:i + SHARD_CODEPOINTS] for i in range(0, len(rest), SHARD_CODEPOINTS)]
    if len(groups) > 256:
        raise ValueError("分片 ID 超过 8 位索引容量")
    mapping = [
        (advances[cp] // 600 << 30) | (cp << 8) | shard
        for cp, shard in sorted((cp, shard) for shard, codepoints in enumerate(groups) for cp in codepoints)
    ]
    prefix = 16 + 8 * len(groups)
    image = bytearray(prefix)
    image.extend(struct.pack(f"<{len(mapping)}I", *mapping))
    largest_woff = largest_sfnt = 0
    for shard, codepoints in enumerate(groups):
        payload = subset_woff2(sfnt, codepoints)
        image.extend(bytes(-len(image) % 4))
        struct.pack_into("<II", image, 16 + 8 * shard, len(image), len(payload))
        image.extend(payload)
        largest_woff = max(largest_woff, len(payload))
        largest_sfnt = max(largest_sfnt, struct.unpack_from(">I", payload, 16)[0])
        print(f"分片 {shard:02d}/{len(groups) - 1:02d}：{len(codepoints):4d} 个码位，{len(payload):7,d} 字节", flush=True)
    if len(image) > PARTITION_SIZE:
        raise ValueError(f"镜像 {len(image):,} 字节超出分区 {PARTITION_SIZE:,} 字节")
    struct.pack_into("<4sIII", image, 0, b"TDF2", len(groups), len(mapping), len(image))
    atomic_write(output, image, lambda staging: verify(source, staging))
    print(f"最大分片 {largest_woff:,} 字节，最大解压后 SFNT {largest_sfnt:,} 字节；分区余量 {PARTITION_SIZE - len(image):,} 字节")


def atomic_write(output: Path, data: bytes | bytearray, check=None) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=output.parent, prefix=output.name + ".", delete=False) as temp:
        staging = Path(temp.name)
        temp.write(data)
    try:
        if check:
            check(staging)
        staging.replace(output)
    finally:
        staging.unlink(missing_ok=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--import-ttf", type=Path, help="可选，从官方发布的 Regular TTF 生成桌面 WOFF2")
    parser.add_argument("--output", type=Path, default=ROOT / "firmware/assets/fonts.bin")
    args = parser.parse_args()
    if args.import_ttf:
        import_regular(args.import_ttf)
    build(DESKTOP_FONT, args.output)
