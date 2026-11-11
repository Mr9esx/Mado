#pragma once

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

namespace presentation {
inline int blend_coverage(int current, int next, int fraction) {
    return (current * (256 - fraction) + next * fraction + 128) / 256;
}

inline double round_fixed(double value, double scale) {
    double magnitude = std::abs(value);
    double scaled = magnitude * scale;
    double whole = std::floor(scaled);
    double fraction = scaled - whole;
    if (fraction > .5 || (fraction == .5 && std::fma(magnitude, scale, -scaled) >= 0)) ++whole;
    return std::copysign(whole / scale, value);
}

inline std::string decimal(double value, int places) {
    char text[64];
    std::snprintf(text, sizeof(text), "%.*f", places, value);
    if (std::strchr(text, '.')) {
        auto end = text + std::strlen(text);
        while (end > text && end[-1] == '0') *--end = '\0';
        if (end > text && end[-1] == '.') *--end = '\0';
    }
    return std::strcmp(text, "-0") == 0 ? "0" : text;
}

inline std::string compact_metric(double value) {
    if (!std::isfinite(value)) return "—";
    if (std::abs(value) < 1000) {
        constexpr double fixed_scales[] = {1, 10, 100, 1000};
        for (int places = 3; places >= 0; --places) {
            auto result = decimal(round_fixed(value, fixed_scales[places]), places);
            if (std::abs(std::strtod(result.c_str(), nullptr)) >= 1000) break;
            if (result.size() <= 4) return result;
        }
    }
    constexpr double scales[] = {1, 1e3, 1e6, 1e9, 1e12};
    constexpr char suffix[] = " kmbt";
    int group = 0;
    while (group < 4 && std::abs(value) >= scales[group + 1]) ++group;
    for (int precision : {2, 1}) {
        int unit = group;
        while (true) {
            double scaled = value / scales[unit];
            int places = precision - 1 - static_cast<int>(std::floor(std::log10(std::abs(scaled))));
            double multiplier = std::pow(10, places);
            double rounded = std::round(scaled * multiplier) / multiplier;
            if (std::abs(rounded) >= 1000 && unit < 4) {
                ++unit;
                continue;
            }
            auto result = decimal(rounded, std::max(0, places));
            if (unit) result += suffix[unit];
            if (result.size() <= 4) return result;
            break;
        }
    }
    double magnitude = std::abs(value);
    int exponent = static_cast<int>(std::floor(std::log10(magnitude)));
    int digit = static_cast<int>(std::round(magnitude / std::pow(10, exponent)));
    if (digit == 10) {
        digit = 1;
        ++exponent;
    }
    auto result = (value < 0 ? "-" : "") + std::to_string(digit) + "e" + std::to_string(exponent);
    return result.size() <= 4 ? result : "超限";
}

inline double count_duration(double from, double to) {
    return std::clamp(std::abs(to - from) * 40, 300.0, 800.0);
}

inline double counted_value(double from, double to, double elapsed) {
    if (!std::isfinite(from) || !std::isfinite(to)) return to;
    const auto duration = count_duration(from, to);
    if (elapsed >= duration) return to;
    const auto fraction = elapsed / duration;
    const auto value = from * (1 - fraction) + to * fraction;
    const double scale = std::trunc(from) == from && std::trunc(to) == to ? 1 : 100;
    return round_fixed(value, scale);
}

inline int roll_distance(int from, int to, int direction) {
    return ((to - from) * direction + 10) % 10;
}

inline char roll_digit(int from, int offset, int direction) {
    return '0' + (from + offset * direction + 10) % 10;
}
}
