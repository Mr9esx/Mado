#pragma once
#include "display.h"
#include <cstddef>
#include <string_view>

constexpr size_t message_limit = 65536;

void protocol_init();
const char *protocol_device_id();
struct DisplayTransport {
    const char *name;
    void (*send)(std::string_view wire);
};

void protocol_receive(const DisplayTransport &transport, const char *line);
void protocol_transport_progress(const DisplayTransport &transport);
void protocol_disconnect(const DisplayTransport &transport);
void protocol_tick(bool pause_rotation);
void protocol_power(int battery, bool known, bool external, bool charging);
void protocol_next_page();
DisplayState protocol_state();
unsigned protocol_brightness();
bool protocol_save_brightness(unsigned value);
bool protocol_screen_rotated();
bool protocol_save_screen_rotation(bool value);
