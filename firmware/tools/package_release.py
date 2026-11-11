#!/usr/bin/env python3
"""将 ESP-IDF 构建产物打包为 Mado Studio 可直接读取的烧录包。"""

import argparse
import hashlib
import json
import re
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[2]
FLASH_IMAGES = {
    0x2000: "bootloader.bin",
    0x8000: "partition-table.bin",
    0x10000: "mado.bin",
}
FONTS_OFFSET = 0x990000


def package_release(build_dir: Path, tag: str, output: Path) -> None:
    version = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())["version"]
    firmware_version = re.search(
        r'firmware_version\[\] = "([^"]+)"', (ROOT / "firmware/main/display.h").read_text()
    )
    if tag != f"v{version}" or firmware_version is None or firmware_version[1] != version:
        raise ValueError("Release 标签、Studio 和设备固件的版本必须一致")

    flash_args = json.loads((build_dir / "flasher_args.json").read_text())
    flash_files = {
        int(address, 0): build_dir / path for address, path in flash_args["flash_files"].items()
    }
    if flash_args["extra_esptool_args"]["chip"] != "esp32c5" or flash_files.keys() != FLASH_IMAGES.keys():
        raise ValueError("构建产物与 T-Display-C5 V1.0 烧录布局不匹配")
    images = {
        address: (name, flash_files[address].read_bytes()) for address, name in FLASH_IMAGES.items()
    }
    images[FONTS_OFFSET] = ("fonts.bin", (ROOT / "firmware/assets/fonts.bin").read_bytes())
    for name, data in images.values():
        if not data or len(data) % 4:
            raise ValueError(f"{name} 为空或未按 4 字节对齐")

    output.mkdir(parents=True, exist_ok=True)
    for mode, offsets in (("install", images.keys()), ("upgrade", (0x10000,))):
        manifest = {
            "schemaVersion": 1,
            "board": "LILYGO T-Display-C5 V1.0",
            "chip": "esp32c5",
            "version": version,
            "mode": mode,
            "images": [],
        }
        path = output / f"mado-firmware-{tag}-tdisplay-c5-v1.0-{mode}.zip"
        with ZipFile(path, "w", compression=ZIP_DEFLATED) as archive:
            for offset in offsets:
                name, data = images[offset]
                manifest["images"].append({
                    "file": name, "offset": offset, "sha256": hashlib.sha256(data).hexdigest(),
                })
                archive.writestr(name, data)
            archive.writestr("manifest.json", json.dumps(manifest, indent=2) + "\n")
        print(path)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build_dir", type=Path)
    parser.add_argument("tag")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    package_release(args.build_dir, args.tag, args.output)
