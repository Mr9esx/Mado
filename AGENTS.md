# TDisplay

## 硬件规格

本项目使用 **LILYGO T-Display-C5 V1.0** 开发板，板卡版本以背面丝印为准。

| 项目 | 规格 |
| --- | --- |
| 主控 | Espressif ESP32-C5，32 位 RISC-V，最高 240 MHz，带 ULP RISC-V 低功耗核 |
| 存储 | 16 MB Flash + 8 MB PSRAM，均为 Quad QIO |
| 无线 | 2.4 GHz / 5 GHz 双频 Wi-Fi 6，BLE 5 |
| 显示 | 1.9 英寸 ST7789 IPS LCD，170 × 320 像素，RGB565，4 线 SPI，3.3 V |
| 触控 | 无触控 |
| 电池管理 | AXP2602 电池管理单元（BMU） |
| USB | USB-C，支持 USB Serial/JTAG，供电规格 5 V / 500 mA |
| 外形尺寸 | 62 × 26 × 10 mm |
| 支持的开发环境 | Arduino IDE、ESP-IDF |

### 板载外设引脚

| 外设 | GPIO 映射 |
| --- | --- |
| ST7789 | CS `GPIO26`，SCK `GPIO7`，MOSI `GPIO9`，DC `GPIO8`，RST `GPIO23`，背光 PWM `GPIO25` |
| I²C | SDA `GPIO2`，SCL `GPIO3`，连接 AXP2602 |
| AXP2602 | INT `GPIO10` |
| 按键 | 用户键 `GPIO0`，BOOT `GPIO28` |

商品截图对 LCD 控制器的标注不一致，驱动配置以 LILYGO 官方文档和示例中的 **ST7789** 为准。

规格和引脚见 [LILYGO 硬件文档及原理图](https://wiki.lilygo.cc/products/t-display-series/t-display-c5/)。

## 厂商参考工程

LILYGO 的 T-Display-C5 仓库是板级参考工程，包含可编译的示例、可复用的驱动和板卡定义。根目录的 PlatformIO 配置默认用 Arduino 框架编译 `factory` 示例。 [官方仓库](https://github.com/Xinyuan-LilyGO/T-Display-C5/tree/master) · [PlatformIO 配置](https://github.com/Xinyuan-LilyGO/T-Display-C5/blob/master/platformio.ini)

- **板卡配置** `board/Lilygo-T-Display-C5.json` 定义 ESP32-C5、16 MB Flash 和 8 MB PSRAM 的构建参数。`include/board_config.h` 定义板载引脚，默认关闭 CST816S 触控，与本项目的无触控板一致。 [板卡定义](https://github.com/Xinyuan-LilyGO/T-Display-C5/blob/master/board/Lilygo-T-Display-C5.json) · [引脚配置](https://github.com/Xinyuan-LilyGO/T-Display-C5/blob/master/include/board_config.h)
- **驱动和示例** `lib/` 提供 ST7789、AXP2602、按键库和 LVGL 9.2.0。`examples/factory/` 演示屏幕初始化、背光、LVGL 仪表盘、电量读取、Wi-Fi、NTP、按键和深睡眠。独立的 `voltameter`、`lvgl`、`spi_test` 和 `wifi_sat` 示例可按需参考。 [库和示例清单](https://github.com/Xinyuan-LilyGO/T-Display-C5) · [Factory 源码](https://github.com/Xinyuan-LilyGO/T-Display-C5/blob/master/examples/factory/factory.ino)
- **V1.0 显示适配** 初始化 ST7789 时参考 `factory` 和 `board_config.h`，使用 `CS = GPIO26`、`RST = GPIO23`。单独的 `lcd` 示例将这两根线硬编码为 `GPIO3`、`GPIO0`，与本项目板卡的引脚表不一致。 [LCD 示例源码](https://github.com/Xinyuan-LilyGO/T-Display-C5/blob/master/examples/lcd/lcd.ino)
- **预编译固件** 厂商仓库中的固件目录标为 V1.1，其二进制与本项目 V1.0 板卡的兼容性尚未确认。 [厂商固件目录](https://github.com/Xinyuan-LilyGO/T-Display-C5/tree/master/firmware/T-Display-C5-V1.1)
