#pragma once

#include <algorithm>
#include <string_view>

namespace wifi {
inline bool valid_credentials(std::string_view ssid, std::string_view password) {
    if (ssid.empty() || ssid.size() > 32 || ssid.find('\0') != std::string_view::npos)
        return false;
    if (password.empty()) return true;
    if (password.size() == 64) {
        return std::all_of(password.begin(), password.end(), [](char ch) {
            return (ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f') || (ch >= 'A' && ch <= 'F');
        });
    }
    return password.size() >= 8 && password.size() <= 63 &&
           std::all_of(password.begin(), password.end(), [](char ch) { return ch >= ' ' && ch <= '~'; });
}
}
