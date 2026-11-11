#pragma once

struct DisplayTransport;

extern const DisplayTransport ble_transport;

bool ble_display_init();
void ble_display_poll();
bool ble_pair_open();
void ble_pair_close();
void ble_forget_peers();
int ble_pair_code();
bool ble_display_connected();
