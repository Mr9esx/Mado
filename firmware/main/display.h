#pragma once
#include "cJSON.h"
#include "settings.h"
#include <cstdint>

inline constexpr char firmware_version[] = "0.1.0";

struct DisplayState {
    const cJSON *config;
    const cJSON *readings;
    const char *page;
    const char *error;
    uint64_t updated_at;
    uint64_t wall_time;
    uint64_t revision;
    uint64_t reading_generation;
    bool snapshot;
    int utc_offset;
    int battery;
    bool power_known;
    bool external;
    bool charging;
    bool connected;
};

void display_init(unsigned brightness, bool rotated);
void display_startup(const DisplayState &state);
void display_prepare(const DisplayState &state);
void display_brightness(unsigned brightness);
void display_draw(const DisplayState &state, const settings::Menu &menu, unsigned brightness, bool rotated);
void display_tick();
