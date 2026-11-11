#pragma once
#include "display.h"
#include <cstddef>

constexpr size_t message_limit = 65536;

void protocol_init();
void protocol_receive(const char *line);
void protocol_tick(bool pause_rotation);
void protocol_power(int battery, bool known, bool external, bool charging);
void protocol_next_page();
DisplayState protocol_state();
unsigned protocol_brightness();
bool protocol_save_brightness(unsigned value);
bool protocol_screen_rotated();
bool protocol_save_screen_rotation(bool value);
