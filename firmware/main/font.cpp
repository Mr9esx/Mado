#include "font.h"

#include "esp_partition.h"
#include "esp_log.h"
#include "ft2build.h"
#include FT_FREETYPE_H
#include FT_OUTLINE_H
#include "lvgl.h"

#include <algorithm>
#include <array>
#include <cstring>
#include <unordered_map>

namespace {
FT_Library library;
const uint8_t *data;
const uint32_t *directory;
const uint32_t *index_words;
uint32_t shard_count, codepoint_count, data_size;
bool indexed_advances;
constexpr uint32_t codepoint_mask = 0x3fffffff;
esp_partition_mmap_handle_t mapping;
unsigned stamp;
struct Face {
    int shard = -1;
    unsigned used = 0;
    FT_Face handle = nullptr;
};
// 小型 LRU 缓存压缩字体解码结果，已渲染字形不依赖 face 生命周期。
std::array<Face, 2> faces;
struct CachedGlyph { Glyph bitmap; unsigned used; };
std::unordered_map<uint32_t, CachedGlyph> glyphs;

uint32_t glyph_key(uint32_t cp, int size, bool bold) {
    return (cp << 7) | (size << 1) | bold;
}

const uint32_t *lookup(uint32_t cp) {
    if (!data) return nullptr;
    auto entry = std::lower_bound(index_words, index_words + codepoint_count, cp << 8,
        [](uint32_t packed, uint32_t target) { return (packed & codepoint_mask) < target; });
    return entry != index_words + codepoint_count && ((*entry & codepoint_mask) >> 8) == cp ? entry : nullptr;
}

int indexed_advance(uint32_t cp, int size) {
    if (indexed_advances) {
        if (auto entry = lookup(cp)) return ((*entry >> 30) * 600 * size + 500) / 1000;
    }
    return -1;
}

FT_Face face_for(uint32_t cp) {
    auto entry = lookup(cp);
    if (!entry) return nullptr;
    int shard = *entry & 0xff;
    if (static_cast<uint32_t>(shard) >= shard_count) return nullptr;
    for (auto &face : faces) {
        if (face.shard == shard) {
            face.used = ++stamp;
            return face.handle;
        }
    }
    auto &victim = *std::min_element(faces.begin(), faces.end(), [](const Face &a, const Face &b) {
        return a.used < b.used;
    });
    if (victim.handle) FT_Done_Face(victim.handle);
    victim = {};
    auto offset = directory[shard * 2];
    auto length = directory[shard * 2 + 1];
    if (offset > data_size || length > data_size - offset ||
        FT_New_Memory_Face(library, data + offset, length, 0, &victim.handle)) {
        ESP_LOGE("font", "打开分片 %d 失败", shard);
        return nullptr;
    }
    victim.shard = shard;
    victim.used = ++stamp;
    return victim.handle;
}

Glyph load(uint32_t cp, int size, bool bold) {
    Glyph out;
    auto face = face_for(cp);
    // 字库没有内嵌 hint，低字号用 FreeType 自动对齐细笔画。
    const FT_Int32 flags = FT_LOAD_NO_BITMAP | (size <= 14
        ? FT_LOAD_FORCE_AUTOHINT | FT_LOAD_TARGET_NORMAL : FT_LOAD_NO_HINTING);
    if (face && !FT_Set_Pixel_Sizes(face, 0, size) &&
        !FT_Load_Char(face, cp, flags)) {
        auto slot = face->glyph;
        if (bold && slot->format == FT_GLYPH_FORMAT_OUTLINE)
            FT_Outline_Embolden(&slot->outline, size * 2);
        if (!FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL)) {
            auto &bitmap = slot->bitmap;
            out.width = bitmap.width;
            out.height = bitmap.rows;
            out.left = slot->bitmap_left;
            out.top = slot->bitmap_top;
            // 绘制和测宽共用字库的原始字距，避免滚动位置或断行随 hint 改变。
            out.advance = indexed_advance(cp, size);
            if (out.advance < 0) out.advance = (slot->linearHoriAdvance + 0x8000) >> 16;
            if (out.width > 0 && out.height > 0) {
                out.opacity.resize(out.width * out.height);
                // 在缓存字形时收窄灰阶过渡，保留曲线和斜笔画的抗锯齿。
                constexpr int low = 12, high = 226;
                for (int row = 0; row < out.height; ++row) {
                    const auto *src = bitmap.buffer + row * bitmap.pitch;
                    auto *dst = out.opacity.data() + row * out.width;
                    for (int col = 0; col < out.width; ++col)
                        dst[col] = std::clamp((src[col] - low) * 255 / (high - low), 0, 255);
                }
            }
            return out;
        }
    }

    // 字库未烧录时提供基础 ASCII 诊断界面，中文显示缺字方框。
    if (cp >= 32 && cp < 127) {
        const auto *font = LV_FONT_DEFAULT;
        lv_font_glyph_dsc_t desc{};
        if (lv_font_get_glyph_dsc(font, &desc, cp, 0)) {
            lv_draw_buf_t *scratch = lv_draw_buf_create(desc.box_w ? desc.box_w : 1,
                desc.box_h ? desc.box_h : 1, LV_COLOR_FORMAT_A8, LV_STRIDE_AUTO);
            if (scratch) {
                auto *bitmap = static_cast<const lv_draw_buf_t *>(lv_font_get_glyph_bitmap(&desc, scratch));
                out.advance = std::max(1, static_cast<int>(desc.adv_w * size / font->line_height));
                out.width = desc.box_w * size / font->line_height;
                out.height = desc.box_h * size / font->line_height;
                out.left = desc.ofs_x * size / font->line_height;
                out.top = (desc.box_h + desc.ofs_y) * size / font->line_height;
                if (bitmap && out.width > 0 && out.height > 0) {
                    out.opacity.resize(out.width * out.height);
                    for (int y = 0; y < out.height; ++y) {
                        for (int x = 0; x < out.width; ++x) {
                            out.opacity[y * out.width + x] =
                                bitmap->data[(y * desc.box_h / out.height) * bitmap->header.stride + x * desc.box_w / out.width];
                        }
                    }
                }
                lv_font_glyph_release_draw_data(&desc);
                lv_draw_buf_destroy(scratch);
                return out;
            }
        }
    }
    out.width = std::max(1, size - 3);
    out.height = out.width;
    out.advance = size;
    out.top = out.height;
    out.opacity.resize(out.width * out.height);
    for (int y = 0; y < out.height; ++y)
        for (int x = 0; x < out.width; ++x)
            if (!x || !y || x == out.width - 1 || y == out.height - 1) out.opacity[y * out.width + x] = 255;
    return out;
}
}

bool font_init() {
    auto part = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, ESP_PARTITION_SUBTYPE_ANY, "fonts");
    if (!part || FT_Init_FreeType(&library)) return false;
    uint32_t header[4];
    if (esp_partition_read(part, 0, header, sizeof(header)) != ESP_OK) return false;
    indexed_advances = memcmp(header, "TDF2", 4) == 0;
    if (!indexed_advances && memcmp(header, "TDF1", 4)) return false;
    shard_count = header[1];
    codepoint_count = header[2];
    data_size = header[3];
    if (!shard_count || shard_count > 256 || codepoint_count > 0x110000 ||
        data_size > part->size || 16 + shard_count * 8 + codepoint_count * 4 > data_size) return false;
    const void *mapped;
    if (esp_partition_mmap(part, 0, data_size, ESP_PARTITION_MMAP_DATA, &mapped, &mapping) != ESP_OK)
        return false;
    data = static_cast<const uint8_t *>(mapped);
    directory = reinterpret_cast<const uint32_t *>(data + 16);
    index_words = directory + 2 * shard_count;
    ESP_LOGI("font", "加载 %lu 个码位，%lu 个分片", static_cast<unsigned long>(codepoint_count),
             static_cast<unsigned long>(shard_count));
    return true;
}

int font_advance(uint32_t cp, int size, bool bold) {
    int advance = indexed_advance(cp, size);
    return advance >= 0 ? advance : font_glyph(cp, size, bold).advance;
}

const Glyph &font_glyph(uint32_t cp, int size, bool bold) {
    uint32_t key = glyph_key(cp, size, bold);
    auto it = glyphs.find(key);
    if (it != glyphs.end()) {
        it->second.used = ++stamp;
        return it->second.bitmap;
    }
    if (glyphs.size() == 512) {
        auto victim = std::min_element(glyphs.begin(), glyphs.end(), [](const auto &a, const auto &b) {
            return a.second.used < b.second.used;
        });
        glyphs.erase(victim);
    }
    return glyphs.emplace(key, CachedGlyph{load(cp, size, bold), ++stamp}).first->second.bitmap;
}

void font_prepare(const char *text, int size, bool bold) {
    std::vector<std::pair<unsigned, uint32_t>> missing;
    while (*text) {
        uint32_t cp = font_next(text);
        auto cached = glyphs.find(glyph_key(cp, size, bold));
        if (cached != glyphs.end()) {
            cached->second.used = ++stamp;
            continue;
        }
        auto entry = lookup(cp);
        missing.emplace_back(entry ? *entry & 0xffu : 256u, cp);
    }
    // 同一片的字形连续准备，避免混排时反复打开压缩分片。
    std::sort(missing.begin(), missing.end());
    missing.erase(std::unique(missing.begin(), missing.end()), missing.end());
    for (const auto &[shard, cp] : missing) font_glyph(cp, size, bold);
}

uint32_t font_next(const char *&s) {
    unsigned c = static_cast<uint8_t>(*s++);
    if (c < 0x80) return c;
    unsigned count = c < 0xe0 ? 1 : c < 0xf0 ? 2 : 3;
    unsigned value = c & (0x7f >> (count + 1));
    for (unsigned i = 0; i < count; ++i) {
        unsigned next = static_cast<uint8_t>(*s);
        if ((next & 0xc0) != 0x80) return 0xfffd;
        ++s;
        value = (value << 6) | (next & 0x3f);
    }
    return value > 0x10ffff ? 0xfffd : value;
}
