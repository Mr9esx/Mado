#pragma once
#include <cstdint>
#include <vector>

struct Glyph {
    int width = 0, height = 0;
    int left = 0, top = 0, advance = 0;
    std::vector<uint8_t> opacity;
};

bool font_init();
int font_advance(uint32_t cp, int size, bool bold);
const Glyph &font_glyph(uint32_t cp, int size, bool bold);
void font_prepare(const char *text, int size, bool bold);
uint32_t font_next(const char *&utf8);
