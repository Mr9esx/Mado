#pragma once

#include <cstdint>
#include <memory>
#include <string>

struct cJSON;

namespace wifi {
constexpr uint16_t port = 47861;

struct Profile {
    std::string ssid, password, ca, certificate, key, id;
};

struct Network {
    std::shared_ptr<const Profile> profile;
    uint32_t address = 0;
};

void init(const char *device_id);
const char *provision(const cJSON *payload);
cJSON *provision_status();
bool forget();
Network active_network();
std::string address();
bool has_saved_network();
}
