#include "display.h"
#include "ble_display.h"
#include "wifi_network.h"
#include "font.h"
#include "presentation.h"
#include "../assets/brand_logo.h"

#include "driver/ledc.h"
#include "driver/spi_master.h"
#include "esp_heap_caps.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_vendor.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "lvgl.h"
#if __has_include("../assets/device_icons.h")
#include "../assets/device_icons.h"
#define TDISPLAY_HAS_ICONS 1
#endif

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <string>
#include <vector>

namespace {
constexpr int width = 320, height = 170;
using Color = uint32_t;
struct Rect { int x, y, w, h; };
constexpr int screen_inset = 4, status_height = 26;
constexpr Rect status_bounds{screen_inset, height - screen_inset - status_height,
                             width - 2 * screen_inset, status_height};
constexpr int status_center = status_bounds.y + status_bounds.h / 2;
struct Palette { Color background, surface, text; };
struct Scroll {
    std::string text;
    uint64_t start = 0;
    std::vector<std::pair<int, size_t>> offsets;
    int length = 0;
    int size = 0;
    bool bold = false;
};
struct CardState {
    std::array<Scroll, 4> lines;
    std::string state;
    std::string number;
    std::string old_number;
    double from = NAN, target = NAN;
    int roll_direction = 1;
    uint64_t value_start = 0;
    uint64_t alert_start = 0;
    bool alerting = false;
};

esp_lcd_panel_handle_t panel;
bool panel_rotated;
lv_display_t *screen;
lv_obj_t *canvas;
uint16_t *pixels, *previous;
std::array<CardState, 8> cards;
std::string current_page;
Scroll connection_error;
std::vector<std::string> connection_lines;
uint64_t current_revision = UINT64_MAX, current_generation = UINT64_MAX;
uint64_t now;

const cJSON *get(const cJSON *obj, const char *name) { return cJSON_GetObjectItemCaseSensitive(obj, name); }
const char *str(const cJSON *value) { return cJSON_IsString(value) ? value->valuestring : ""; }
const char *str(const cJSON *obj, const char *name) { return str(get(obj, name)); }
double num(const cJSON *obj, const char *key, double fallback = NAN) {
    const auto v = get(obj, key);
    return cJSON_IsNumber(v) ? v->valuedouble : fallback;
}
bool is(const char *a, const char *b) { return strcmp(a, b) == 0; }
Color hex(const cJSON *obj, const char *key, Color fallback) {
    auto s = str(obj, key);
    return s[0] == '#' && strlen(s) == 7 ? strtoul(s + 1, nullptr, 16) : fallback;
}
Palette palette(const DisplayState &state) {
    const auto theme = get(state.config, "theme");
    const auto colors = get(theme, str(theme, "active"));
    return {hex(colors, "background", 0x1e1e1e), hex(colors, "surface", 0x4a4a4a),
            hex(colors, "text", 0xf1f1f1)};
}
const cJSON *active_page(const DisplayState &state) {
    const cJSON *page;
    cJSON_ArrayForEach(page, get(state.config, "pages")) {
        if (is(str(page, "pageId"), state.page)) return page;
    }
    return nullptr;
}
uint16_t rgb565(Color c) {
    return ((c >> 8) & 0xf800) | ((c >> 5) & 0x07e0) | ((c >> 3) & 0x001f);
}
Color mix(Color base, Color ink, int alpha) {
    Color color = 0;
    for (int bit : {0, 8, 16}) {
        color |= (((base >> bit & 255) * (255 - alpha) + (ink >> bit & 255) * alpha + 127) / 255) << bit;
    }
    return color;
}
void box(Rect b, Color color) {
    int x = std::max(0, b.x), end = std::min(width, b.x + b.w);
    if (x >= end) return;
    uint16_t c = rgb565(color);
    for (int y = std::max(0, b.y); y < std::min(height, b.y + b.h); ++y)
        std::fill(pixels + y * width + x, pixels + y * width + end, c);
}
void opacity_pixel(int x, int y, Color ink, int alpha) {
    if (x < 0 || x >= width || y < 0 || y >= height || !alpha) return;
    auto &p = pixels[y * width + x];
    if (alpha >= 255) {
        p = rgb565(ink);
        return;
    }
    Color base = ((p & 0xf800) << 8) | ((p & 0x07e0) << 5) | ((p & 0x001f) << 3);
    base |= (base >> 5) & 0x070007;
    base |= (base >> 6) & 0x000300;
    p = rgb565(mix(base, ink, alpha));
}
int measure(const char *text, int size, bool bold = false) {
    int x = 0;
    while (*text) x += font_advance(font_next(text), size, bold);
    return x;
}
enum class Tracking { normal, title };
bool is_han(uint32_t cp) {
    return cp == 0x3007 || (cp >= 0x3400 && cp <= 0x4dbf) || (cp >= 0x4e00 && cp <= 0x9fff)
        || (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0x20000 && cp <= 0x2fa1f)
        || (cp >= 0x30000 && cp <= 0x3347f);
}
int tracking_between(Tracking tracking, uint32_t current, const char *next) {
    if (tracking == Tracking::normal || !*next) return 0;
    return is_han(current) && is_han(font_next(next)) ? -2 : 0;
}
int draw_text(Rect clip, int x, int top, const char *text, int size, Color color,
              bool bold = false, bool fade_left = false, bool fade_right = false,
              bool fade_top = false, bool fade_bottom = false, Tracking tracking = Tracking::normal,
              int subpixel_x = 0, int subpixel_y = 0) {
    int baseline = top + (size * (size <= 8 ? 82 : 92) + 50) / 100;
    int dx = subpixel_x != 0, dy = subpixel_y != 0;
    int fraction = subpixel_x + subpixel_y;
    while (*text) {
        const auto cp = font_next(text);
        const auto &g = font_glyph(cp, size, bold);
        int gx = x + g.left, gy = baseline - g.top;
        auto coverage = [&](int xx, int yy) {
            return xx >= 0 && xx < g.width && yy >= 0 && yy < g.height
                ? g.opacity[yy * g.width + xx] : 0;
        };
        for (int y = std::max(clip.y, gy - dy); y < std::min(clip.y + clip.h, gy + g.height); ++y) {
            int vertical = 255;
            if (fade_top) vertical = std::min(vertical, (y - clip.y) * 255 / 4);
            if (fade_bottom) vertical = std::min(vertical, (clip.y + clip.h - y - 1) * 255 / 4);
            for (int xx = std::max(clip.x, gx - dx); xx < std::min(clip.x + clip.w, gx + g.width); ++xx) {
                int opacity = vertical;
                if (fade_left) opacity = std::min(opacity, (xx - clip.x) * 255 / 4);
                if (fade_right) opacity = std::min(opacity, (clip.x + clip.w - xx - 1) * 255 / 4);
                int alpha = coverage(xx - gx, y - gy);
                if (fraction)
                    alpha = presentation::blend_coverage(alpha, coverage(xx - gx + dx, y - gy + dy), fraction);
                opacity_pixel(xx, y, color, opacity * alpha / 255);
            }
        }
        x += std::max(0, g.advance + tracking_between(tracking, cp, text));
        if (x - dx >= clip.x + clip.w) break;
    }
    return x;
}
std::string single_line(const char *text) {
    std::string result;
    bool gap = false;
    while (*text) {
        if (*text == '\n' || *text == '\r' || *text == '\t') {
            if (!gap) result += ' ';
            gap = true;
        } else {
            result += *text;
            gap = false;
        }
        ++text;
    }
    return result;
}
void line(Rect bounds, Scroll &scroll, const char *text, int size, Color color,
          bool bold = false, int speed = 32, Tracking tracking = Tracking::normal, bool smooth = false) {
    auto normalized = single_line(text);
    if (scroll.text != normalized || scroll.size != size || scroll.bold != bold) {
        scroll.text = std::move(normalized);
        scroll.size = size;
        scroll.bold = bold;
        scroll.length = 0;
        scroll.offsets.clear();
        auto content = scroll.text.c_str();
        for (auto next = content; *next;) {
            scroll.offsets.emplace_back(scroll.length, next - content);
            const auto cp = font_next(next);
            int advance = font_advance(cp, size, bold);
            scroll.length += std::max(0, advance + tracking_between(tracking, cp, next));
        }
        if (smooth && scroll.length > bounds.w) font_prepare(scroll.text.c_str(), size, bold);
        scroll.start = smooth ? esp_timer_get_time() / 1000 : now;
    }
    int length = scroll.length;
    if (length <= bounds.w) {
        draw_text(bounds, bounds.x, bounds.y, scroll.text.c_str(), size, color,
                  bold, false, false, false, false, tracking);
        return;
    }
    auto elapsed = now >= scroll.start ? now - scroll.start : 0;
    uint64_t travel = elapsed > 800 ? (elapsed - 800) * speed * 256 / 1000 : 0;
    int advance = (travel / 256) % (length + 18);
    int fraction = smooth ? travel % 256 : 0;
    auto paint = [&](int x) {
        auto start = std::lower_bound(scroll.offsets.begin(), scroll.offsets.end(), bounds.x - x - size,
            [](const auto &pos, int target) { return pos.first < target; });
        if (start != scroll.offsets.end()) {
            draw_text(bounds, x + start->first, bounds.y, scroll.text.c_str() + start->second,
                      size, color, bold, elapsed > 800, true, false, false, tracking, fraction);
        }
    };
    paint(bounds.x - advance);
    if (elapsed > 800) paint(bounds.x - advance + length + 18);
}
std::vector<std::string> wrap(const char *text, int pixels_wide, int size) {
    std::vector<std::string> lines(1);
    int used = 0;
    while (*text) {
        const char *start = text;
        auto cp = font_next(text);
        if (cp == '\r') continue;
        if (cp == '\t') cp = ' ';
        auto advance = font_advance(cp, size, false);
        if (cp == '\n' || (used > 0 && used + advance > pixels_wide)) {
            lines.emplace_back();
            used = 0;
            if (cp == '\n') continue;
        }
        if (cp == ' ' && *start == '\t') lines.back() += ' ';
        else lines.back().append(start, text - start);
        used += advance;
    }
    return lines;
}
std::string fit_text(const char *text, int size, int room, bool bold = false) {
    if (measure(text, size, bold) <= room) return text;
    int limit = room - measure("…", size, bold), used = 0;
    auto end = text;
    for (auto cursor = text; *cursor;) {
        used += font_glyph(font_next(cursor), size, bold).advance;
        if (used > limit) break;
        end = cursor;
    }
    return std::string(text, end) + "…";
}
std::string countdown(double deadline, uint64_t wall, const char *expired) {
    if (!std::isfinite(deadline) || !wall) return "—";
    if (deadline <= wall) return *expired ? expired : "待刷新";
    auto seconds = static_cast<unsigned long>(ceil((deadline - wall) / 1000));
    char result[32];
    if (seconds >= 86400) snprintf(result, sizeof(result), "%lud%luh", seconds / 86400, seconds / 3600 % 24);
    else if (seconds >= 3600) snprintf(result, sizeof(result), "%luh%02lum", seconds / 3600, seconds / 60 % 60);
    else if (seconds >= 60) snprintf(result, sizeof(result), "%lum", seconds / 60);
    else snprintf(result, sizeof(result), "%lus", seconds);
    return result;
}
double ease(double x, double x1 = .42) {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    // 与预览的 CSS ease-in-out / ease-out 共用控制点。
    double low = 0, high = 1;
    for (int i = 0; i < 12; ++i) {
        double t = (low + high) / 2, inv = 1 - t;
        double position = 3 * inv * inv * t * x1 + 3 * inv * t * t * .58 + t * t * t;
        if (position < x) low = t;
        else high = t;
    }
    double t = (low + high) / 2;
    return t * t * (3 - 2 * t);
}
double ratio(double value, double low = 0, double high = 1) {
    return std::isfinite(value) && std::isfinite(low) && std::isfinite(high) && high > low ?
        std::clamp((value - low) / (high - low), 0.0, 1.0) : 0;
}
Color contrast(Color c) {
    auto linear = [](double v) { v /= 255; return v <= .04045 ? v / 12.92 : pow((v + .055) / 1.055, 2.4); };
    double l = .2126 * linear((c >> 16) & 255) + .7152 * linear((c >> 8) & 255) + .0722 * linear(c & 255);
    return l > .179 ? 0 : 0xffffff;
}
void badge(Rect b, const std::string &text, Color fg, Color bg, int size, int text_inset = 1) {
    box({b.x, b.y + 1, b.w, b.h - 2}, bg);
    box({b.x + 1, b.y, b.w - 2, 1}, bg);
    box({b.x + 1, b.y + b.h - 1, b.w - 2, 1}, bg);
    auto clipped = fit_text(text.c_str(), size, b.w - 6, true);
    draw_text({b.x + 3, b.y, b.w - 6, b.h}, b.x + 3, b.y + text_inset, clipped.c_str(), size, fg, true);
}
void icon(const char *name, int x, int y, int size, Color color, bool rotate = false) {
#ifdef TDISPLAY_HAS_ICONS
    for (const auto &asset : td_icons) {
        if (asset.size != size || !is(asset.name, name)) continue;
        double angle = rotate ? (now % 1000) * 6.283185307 / 1000 : 0;
        double sine = sin(angle), cosine = cos(angle), center = (size - 1) / 2.0;
        for (int dy = 0; dy < size; ++dy)
            for (int dx = 0; dx < size; ++dx) {
                int sx = lround((dx - center) * cosine + (dy - center) * sine + center);
                int sy = lround(-(dx - center) * sine + (dy - center) * cosine + center);
                if (sx >= 0 && sx < size && sy >= 0 && sy < size)
                    opacity_pixel(x + dx, y + dy, color, asset.alpha[sy * size + sx]);
            }
        return;
    }
#endif
    (void)name;
    (void)rotate;
    box({x + 3, y + size / 2, size - 6, 2}, color);
}
void status_bar(const DisplayState &state, const Palette &p) {
    constexpr int center = status_center;
    constexpr int name_size = 11, time_size = 11, gap = 6;
    box(status_bounds, p.surface);
    int pages = cJSON_GetArraySize(get(state.config, "pages"));
    int page = 0;
    const char *title = "Codex 软件";
    cJSON *entry;
    int i = 0;
    cJSON_ArrayForEach(entry, get(state.config, "pages")) {
        if (is(str(entry, "pageId"), state.page)) {
            page = i;
            title = str(entry, "name");
        }
        ++i;
    }
    int dots = pages ? pages : 1;
    int top = center - (dots * 3 - 1) / 2;
    for (i = 0; i < dots; ++i)
        box({12, top + i * 3, 2, 2}, i == page ? p.text : mix(p.surface, p.text, 71));
    const char *connection = !state.config && *state.error ? "attention"
        : state.connected ? (is(state.transport, "ble") ? "bluetooth" : state.transport) : state.config ? "link-break" : "loading-four";
    int power_x;
    if (!state.power_known) {
        power_x = 308 - measure("—", 12);
        draw_text({power_x, center - 6, 308 - power_x, 12}, power_x, center - 6, "—", 12, p.text);
    } else {
        char percent[16];
        if (state.battery >= 0) snprintf(percent, sizeof(percent), "%d%%", state.battery);
        else snprintf(percent, sizeof(percent), "—");
        bool battery_group = !state.external || state.charging;
        int text_width = battery_group ? measure(percent, 12) + 4 : 0;
        power_x = 308 - 18 - text_width;
        icon(state.charging ? "battery-charge" : state.external ? "plug-one" : "battery-empty",
             power_x, center - 9, 18, p.text);
        if (battery_group) {
            if (state.battery > 0 && !state.charging)
                box({power_x + 3, center - 2, static_cast<int>(lround(10 * state.battery / 100.0)), 4}, p.text);
            draw_text({power_x + 22, center - 6, text_width - 4, 12},
                      power_x + 22, center - 6, percent, 12, p.text);
        }
    }
    int connection_x = power_x - 18;
    icon(connection, connection_x, center - 7, 14, p.text, is(connection, "loading-four"));

    // 预留完整时间宽度，电量到 100% 或页名过长时优先收缩页名。
    constexpr int name_x = 20;
    int name_room = connection_x - 2 * gap - name_x - measure("Updated at 00:00:00", time_size);
    int name_width = std::min({measure(title, name_size, true) + 6, name_room, 86});
    badge({name_x, center - 7, name_width, 14}, title, p.background, p.text, name_size);
    int time_x = name_x + name_width + gap;
    char timestamp[40] = "Updated at —";
    if (state.updated_at) {
        time_t seconds = state.updated_at / 1000 + state.utc_offset * 60;
        struct tm tm{};
        gmtime_r(&seconds, &tm);
        snprintf(timestamp, sizeof(timestamp), "Updated at %02d:%02d:%02d", tm.tm_hour, tm.tm_min, tm.tm_sec);
    }
    draw_text({time_x, center - 6, connection_x - gap - time_x, 12},
              time_x, center - 6, timestamp, time_size, p.text);
}
void connection_panel(const DisplayState &state, const Palette &p) {
    box({4, 4, 312, 132}, p.surface);
    if (*state.error) {
        icon("attention", 12, 14, 16, 0xd63f41);
        draw_text({34, 12, 274, 20}, 34, 12, "连接失败", 20, p.text);
        if (connection_error.text != state.error) {
            connection_error.text = state.error;
            connection_error.start = now;
            connection_lines = wrap(state.error, 296, 14);
        }
        int over = std::max(0, static_cast<int>(connection_lines.size()) * 14 - 92);
        int travel = over * 50;
        int elapsed = over ? (now - connection_error.start) % (1600 + travel * 2) : 0;
        int offset = elapsed < 800 ? 0 : elapsed < 800 + travel ? (elapsed - 800) / 50 :
                     elapsed < 1600 + travel ? over : over - (elapsed - 1600 - travel) / 50;
        Rect clip{12, 36, 296, 92};
        size_t first = offset / 14;
        for (size_t i = first; i < connection_lines.size() && i <= first + 7; ++i)
            draw_text(clip, 12, clip.y + static_cast<int>(i) * 14 - offset, connection_lines[i].c_str(), 14,
                      p.text, false, false, false, offset > 0, offset < over);
        return;
    }
    connection_error.text.clear();
    const char *text = state.connected ? "等待同步" : "等待连接";
    draw_text({4, 4, 312, 132}, (320 - measure(text, 20)) / 2, 60, text, 20, p.text);
}
void settings_panel(const DisplayState &state, const Palette &p, const settings::Menu &menu,
                    unsigned brightness, bool rotated) {
    using settings::View;
    box({4, 4, 312, 132}, p.surface);
    box(status_bounds, p.surface);
    const auto heading = menu.view == View::menu ? "设备设置"
                       : menu.view == View::brightness ? "屏幕亮度"
                       : menu.view == View::bluetooth ? "蓝牙配对"
                       : menu.view == View::wifi ? "Wi-Fi" : "设备信息";
    draw_text({12, 12, 296, 22}, 12, 12, heading, 20, p.text);
    auto right = [&](int y, const char *value, int size, Color color, bool bold = false) {
        draw_text({12, y, 288, size + 2}, 300 - measure(value, size, bold), y, value, size, color, bold);
    };
    auto footer = [&](int y, const char *label) {
        draw_text({12, y, 296, 12}, (width - measure(label, 11)) / 2, y, label, 11, p.text);
    };
    if (menu.view == View::menu) {
        const char *items[] = {"屏幕亮度", "屏幕反转", "设备信息", "蓝牙配对", "Wi-Fi", "返回仪表盘"};
        char percent[16];
        snprintf(percent, sizeof(percent), "%u%%", brightness);
        const char *values[] = {percent, rotated ? "开启" : "关闭", ">", ">", ">", ""};
        for (unsigned i = 0; i < 6; ++i) {
            const int y = 32 + 17 * i;
            const bool selected = menu.selected == i;
            Color ink = selected ? p.background : p.text;
            box({12, y, 296, 16}, selected ? p.text : p.background);
            draw_text({20, y + 1, 220, 15}, 20, y + 1, items[i], 13, ink);
            right(y + 3, values[i], 13, ink);
        }
        footer(status_center - 12, "用户键 选择    BOOT 确认");
        footer(status_center, menu.save_failed ? "保存失败，短按 BOOT 重试" : "长按 BOOT 退出");
    } else if (menu.view == View::brightness) {
        char percent[16];
        snprintf(percent, sizeof(percent), "%u%%", menu.brightness);
        right(43, percent, 32, p.text, true);
        box({12, 85, 296, 6}, mix(p.surface, p.text, 71));
        box({12, 85, static_cast<int>(lround(296 * menu.brightness / 100.0)), 6}, p.text);
        draw_text({12, 97, 56, 14}, 12, 97, "10%", 11, p.text);
        right(97, "100%", 11, p.text);
        footer(status_center - 12, "用户键 -10%    BOOT +10%");
        footer(status_center, menu.save_failed ? "保存失败，长按 BOOT 重试" : "长按 BOOT 保存返回");
    } else if (menu.view == View::bluetooth) {
        int code = ble_pair_code();
        if (menu.forget_selected) {
            draw_text({12, 48, 296, 24}, 12, 48, "清除已配对电脑？", 20, p.text);
            footer(status_center - 12, "用户键 取消    BOOT 确认");
        } else if (code >= 0) {
            char passkey[12];
            snprintf(passkey, sizeof(passkey), "%06d", code);
            right(43, passkey, 32, p.text, true);
            draw_text({12, 85, 296, 18}, 12, 85, "在 Mado Studio 连接并输入配对码", 14, p.text);
            footer(status_center - 12, "用户键 忘记电脑");
        } else {
            draw_text({12, 48, 296, 24}, 12, 48, ble_display_connected() ? "蓝牙已配对" : "配对窗口已关闭", 20, p.text);
            footer(status_center - 12, "用户键 忘记电脑    BOOT 开始配对");
        }
        footer(status_center, "长按 BOOT 返回");
    } else if (menu.view == View::wifi) {
        const auto ip = wifi::address();
        if (menu.forget_selected) {
            draw_text({12, 48, 296, 24}, 12, 48, "忘记已保存的网络？", 20, p.text);
            footer(status_center - 12, "用户键 取消    BOOT 确认");
        } else if (!ip.empty()) {
            draw_text({12, 43, 296, 20}, 12, 43, "设备 IP", 14, p.text);
            draw_text({12, 70, 296, 28}, 12, 70, ip.c_str(), 22, p.text);
            footer(status_center - 12, "用户键 忘记网络");
        } else if (wifi::has_saved_network()) {
            draw_text({12, 48, 296, 24}, 12, 48, "等待接入网络", 20, p.text);
            footer(status_center - 12, menu.save_failed ? "暂时无法忘记网络" : "用户键 忘记网络");
        } else {
            draw_text({12, 48, 296, 24}, 12, 48, "在 Studio 配置 Wi-Fi", 18, p.text);
            footer(status_center - 12, "通过蓝牙或 USB 连接");
        }
        footer(status_center, "长按 BOOT 返回");
    } else {
        char version[24], battery_text[16] = "—";
        snprintf(version, sizeof(version), "v%s", firmware_version);
        if (state.battery >= 0) snprintf(battery_text, sizeof(battery_text), "%d%%", state.battery);
        const char *labels[] = {"型号", "固件", "连接", "电池"};
        const char *values[] = {"T-Display-C5 V1.0", version, state.connected ? (is(state.transport, "ble") ? "蓝牙已连接" : is(state.transport, "wifi") ? "Wi-Fi 已连接" : "USB 已连接") : "未连接", battery_text};
        for (int i = 0; i < 4; ++i) {
            int y = 42 + i * 20;
            draw_text({12, y, 88, 16}, 12, y, labels[i], 13, p.text);
            right(y, values[i], 13, p.text);
        }
        footer(status_center - 6, "BOOT 返回");
    }
}
void chart(Rect b, const cJSON *render, const cJSON *reading, const Palette &p, Color ink) {
    auto style = get(render, "chart");
    auto samples = get(reading, "values");
    int n = cJSON_GetArraySize(samples);
    double low = num(style, "min"), high = num(style, "max");
    for (int i = 0; i < n; ++i) {
        const auto value = cJSON_GetArrayItem(samples, i);
        if (!cJSON_IsNumber(value)) continue;
        int blocks = lround(ratio(value->valuedouble, low, high) * 7);
        int plot_width = n * 5 - 1;
        int left = lround(i * 5 * (b.w - 16) / static_cast<double>(plot_width));
        int right = lround((i * 5 + 4) * (b.w - 16) / static_cast<double>(plot_width));
        for (int j = 0; j < blocks; ++j)
            box({b.x + 8 + left, b.y + b.h - 11 - j * 4, right - left, 3}, ink);
    }
    auto last = cJSON_GetArrayItem(samples, n - 1);
    auto text = presentation::compact_metric(cJSON_IsNumber(last) ? last->valuedouble : NAN);
    if (cJSON_IsNumber(last)) text += str(render, "unit");
    // 最新读数覆盖在柱条上，柱条照常使用整块绘图区。
    badge({b.x + 8, b.y + b.h - 34, std::min(b.w - 16, measure(text.c_str(), 8, true) + 6), 10},
          text, p.background, ink, 8, 2);
}
double current_value(const CardState &s) {
    return presentation::counted_value(s.from, s.target, now - s.value_start);
}
void draw_number(Rect clip, const std::string &text, const CardState &card, int size,
                 Color ink, bool roll) {
    int inset = (clip.h - size) / 2;
    if (!roll || now - card.value_start >= 300 ||
        text.find_first_not_of("0123456789-.kmbte") != std::string::npos ||
        card.old_number.find_first_not_of("0123456789-.kmbte") != std::string::npos) {
        draw_text(clip, clip.x, clip.y + inset, text.c_str(), size, ink, true);
        return;
    }
    double progress = ease((now - card.value_start) / 300.0, 0);
    auto alignment = static_cast<std::ptrdiff_t>(card.old_number.size()) - static_cast<std::ptrdiff_t>(text.size());
    int x = clip.x;
    for (size_t i = 0; i < text.size(); ++i) {
        char digit[] = {text[i], '\0'};
        auto previous = alignment + static_cast<std::ptrdiff_t>(i);
        char old = previous >= 0 ? card.old_number[previous] : ' ';
        int w = measure(digit, size, true);
        if (digit[0] != old && digit[0] >= '0' && digit[0] <= '9' && old >= '0' && old <= '9') {
            int distance = presentation::roll_distance(old - '0', digit[0] - '0', card.roll_direction);
            int travel = lround(clip.h * distance * progress * 256);
            int shift = travel / 256;
            int first = shift / clip.h;
            for (int row = first; row <= std::min(first + 1, distance); ++row) {
                char glyph[] = {presentation::roll_digit(old - '0', row, card.roll_direction), '\0'};
                draw_text(clip, x, clip.y + inset + row * clip.h - shift, glyph, size, ink,
                          true, false, false, false, false, Tracking::normal, 0, travel % 256);
            }
        } else draw_text(clip, x, clip.y + inset, digit, size, ink, true);
        x += w;
    }
}
void card(Rect b, CardState &local, const cJSON *render, const cJSON *reading,
          const DisplayState &state, const Palette &p, bool fresh) {
    auto kind = str(render, "kind");
    bool status = is(kind, "state");
    auto state_key = str(reading, "state");
    auto style = get(get(render, "stateStyles"), state_key);
    auto label = str(render, "label");
    if (status) {
        auto translated = str(get(render, "stateLabels"), state_key);
        if (*translated) label = translated;
        if (local.state != state_key) {
            local.state = state_key;
            local.alerting = false;
        }
        if (fresh && !state.snapshot && cJSON_IsTrue(get(reading, "notify"))) {
            local.alert_start = now;
            local.alerting = true;
        }
    }
    Color accent = hex(style, "color", hex(render, "accentColor", p.text));
    Color surface = p.surface, ink = p.text;
    if (local.alerting) {
        auto t = now - local.alert_start;
        if (t >= num(style, "alertDurationMs", 2000)) local.alerting = false;
        else {
            t %= 1000;
            auto bg = t < 420 ? ease(t / 420.0) : 1 - ease((t - 420.0) / 580);
            auto fg = t < 420 ? ease((t - 120.0) / 300) : 1 - ease((t - 540.0) / 460);
            surface = mix(surface, accent, lround(bg * 255));
            ink = mix(ink, hex(style, "alertText", contrast(accent)), lround(fg * 255));
        }
    }
    box(b, surface);
    if (is(kind, "progress")) {
        box({b.x, b.y, b.w, 4}, mix(surface, p.text, 71));
        box({b.x, b.y, static_cast<int>(lround(b.w * ratio(num(reading, "ratio")))), 4}, accent);
    } else box({b.x, b.y, b.w, 4}, accent);
    bool tall = b.h > 64;
    auto range = get(render, "range");
    auto limits = is(str(range, "mode"), "reading") ? get(reading, "bounds") : range;
    double low = num(limits, "min"), high = num(limits, "max");
    bool has_bounds = is(kind, "range") && std::isfinite(low) && std::isfinite(high) && low < high;
    bool short_range = has_bounds && !tall;
    line({b.x + 8, b.y + (short_range ? 10 : 12), b.w - 13, 16}, local.lines[0], label, 14, ink, false, 16, Tracking::title);
    std::array<const cJSON *, 2> fields{};
    std::array<const char *, 2> values{};
    size_t detail_count = 0;
    if (tall) {
        cJSON *detail;
        cJSON_ArrayForEach(detail, get(render, "detailFields")) {
            const char *value = str(get(reading, "details"), str(detail, "key"));
            if (!*value) continue;
            fields[detail_count] = detail;
            values[detail_count++] = value;
            if (detail_count == values.size()) break;
        }
    }
    bool large_value = tall && (is(kind, "metric") || is(kind, "progress"));
    int row_height = large_value ? 32 : 20;
    int top = b.y + (tall ? (detail_count ? 35 : 75 - row_height / 2) : 36);
    if (status) {
        auto title = str(reading, "detail");
        if (!*title) title = "—";
        line({b.x + 8, top, b.w - 16, 20}, local.lines[1], title, 20, ink, true, 32, Tracking::normal, true);
    } else if (is(kind, "chart")) {
        chart(b, render, reading, p, accent);
    } else {
        bool trend = is(kind, "trend");
        bool is_countdown = is(str(render, "format"), "countdown");
        auto anim = str(render, "numberAnimation");
        auto value = num(reading, "value");
        bool changed = (std::isfinite(value) != std::isfinite(local.target)) ||
                       (std::isfinite(value) && value != local.target);
        if (changed || (fresh && state.snapshot)) {
            auto previous_value = current_value(local);
            local.from = fresh && !state.snapshot && is(anim, "count") ? previous_value : value;
            local.roll_direction = value < local.target ? -1 : 1;
            local.target = value;
            local.old_number = fresh && !state.snapshot ? local.number : "";
            local.value_start = now;
        }
        auto expired = str(render, "expiredText");
        if (!*expired) expired = "待刷新";
        auto text = is_countdown ? countdown(num(reading, "deadlineUnixMs"), state.wall_time, expired) :
                    presentation::compact_metric(is(anim, "count") ? current_value(local) : value);
        local.number = text;
        auto characters = std::count_if(text.begin(), text.end(), [](uint8_t byte) { return (byte & 0xc0) != 0x80; });
        bool compact_text = characters >= 4 || (is_countdown && text == expired);
        int size = large_value ? (is_countdown && compact_text ? 26 : 32) : (compact_text ? 17 : 20);
        if (short_range) top = b.y + 30;
        int x = b.x + 8;
        int edge = b.x + b.w - 8;
        if (trend && std::isfinite(num(reading, "delta"))) {
            auto delta = num(reading, "delta");
            auto delta_text = presentation::compact_metric(std::abs(delta));
            int direction = delta_text == "0" ? 0 : delta < 0 ? -1 : 1;
            auto tag = std::string(direction > 0 ? "↑" : direction < 0 ? "↓" : "→") + delta_text;
            auto unit = str(get(render, "trend"), "deltaUnit");
            if (!*unit) unit = str(render, "unit");
            tag += is(unit, "pp") ? "%" : unit;
            int tag_width = std::min((b.w - 16) / 2, measure(tag.c_str(), 8, true) + 6);
            edge -= tag_width;
            badge({edge, top + row_height - 11, tag_width, 11}, tag,
                  direction > 0 ? 0x1e1e1e : direction < 0 ? 0xffffff : p.background,
                  direction > 0 ? 0x48b34a : direction < 0 ? 0xd63f41 : ink, 8);
            edge -= 2;
        }
        auto unit = std::isfinite(value) ? str(render, "unit") : "";
        constexpr int unit_size = 11, max_unit_width = 22;
        auto rows = wrap(unit, max_unit_width, unit_size);
        int unit_width = 0;
        for (const auto &row : rows) unit_width = std::max(unit_width, std::min(max_unit_width, measure(row.c_str(), unit_size)));
        int available = std::max(0, edge - x - unit_width - (unit_width ? 2 : 0));
        if (is_countdown) text = fit_text(text.c_str(), size, available, true);
        else if (unit_width) while (size > 15 && measure(text.c_str(), size, true) > available) --size;
        int value_width = std::min(available, measure(text.c_str(), size, true));
        draw_number({x, top, value_width, row_height}, text, local, size, ink,
                    is(anim, "digitRoll") && !is_countdown);
        int unit_x = x + value_width + 2;
        if (unit_width) {
            if (rows.size() > 2) {
                rows.resize(2);
                auto &last = rows[1];
                while (!last.empty() && measure((last + "…").c_str(), unit_size) > unit_width) {
                    size_t at = last.size() - 1;
                    while (at && (static_cast<uint8_t>(last[at]) & 0xc0) == 0x80) --at;
                    last.resize(at);
                }
                last += "…";
            }
            for (size_t i = 0; i < rows.size(); ++i) {
                int y = top + row_height - unit_size * static_cast<int>(rows.size() - i);
                draw_text({unit_x, y, std::max(0, std::min(unit_width, edge - unit_x)), unit_size},
                          unit_x, y, rows[i].c_str(), unit_size, ink);
            }
        }
        if (has_bounds) {
            int y = b.y + b.h - 14, w = b.w - 16;
            box({x, y + 3, w, 1}, mix(surface, ink, 140));
            for (int i = 0; i < 3; ++i) box({x + (w - 1) * i / 2, y + 1, 1, 4}, ink);
            if (std::isfinite(value))
                box({x + static_cast<int>(lround((w - 3) * ratio(value, low, high))), y, 3, 6}, accent);
        }
    }
    for (size_t i = 0; i < detail_count; ++i) {
        std::string content = std::string(str(fields[i], "label")) + " " + values[i];
        line({b.x + 8, b.y + 84 + static_cast<int>(i) * 16, b.w - 16, 16}, local.lines[i + 2],
             content.c_str(), 12, ink);
    }
}
bool transfer_complete(esp_lcd_panel_io_handle_t, esp_lcd_panel_io_event_data_t *, void *) {
    lv_display_flush_ready(screen);
    return false;
}
void flush(lv_display_t *display, const lv_area_t *a, uint8_t *rgb) {
    lv_draw_sw_rgb565_swap(rgb, lv_area_get_width(a) * lv_area_get_height(a));
    if (esp_lcd_panel_draw_bitmap(panel, a->x1, a->y1, a->x2 + 1, a->y2 + 1, rgb) != ESP_OK)
        lv_display_flush_ready(display);
}
uint32_t tick() { return esp_timer_get_time() / 1000; }

void set_panel_orientation(bool rotated) {
    // 屏幕使用 240 列显存的中央 170 列，两个方向的 35 px 偏移相同。
    ESP_ERROR_CHECK(esp_lcd_panel_mirror(panel, !rotated, rotated));
    panel_rotated = rotated;
}

void present(bool full = false) {
    if (full) {
        memcpy(previous, pixels, width * height * sizeof(*pixels));
        lv_obj_invalidate(canvas);
        lv_refr_now(screen);
        return;
    }
    // 一组连续脏行合成一个矩形，让同一条滚动文字尽量在一次 SPI 传输中更新。
    int start = -1, left = width, right = 0;
    bool dirty = false;
    for (int y = 0; y <= height; ++y) {
        if (y < height) {
            auto *source = pixels + y * width;
            auto *target = previous + y * width;
            if (memcmp(source, target, width * sizeof(uint16_t)) != 0) {
                int first = 0, last = width - 1;
                while (source[first] == target[first]) ++first;
                while (source[last] == target[last]) --last;
                memcpy(target + first, source + first, (last - first + 1) * sizeof(uint16_t));
                if (start < 0) {
                    start = y;
                    left = first;
                    right = last;
                } else {
                    left = std::min(left, first);
                    right = std::max(right, last);
                }
                continue;
            }
        }
        if (start >= 0) {
            lv_area_t area{left & ~1, start, right | 1, y - 1};
            lv_obj_invalidate_area(canvas, &area);
            dirty = true;
            start = -1;
        }
    }
    // 应用负责帧节拍，LVGL 在同一轮提交已合成的画面。
    if (dirty) lv_refr_now(screen);
}
}

void display_brightness(unsigned level) {
    ESP_ERROR_CHECK(ledc_set_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0, level * 255 / 100));
    ESP_ERROR_CHECK(ledc_update_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0));
}

void display_init(unsigned brightness, bool rotated) {
    spi_bus_config_t spi{};
    spi.mosi_io_num = 9;
    spi.miso_io_num = -1;
    spi.sclk_io_num = 7;
    spi.quadwp_io_num = -1;
    spi.quadhd_io_num = -1;
    spi.max_transfer_sz = width * 20 * 2;
    ESP_ERROR_CHECK(spi_bus_initialize(SPI2_HOST, &spi, SPI_DMA_CH_AUTO));
    esp_lcd_panel_io_spi_config_t io{};
    io.cs_gpio_num = 26;
    io.dc_gpio_num = 8;
    io.pclk_hz = 40000000;
    io.trans_queue_depth = 2;
    io.lcd_cmd_bits = 8;
    io.lcd_param_bits = 8;
    io.on_color_trans_done = transfer_complete;
    esp_lcd_panel_io_handle_t handle{};
    ESP_ERROR_CHECK(esp_lcd_new_panel_io_spi(SPI2_HOST, &io, &handle));
    esp_lcd_panel_dev_config_t dev{};
    dev.reset_gpio_num = 23;
    // Match the effective MADCTL setting in the LILYGO factory driver.
    dev.rgb_ele_order = LCD_RGB_ELEMENT_ORDER_RGB;
    dev.bits_per_pixel = 16;
    ESP_ERROR_CHECK(esp_lcd_new_panel_st7789(handle, &dev, &panel));
    ESP_ERROR_CHECK(esp_lcd_panel_reset(panel));
    ESP_ERROR_CHECK(esp_lcd_panel_init(panel));
    set_panel_orientation(rotated);
    ESP_ERROR_CHECK(esp_lcd_panel_swap_xy(panel, true));
    ESP_ERROR_CHECK(esp_lcd_panel_invert_color(panel, true));
    ESP_ERROR_CHECK(esp_lcd_panel_set_gap(panel, 0, 35));
    ESP_ERROR_CHECK(esp_lcd_panel_disp_on_off(panel, true));

    ledc_timer_config_t timer{};
    timer.speed_mode = LEDC_LOW_SPEED_MODE;
    timer.duty_resolution = LEDC_TIMER_8_BIT;
    timer.timer_num = LEDC_TIMER_0;
    timer.freq_hz = 5000;
    timer.clk_cfg = LEDC_AUTO_CLK;
    ESP_ERROR_CHECK(ledc_timer_config(&timer));
    ledc_channel_config_t channel{};
    channel.gpio_num = 25;
    channel.speed_mode = LEDC_LOW_SPEED_MODE;
    channel.channel = LEDC_CHANNEL_0;
    channel.timer_sel = LEDC_TIMER_0;
    channel.intr_type = LEDC_INTR_DISABLE;
    ESP_ERROR_CHECK(ledc_channel_config(&channel));

    lv_init();
    lv_tick_set_cb(tick);
    screen = lv_display_create(width, height);
    lv_display_set_color_format(screen, LV_COLOR_FORMAT_RGB565);
    lv_display_set_flush_cb(screen, flush);
    constexpr size_t transfer = width * 20 * sizeof(uint16_t);
    auto a = heap_caps_malloc(transfer, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    auto b = heap_caps_malloc(transfer, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    pixels = static_cast<uint16_t *>(heap_caps_calloc(width * height, sizeof(uint16_t), MALLOC_CAP_SPIRAM));
    previous = static_cast<uint16_t *>(heap_caps_calloc(width * height, sizeof(uint16_t), MALLOC_CAP_SPIRAM));
    if (!a || !b || !pixels || !previous) abort();
    lv_display_set_buffers(screen, a, b, transfer, LV_DISPLAY_RENDER_MODE_PARTIAL);
    canvas = lv_canvas_create(lv_screen_active());
    lv_obj_remove_flag(canvas, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_pos(canvas, 0, 0);
    lv_canvas_set_buffer(canvas, previous, width, height, LV_COLOR_FORMAT_RGB565);
    if (!font_init()) ESP_LOGW("display", "未加载 CN 字库");
    display_brightness(brightness);
    ESP_LOGI("display", "ST7789 ready; PSRAM free: %u", static_cast<unsigned>(heap_caps_get_free_size(MALLOC_CAP_SPIRAM)));
}

void display_startup(const DisplayState &state) {
    auto p = palette(state);
    const auto *logo = is(str(get(state.config, "theme"), "active"), "light")
        ? td_brand_light : td_brand_dark;
    constexpr int gap = 12, text_gap = 3, loading_gap = 24;
    constexpr int text_width = std::max(td_wordmark_width, td_slogan_width);
    constexpr int text_height = td_wordmark_height + text_gap + td_slogan_height;
    constexpr int brand_width = td_brand_size + gap + text_width;
    constexpr int left = (width - brand_width) / 2;
    constexpr int text_left = left + td_brand_size + gap;
    constexpr int content_height = td_brand_size + loading_gap + td_loading_height;
    constexpr int top = (height - content_height) / 2;
    constexpr int text_top = top + (td_brand_size - text_height) / 2;
    constexpr int slogan_top = text_top + td_wordmark_height + text_gap;
    constexpr int loading_top = top + td_brand_size + loading_gap;
    static_assert(brand_width <= width);
    static_assert(text_height <= td_brand_size);
    static_assert(content_height <= height);
    static_assert(td_loading_width <= width && td_slogan_width <= width);
    box({0, 0, width, height}, p.background);
    for (int y = 0; y < td_brand_size; ++y) {
        for (int x = 0; x < td_brand_size; ++x) {
            auto rgba = logo[y * td_brand_size + x];
            opacity_pixel(left + x,
                          top + y, rgba >> 8, rgba & 255);
        }
    }
    for (int y = 0; y < td_wordmark_height; ++y) {
        for (int x = 0; x < td_wordmark_width; ++x) {
            opacity_pixel(text_left + x, text_top + y, p.text,
                          td_wordmark_alpha[y * td_wordmark_width + x]);
        }
    }
    const auto slogan_color = mix(p.background, p.text, 190);
    for (int y = 0; y < td_slogan_height; ++y) {
        for (int x = 0; x < td_slogan_width; ++x) {
            opacity_pixel(text_left + x, slogan_top + y,
                          slogan_color, td_slogan_alpha[y * td_slogan_width + x]);
        }
    }
    const auto loading_color = mix(p.background, p.text, 210);
    for (int y = 0; y < td_loading_height; ++y) {
        for (int x = 0; x < td_loading_width; ++x) {
            opacity_pixel((width - td_loading_width) / 2 + x,
                          loading_top + y, loading_color,
                          td_loading_alpha[y * td_loading_width + x]);
        }
    }
    present();
}

void display_prepare(const DisplayState &state) {
    constexpr auto number = "0123456789.-%kmbte—";
    for (int size : {8, 15, 16, 17, 18, 19, 20, 26, 32}) font_prepare(number, size, true);
    font_prepare("Updated at 0123456789:—", 11, false);
    font_prepare("0123456789%—", 12, false);
    font_prepare("等待连接同步失败", 20, false);
    std::string units, chart_units;
    const cJSON *item;
    cJSON_ArrayForEach(item, get(active_page(state), "cards")) {
        auto render = get(item, "render");
        auto &text = is(str(render, "kind"), "chart") ? chart_units : units;
        text += str(render, "unit");
    }
    font_prepare(units.c_str(), 11, false);
    font_prepare(chart_units.c_str(), 8, true);
}

void display_draw(const DisplayState &state, const settings::Menu &menu, unsigned brightness, bool rotated) {
    now = esp_timer_get_time() / 1000;
    auto p = palette(state);
    box({0, 0, width, height}, p.background);
    if (current_page != state.page || current_revision != state.revision) {
        cards = {};
        current_page = state.page;
        current_revision = state.revision;
    }
    bool fresh = current_generation != state.reading_generation;
    current_generation = state.reading_generation;
    if (menu.active()) settings_panel(state, p, menu, brightness, rotated);
    else if (!state.config) connection_panel(state, p);
    else {
        for (int y = 0; y < 2; ++y)
            for (int x = 0; x < 4; ++x) box({4 + x * 79, 4 + y * 68, 75, 64}, p.surface);
        cJSON *item;
        size_t i = 0;
        cJSON_ArrayForEach(item, get(active_page(state), "cards")) {
            if (i == cards.size()) break;
            Rect b{4 + static_cast<int>(num(item, "x", 0)) * 79,
                   4 + static_cast<int>(num(item, "y", 0)) * 68,
                   static_cast<int>(num(item, "w", 1)) * 79 - 4,
                   static_cast<int>(num(item, "h", 1)) * 68 - 4};
            card(b, cards[i++], get(item, "render"), get(state.readings, str(item, "cardId")), state, p, fresh);
        }
    }
    if (!menu.active()) status_bar(state, p);

    const bool orientation_changed = panel_rotated != rotated;
    if (orientation_changed) set_panel_orientation(rotated);
    present(orientation_changed);
}

void display_tick() { lv_timer_handler(); }
