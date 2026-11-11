#include "protocol.h"
#include "wifi_network.h"

#include "esp_heap_caps.h"
#include "esp_mac.h"
#include "esp_random.h"
#include "esp_timer.h"
#include "nvs_flash.h"
#include "nvs.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <set>
#include <string>
#include <utility>

namespace {
using Json = std::unique_ptr<cJSON, decltype(&cJSON_Delete)>;
Json own(cJSON *value) { return Json(value, cJSON_Delete); }
const cJSON *field(const cJSON *obj, const char *name) { return cJSON_GetObjectItemCaseSensitive(obj, name); }
const char *str(const cJSON *value) { return cJSON_IsString(value) ? value->valuestring : ""; }
const char *str(const cJSON *obj, const char *name) { return str(field(obj, name)); }
bool is(const char *a, const char *b) { return strcmp(a, b) == 0; }
bool number(const cJSON *v) { return cJSON_IsNumber(v) && std::isfinite(v->valuedouble); }
bool integer(const cJSON *v, double min = 0, double max = 9007199254740991.0) {
    return number(v) && v->valuedouble >= min && v->valuedouble <= max &&
           floor(v->valuedouble) == v->valuedouble;
}
uint64_t u64(const cJSON *obj, const char *key) {
    const cJSON *v = field(obj, key);
    return integer(v) ? static_cast<uint64_t>(v->valuedouble) : 0;
}
bool text(const cJSON *v, size_t min, size_t max) {
    if (!cJSON_IsString(v)) return false;
    size_t len = strlen(v->valuestring);
    return len >= min && len <= max;
}
bool color(const cJSON *v) {
    return text(v, 7, 7) && str(v)[0] == '#' && strspn(str(v) + 1, "0123456789abcdefABCDEF") == 6;
}
std::string serialize(const cJSON *obj) {
    std::unique_ptr<char, decltype(&cJSON_free)> value(cJSON_PrintUnformatted(obj), cJSON_free);
    return value ? value.get() : "";
}

nvs_handle_t store;
Json config = own(nullptr);
Json readings = own(nullptr);
std::string page;
std::string session;
const DisplayTransport *owner;
const DisplayTransport *recipient;
std::string last_request;
std::string last_response;
std::string welcome_response;
std::string hello_request;
std::string error_message;
char device_id[13];
char boot_id[17];
uint64_t revision;
uint64_t received_id;
uint64_t sent_id;
uint64_t sequence;
uint64_t generation;
uint64_t status_sequence;
uint64_t active_at;
uint64_t status_at;
uint64_t rotated_at;
uint64_t config_at;
uint64_t readings_at;
uint64_t clock_unix;
uint64_t clock_at;
int utc_offset;
int battery = -1;
bool external;
bool charging;
bool power_known;
bool await_snapshot = true;
bool snapshot = true;
unsigned brightness = 70;
bool screen_rotated = false;

bool save_brightness(unsigned value) {
    if (value == brightness) return true;
    if (nvs_set_u8(store, "brightness", value) != ESP_OK || nvs_commit(store) != ESP_OK) return false;
    brightness = value;
    return true;
}

bool save_screen_rotation(bool value) {
    if (value == screen_rotated) return true;
    if (nvs_set_u8(store, "screen_rotated", value) != ESP_OK || nvs_commit(store) != ESP_OK) return false;
    screen_rotated = value;
    return true;
}

uint64_t monotonic_ms() { return esp_timer_get_time() / 1000; }
uint64_t wall_ms() { return clock_unix ? clock_unix + monotonic_ms() - clock_at : 0; }
void send(const std::string &wire) {
    if (recipient && !wire.empty()) recipient->send(wire);
}
std::string envelope(const char *type, cJSON *payload, bool use_session = true) {
    auto obj = own(cJSON_CreateObject());
    cJSON_AddNumberToObject(obj.get(), "protocolVersion", 1);
    if (!use_session || session.empty()) cJSON_AddNullToObject(obj.get(), "sessionId");
    else cJSON_AddStringToObject(obj.get(), "sessionId", session.c_str());
    cJSON_AddNumberToObject(obj.get(), "id", ++sent_id);
    cJSON_AddStringToObject(obj.get(), "type", type);
    cJSON_AddItemToObject(obj.get(), "payload", payload);
    return serialize(obj.get()) + "\n";
}
std::string result(uint64_t id, const char *error, cJSON *data = nullptr, bool use_session = true) {
    cJSON *payload = cJSON_CreateObject();
    cJSON_AddNumberToObject(payload, "replyTo", id);
    cJSON_AddBoolToObject(payload, "ok", !error);
    if (error) {
        cJSON *reason = cJSON_AddObjectToObject(payload, "error");
        cJSON_AddStringToObject(reason, "code", error);
        cJSON_AddStringToObject(reason, "message", error);
        if (data) cJSON_AddItemToObject(reason, "details", data);
    } else {
        cJSON_AddItemToObject(payload, "data", data ? data : cJSON_CreateObject());
    }
    return envelope("result", payload, use_session);
}
void status() {
    if (session.empty()) return;
    auto previous = recipient;
    recipient = owner;
    auto *p = cJSON_CreateObject();
    cJSON_AddNumberToObject(p, "statusSequence", ++status_sequence);
    cJSON_AddNumberToObject(p, "configRevision", revision);
    if (page.empty()) cJSON_AddNullToObject(p, "activePageId");
    else cJSON_AddStringToObject(p, "activePageId", page.c_str());
    cJSON_AddNumberToObject(p, "brightnessPercent", brightness);
    cJSON_AddBoolToObject(p, "screenRotated", screen_rotated);
    cJSON_AddStringToObject(p, "transport", owner->name);
    cJSON_AddNullToObject(p, "appVersion");
    auto *power = cJSON_AddObjectToObject(p, "power");
    cJSON_AddStringToObject(power, "source", !power_known ? "unknown" : external ? "external" : "battery");
    if (battery < 0) cJSON_AddNullToObject(power, "batteryPercent");
    else cJSON_AddNumberToObject(power, "batteryPercent", battery);
    if (power_known) cJSON_AddBoolToObject(power, "charging", charging);
    else cJSON_AddNullToObject(power, "charging");
    cJSON_AddBoolToObject(p, "clockSynced", clock_unix != 0);
    cJSON_AddNumberToObject(p, "uptimeMs", monotonic_ms());
    if (config_at) cJSON_AddNumberToObject(p, "lastConfigAppliedAtUnixMs", config_at);
    else cJSON_AddNullToObject(p, "lastConfigAppliedAtUnixMs");
    if (readings_at) cJSON_AddNumberToObject(p, "lastReadingsAppliedAtUnixMs", readings_at);
    else cJSON_AddNullToObject(p, "lastReadingsAppliedAtUnixMs");
    send(envelope("device.status", p));
    status_at = monotonic_ms();
    recipient = previous;
}
bool valid_render(const cJSON *r) {
    if (!cJSON_IsObject(r) || !text(field(r, "label"), 1, 128)) return false;
    const char *kind = str(r, "kind");
    if (!(is(kind, "metric") || is(kind, "progress") || is(kind, "state") ||
          is(kind, "chart") || is(kind, "trend") || is(kind, "range"))) return false;
    if (auto v = field(r, "unit"); v && !text(v, 0, 48)) return false;
    if (auto v = field(r, "accentColor"); v && !color(v)) return false;
    if (auto v = field(r, "numberAnimation"); v &&
        !is(str(v), "none") && !is(str(v), "count") && !is(str(v), "digitRoll")) return false;
    if (auto v = field(r, "format"); v && !is(str(v), "number") && !is(str(v), "countdown")) return false;
    if (auto v = field(r, "expiredText"); v && !text(v, 1, 64)) return false;
    if (auto details = field(r, "detailFields")) {
        if (!cJSON_IsArray(details) || cJSON_GetArraySize(details) > 2) return false;
        std::set<std::string> names;
        cJSON *entry;
        cJSON_ArrayForEach(entry, details) {
            if (!text(field(entry, "key"), 1, 64) || !text(field(entry, "label"), 1, 64) ||
                !names.insert(str(entry, "key")).second) return false;
        }
    }
    if (is(kind, "state")) {
        auto labels = field(r, "stateLabels");
        if (!cJSON_IsObject(labels) || cJSON_GetArraySize(labels) < 1 || cJSON_GetArraySize(labels) > 24) return false;
        cJSON *v;
        cJSON_ArrayForEach(v, labels) {
            if (!text(v, 1, 128)) return false;
        }
        if (auto styles = field(r, "stateStyles")) {
            if (!cJSON_IsObject(styles)) return false;
            cJSON_ArrayForEach(v, styles) {
                if (!field(labels, v->string) || !cJSON_IsObject(v)) return false;
                if (auto c = field(v, "color"); c && !color(c)) return false;
                if (auto c = field(v, "alertText"); c && !color(c) && !is(str(c), "auto")) return false;
                if (auto t = field(v, "alertDurationMs"); t &&
                    (!integer(t, 0, 10000) || t->valueint % 1000 != 0)) return false;
            }
        }
    }
    if (is(kind, "chart")) {
        auto c = field(r, "chart");
        if (!integer(field(c, "sampleCount"), 1, 12) ||
            !integer(field(c, "rangeSeconds"), 1) ||
            !number(field(c, "min")) || !number(field(c, "max")) ||
            field(c, "min")->valuedouble >= field(c, "max")->valuedouble) return false;
    }
    if (is(kind, "range")) {
        auto c = field(r, "range");
        if (!is(str(c, "mode"), "reading") &&
            (!is(str(c, "mode"), "fixed") || !number(field(c, "min")) || !number(field(c, "max")) ||
             field(c, "min")->valuedouble >= field(c, "max")->valuedouble)) return false;
    }
    return true;
}
const cJSON *find_page(const cJSON *conf, const char *id) {
    cJSON *p;
    cJSON_ArrayForEach(p, field(conf, "pages")) {
        if (is(str(p, "pageId"), id)) return p;
    }
    return nullptr;
}
bool valid_config(const cJSON *conf) {
    if (u64(conf, "schemaVersion") != 1 || !is(str(conf, "profileId"), "tdisplay-c5-v1")) return false;
    auto theme = field(conf, "theme");
    if (!is(str(theme, "active"), "dark") && !is(str(theme, "active"), "light")) return false;
    for (auto scheme : {"dark", "light"}) {
        for (auto property : {"background", "surface", "text"}) {
            if (!color(field(field(theme, scheme), property))) return false;
        }
    }
    auto pages = field(conf, "pages");
    if (!cJSON_IsArray(pages) || cJSON_GetArraySize(pages) < 1 || cJSON_GetArraySize(pages) > 5) return false;
    auto playback = field(conf, "playback");
    auto interval = field(playback, "rotationSeconds");
    if (!integer(interval, 0, 60)) return false;
    switch (interval->valueint) {
        case 0: case 5: case 10: case 15: case 30: case 60: break;
        default: return false;
    }
    std::set<std::string> page_ids, card_ids;
    cJSON *p;
    cJSON_ArrayForEach(p, pages) {
        if (!text(field(p, "pageId"), 1, 96) || !text(field(p, "name"), 1, 80) ||
            !page_ids.insert(str(p, "pageId")).second) return false;
        auto cards = field(p, "cards");
        if (!cJSON_IsArray(cards) || cJSON_GetArraySize(cards) > 8) return false;
        unsigned cells = 0;
        cJSON *c;
        cJSON_ArrayForEach(c, cards) {
            if (!text(field(c, "cardId"), 1, 384) || !card_ids.insert(str(c, "cardId")).second ||
                !integer(field(c, "x"), 0, 3) || !integer(field(c, "y"), 0, 1) ||
                !integer(field(c, "w"), 1, 4) || !integer(field(c, "h"), 1, 2) ||
                !valid_render(field(c, "render"))) return false;
            int x = u64(c, "x"), y = u64(c, "y"), w = u64(c, "w"), h = u64(c, "h");
            bool span = (h == 1 && (w == 1 || w == 2 || w == 4)) || (h == 2 && w == 2);
            if (!span || x + w > 4 || y + h > 2) return false;
            for (int row = y; row < y + h; ++row) {
                for (int col = x; col < x + w; ++col) {
                    unsigned bit = 1u << (row * 4 + col);
                    if (cells & bit) return false;
                    cells |= bit;
                }
            }
        }
    }
    return page_ids.count(str(playback, "defaultPageId")) != 0;
}
bool valid_reading(const cJSON *render, const cJSON *v, bool initial) {
    if (cJSON_IsNull(v)) return true;
    if (!cJSON_IsObject(v) || !is(str(render, "kind"), str(v, "kind"))) return false;
    const char *kind = str(render, "kind");
    if (auto details = field(v, "details")) {
        if (!cJSON_IsObject(details) || cJSON_GetArraySize(details) > 2) return false;
        cJSON *detail;
        cJSON_ArrayForEach(detail, details) {
            bool declared = false;
            cJSON *definition;
            cJSON_ArrayForEach(definition, field(render, "detailFields")) {
                if (is(str(definition, "key"), detail->string)) declared = true;
            }
            if (!declared || !text(detail, 0, 512)) return false;
        }
    }
    if (is(kind, "state")) {
        if (!field(field(render, "stateLabels"), str(v, "state"))) return false;
        if (auto detail = field(v, "detail"); detail && !cJSON_IsNull(detail) && !text(detail, 0, 1024)) return false;
        if (auto notify = field(v, "notify"); notify &&
            (!cJSON_IsBool(notify) || (cJSON_IsTrue(notify) &&
            (initial || !cJSON_IsTrue(field(render, "supportsNotify")))))) return false;
        return true;
    }
    if (is(kind, "chart")) {
        auto values = field(v, "values");
        if (!integer(field(v, "windowEndUnixMs")) || !cJSON_IsArray(values) ||
            cJSON_GetArraySize(values) != static_cast<int>(u64(field(render, "chart"), "sampleCount"))) return false;
        cJSON *sample;
        cJSON_ArrayForEach(sample, values) {
            if (!cJSON_IsNull(sample) && !number(sample)) return false;
        }
        return true;
    }
    if (is(str(render, "format"), "countdown")) return integer(field(v, "deadlineUnixMs"));
    if (!number(field(v, "value"))) return false;
    if (is(kind, "progress")) {
        auto ratio = field(v, "ratio");
        return cJSON_IsNull(ratio) || (number(ratio) && ratio->valuedouble >= 0 && ratio->valuedouble <= 1);
    }
    if (is(kind, "trend")) {
        auto delta = field(v, "delta");
        return cJSON_IsNull(delta) || number(delta);
    }
    if (is(kind, "range")) {
        if (auto b = field(v, "bounds")) {
            return number(field(b, "min")) && number(field(b, "max")) &&
                   field(b, "min")->valuedouble < field(b, "max")->valuedouble;
        }
    }
    return true;
}
bool valid_readings(const cJSON *values, bool initial) {
    if (!config || !cJSON_IsObject(values)) return false;
    int count = 0;
    cJSON *p, *c;
    cJSON_ArrayForEach(p, field(config.get(), "pages")) {
        cJSON_ArrayForEach(c, field(p, "cards")) {
            ++count;
            if (!valid_reading(field(c, "render"), field(values, str(c, "cardId")), initial)) return false;
        }
    }
    return count == cJSON_GetArraySize(values);
}
bool store_config(const cJSON *value, uint64_t version) {
    auto record = own(cJSON_CreateObject());
    cJSON_AddNumberToObject(record.get(), "revision", version);
    cJSON_AddItemToObject(record.get(), "config", cJSON_Duplicate(value, true));
    auto payload = serialize(record.get());
    return !payload.empty() && nvs_set_blob(store, "config", payload.data(), payload.size()) == ESP_OK &&
           nvs_commit(store) == ESP_OK;
}
void keep_compatible_readings(const cJSON *next) {
    auto kept = own(cJSON_CreateObject());
    cJSON *next_page, *next_card, *old_page, *old_card;
    cJSON_ArrayForEach(next_page, field(next, "pages")) {
        cJSON_ArrayForEach(next_card, field(next_page, "cards")) {
            auto id = str(next_card, "cardId");
            auto value = field(readings.get(), id);
            if (!value) continue;
            cJSON_ArrayForEach(old_page, field(config.get(), "pages")) {
                cJSON_ArrayForEach(old_card, field(old_page, "cards")) {
                    if (is(str(old_card, "cardId"), id) &&
                        cJSON_Compare(field(old_card, "render"), field(next_card, "render"), true)) {
                        auto copy = cJSON_Duplicate(value, true);
                        if (cJSON_IsObject(copy)) cJSON_DeleteItemFromObjectCaseSensitive(copy, "notify");
                        cJSON_AddItemToObject(kept.get(), id, copy);
                    }
                }
            }
        }
    }
    readings = std::move(kept);
}
void select_page(const char *id) {
    page = id;
    rotated_at = monotonic_ms();
}
void hello(const cJSON *request, const char *line) {
    if (u64(request, "id") != 1 || !cJSON_IsNull(field(request, "sessionId"))) return;
    if (!session.empty() && owner == recipient && hello_request == line) {
        active_at = monotonic_ms();
        send(welcome_response);
        return;
    }
    if (!session.empty()) {
        send(result(1, "SESSION_BUSY", nullptr, false));
        return;
    }
    auto input = field(request, "payload");
    bool compatible = false;
    cJSON *item;
    cJSON_ArrayForEach(item, field(input, "supportedProtocolVersions")) {
        if (integer(item, 1, 1)) compatible = true;
    }
    if (!compatible) {
        send(result(1, "UNSUPPORTED_PROTOCOL", nullptr, false));
        return;
    }
    if (!text(field(input, "appVersion"), 1, 64)) {
        send(result(1, "INVALID_PAYLOAD", nullptr, false));
        return;
    }
    char new_id[17];
    snprintf(new_id, sizeof(new_id), "%08lx%08lx", static_cast<unsigned long>(esp_random()),
             static_cast<unsigned long>(esp_random()));
    session = new_id;
    owner = recipient;
    active_at = monotonic_ms();
    received_id = 1;
    sent_id = sequence = status_sequence = 0;
    await_snapshot = true;
    error_message.clear();
    last_request.clear();
    last_response.clear();
    hello_request = line;
    auto payload = own(cJSON_Parse(R"({"replyTo":1,"profileId":"tdisplay-c5-v1","capabilities":{"maxPages":5,"columns":4,"rows":2,"maxMessageBytes":65536,"maxChartPoints":12,"maxDetailBytes":1024,"supportedSpans":[{"w":1,"h":1},{"w":2,"h":1},{"w":2,"h":2},{"w":4,"h":1}],"kinds":["metric","progress","state","chart","trend","range"],"numberAnimations":["none","count","digitRoll"],"settings":["brightnessPercent","screenRotated"],"wifiProvisioning":true}})"));
    cJSON_AddStringToObject(payload.get(), "firmwareVersion", firmware_version);
    cJSON_AddStringToObject(payload.get(), "deviceId", device_id);
    cJSON_AddStringToObject(payload.get(), "bootId", boot_id);
    cJSON_AddNumberToObject(payload.get(), "configRevision", revision);
    welcome_response = envelope("welcome", payload.release());
    send(welcome_response);
    status();
}
std::string execute(const char *type, const cJSON *p, uint64_t id, bool &changed) {
    if (is(type, "wifi.provision") || is(type, "wifi.status")) {
        if (is(owner->name, "wifi")) return result(id, "LOCAL_LINK_REQUIRED");
        if (is(type, "wifi.status")) return result(id, nullptr, wifi::provision_status());
        const char *error = wifi::provision(p);
        return result(id, error, error ? nullptr : wifi::provision_status());
    }
    if (is(type, "ping")) {
        auto *data = cJSON_CreateObject();
        cJSON_AddNumberToObject(data, "uptimeMs", monotonic_ms());
        return result(id, nullptr, data);
    }
    if (is(type, "time.sync")) {
        if (!integer(field(p, "unixMs"), 1) || !integer(field(p, "utcOffsetMinutes"), -840, 840))
            return result(id, "INVALID_PAYLOAD");
        clock_unix = u64(p, "unixMs");
        clock_at = monotonic_ms();
        utc_offset = field(p, "utcOffsetMinutes")->valueint;
        auto *data = cJSON_CreateObject();
        cJSON_AddBoolToObject(data, "clockSynced", true);
        return result(id, nullptr, data);
    }
    if (is(type, "settings.set")) {
        if (cJSON_GetArraySize(p) != 1) return result(id, "INVALID_PAYLOAD");
        if (auto rotated = field(p, "screenRotated")) {
            if (!cJSON_IsBool(rotated)) return result(id, "INVALID_PAYLOAD");
            bool value = cJSON_IsTrue(rotated);
            bool needs_update = screen_rotated != value;
            if (!save_screen_rotation(value)) return result(id, "STORAGE_FAILED");
            changed = needs_update;
            auto *data = cJSON_CreateObject();
            cJSON_AddBoolToObject(data, "screenRotated", screen_rotated);
            return result(id, nullptr, data);
        }
        if (!integer(field(p, "brightnessPercent"), 0, 100)) return result(id, "INVALID_PAYLOAD");
        unsigned value = u64(p, "brightnessPercent");
        changed = brightness != value;
        if (!save_brightness(value)) {
            changed = false;
            return result(id, "STORAGE_FAILED");
        }
        auto *data = cJSON_CreateObject();
        cJSON_AddNumberToObject(data, "brightnessPercent", brightness);
        return result(id, nullptr, data);
    }
    bool setting = is(type, "config.set");
    if (!setting && !is(type, "readings.set") && !is(type, "page.set")) return result(id, "INVALID_PAYLOAD");
    const char *version_key = setting ? "baseRevision" : "configRevision";
    if (!integer(field(p, version_key))) return result(id, "INVALID_PAYLOAD");
    if (u64(p, version_key) != revision) {
        auto *detail = cJSON_CreateObject();
        cJSON_AddNumberToObject(detail, "expectedRevision", revision);
        cJSON_AddNumberToObject(detail, "receivedRevision", u64(p, version_key));
        return result(id, "CONFIG_REVISION_MISMATCH", detail);
    }
    if (setting) {
        auto *next = field(p, "config");
        if (!valid_config(next)) return result(id, "INVALID_PAYLOAD");
        bool different = !config || !cJSON_Compare(config.get(), next, true);
        if (different) {
            auto copy = own(cJSON_Duplicate(next, true));
            if (!copy || !store_config(next, revision + 1)) return result(id, "STORAGE_FAILED");
            keep_compatible_readings(next);
            config = std::move(copy);
            ++revision;
            if (!find_page(next, page.c_str())) select_page(str(field(next, "playback"), "defaultPageId"));
            rotated_at = monotonic_ms();
            config_at = wall_ms();
            snapshot = true;
            ++generation;
        }
        error_message.clear();
        await_snapshot = true;
        changed = true;
        auto *data = cJSON_CreateObject();
        cJSON_AddNumberToObject(data, "configRevision", revision);
        cJSON_AddBoolToObject(data, "changed", different);
        return result(id, nullptr, data);
    }
    if (is(type, "page.set")) {
        auto target = str(p, "pageId");
        if (!find_page(config.get(), target)) return result(id, "PAGE_NOT_FOUND");
        changed = page != target;
        select_page(target);
        auto *data = cJSON_CreateObject();
        cJSON_AddNumberToObject(data, "configRevision", revision);
        cJSON_AddStringToObject(data, "activePageId", page.c_str());
        return result(id, nullptr, data);
    }
    bool initial = is(str(p, "mode"), "snapshot");
    if (!initial && !is(str(p, "mode"), "update")) return result(id, "INVALID_PAYLOAD");
    if (await_snapshot && !initial) return result(id, "SNAPSHOT_REQUIRED");
    if (!await_snapshot && initial) return result(id, "INVALID_STATE");
    if (!integer(field(p, "sequence"), 1)) return result(id, "INVALID_PAYLOAD");
    if (u64(p, "sequence") <= sequence) return result(id, "STALE_SEQUENCE");
    auto *values = field(p, "readings");
    if (!valid_readings(values, initial)) return result(id, "INVALID_PAYLOAD");
    auto copy = own(cJSON_Duplicate(values, true));
    if (!copy) return result(id, "INTERNAL_ERROR");
    readings = std::move(copy);
    sequence = u64(p, "sequence");
    snapshot = initial;
    await_snapshot = false;
    readings_at = wall_ms();
    ++generation;
    error_message.clear();
    auto *data = cJSON_CreateObject();
    cJSON_AddNumberToObject(data, "configRevision", revision);
    cJSON_AddNumberToObject(data, "acceptedSequence", sequence);
    return result(id, nullptr, data);
}
bool validate_json_tree(const cJSON *node, unsigned depth = 0) {
    if (depth > 16) return false;
    std::set<std::string> names;
    cJSON *child;
    cJSON_ArrayForEach(child, node) {
        if (cJSON_IsObject(node) && (!child->string || !names.insert(child->string).second)) return false;
        if ((cJSON_IsObject(child) || cJSON_IsArray(child)) && !validate_json_tree(child, depth + 1)) return false;
    }
    return true;
}
bool reasonable_nesting(const char *line) {
    unsigned depth = 0;
    bool quoted = false;
    for (const char *p = line; *p; ++p) {
        if (quoted && *p == '\\' && p[1]) ++p;
        else if (*p == '"') quoted = !quoted;
        else if (!quoted && (*p == '[' || *p == '{') && ++depth > 16) return false;
        else if (!quoted && (*p == ']' || *p == '}')) {
            if (depth == 0) return false;
            --depth;
        }
    }
    return depth == 0 && !quoted;
}
}

void protocol_init() {
    // JSON frames and retained readings can outgrow the internal memory left by both radios.
    cJSON_Hooks json_memory{
        [](size_t size) { return heap_caps_malloc(size, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT); },
        free,
    };
    cJSON_InitHooks(&json_memory);
    ESP_ERROR_CHECK(nvs_flash_init_partition("displaycfg"));
    ESP_ERROR_CHECK(nvs_open_from_partition("displaycfg", "display", NVS_READWRITE, &store));
    uint8_t saved_brightness;
    if (nvs_get_u8(store, "brightness", &saved_brightness) == ESP_OK && saved_brightness <= 100)
        brightness = saved_brightness;
    uint8_t saved_rotation;
    if (nvs_get_u8(store, "screen_rotated", &saved_rotation) == ESP_OK)
        screen_rotated = saved_rotation == 1;
    size_t size = 0;
    if (nvs_get_blob(store, "config", nullptr, &size) == ESP_OK && size && size <= message_limit) {
        std::string payload(size, '\0');
        if (nvs_get_blob(store, "config", payload.data(), &size) == ESP_OK) {
            auto data = own(cJSON_Parse(payload.c_str()));
            auto value = field(data.get(), "config");
            if (valid_config(value) && u64(data.get(), "revision") > 0) {
                config = own(cJSON_Duplicate(value, true));
                revision = u64(data.get(), "revision");
                select_page(str(field(value, "playback"), "defaultPageId"));
            }
        }
    }
    uint8_t mac[6];
    ESP_ERROR_CHECK(esp_read_mac(mac, ESP_MAC_BASE));
    snprintf(device_id, sizeof(device_id), "%02x%02x%02x%02x%02x%02x", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    snprintf(boot_id, sizeof(boot_id), "%08lx%08lx", static_cast<unsigned long>(esp_random()),
             static_cast<unsigned long>(esp_random()));
}

const char *protocol_device_id() { return device_id; }

static void receive(const char *line) {
    if (!reasonable_nesting(line)) return;
    auto request = own(cJSON_ParseWithOpts(line, nullptr, true));
    auto r = request.get();
    if (!cJSON_IsObject(r) || !integer(field(r, "id"), 1) || !validate_json_tree(r)) return;
    auto id = u64(r, "id");
    auto type = str(r, "type");
    if (u64(r, "protocolVersion") != 1) {
        send(result(id, "UNSUPPORTED_PROTOCOL", nullptr, false));
        return;
    }
    if (is(type, "hello")) {
        hello(r, line);
        return;
    }
    if (session.empty() || owner != recipient || session != str(r, "sessionId")) {
        send(result(id, "INVALID_SESSION", nullptr, false));
        return;
    }
    active_at = monotonic_ms();
    if (is(type, "session.close") && id > received_id && cJSON_IsObject(field(r, "payload"))) {
        send(result(id, nullptr));
        protocol_disconnect(*owner);
        return;
    }
    if (id == received_id && !last_response.empty()) {
        if (last_request == line) send(last_response);
        else {
            send(result(id, "REQUEST_ID_REUSED"));
            session.clear();
        }
        return;
    }
    if (id <= received_id) {
        send(result(id, "DUPLICATE_EXPIRED"));
        return;
    }
    bool changed = false;
    received_id = id;
    last_request = line;
    auto p = field(r, "payload");
    last_response = cJSON_IsObject(p) ? execute(type, p, id, changed) : result(id, "INVALID_PAYLOAD");
    send(last_response);
    if (!config && last_response.find("\"ok\":false") != std::string::npos)
        error_message = "配置未能通过校验，请检查 Studio 的连接和布局设置。";
    if (changed) status();
}

void protocol_receive(const DisplayTransport &transport, const char *line) {
    recipient = &transport;
    receive(line);
    recipient = nullptr;
}

void protocol_transport_progress(const DisplayTransport &transport) {
    if (owner == &transport && !session.empty()) active_at = monotonic_ms();
}

void protocol_disconnect(const DisplayTransport &transport) {
    if (owner != &transport) return;
    session.clear();
    await_snapshot = true;
    owner = nullptr;
}

void protocol_power(int percent, bool known, bool has_external, bool is_charging) {
    if (power_known == known && battery == percent && external == has_external && charging == is_charging) return;
    battery = percent;
    external = has_external;
    charging = is_charging;
    power_known = known;
    status();
}

void protocol_next_page() {
    auto pages = field(config.get(), "pages");
    if (cJSON_GetArraySize(pages) < 2) return;
    auto current = find_page(config.get(), page.c_str());
    auto next = current && current->next ? current->next : cJSON_GetArrayItem(pages, 0);
    select_page(str(next, "pageId"));
    status();
}

void protocol_tick(bool pause_rotation) {
    auto now = monotonic_ms();
    if (!session.empty() && now - active_at >= 15000) {
        protocol_disconnect(*owner);
    }
    if (!session.empty() && now - status_at >= 5000) status();
    auto rotation = u64(field(config.get(), "playback"), "rotationSeconds");
    if (pause_rotation) rotated_at = now;
    else if (rotation && now - rotated_at >= rotation * 1000) protocol_next_page();
}

DisplayState protocol_state() {
    return {config.get(), readings.get(), page.c_str(), error_message.c_str(), readings_at, wall_ms(), revision,
            generation, snapshot, utc_offset, battery, power_known, external, charging, !session.empty(), session.empty() ? nullptr : owner->name};
}

unsigned protocol_brightness() { return brightness; }

bool protocol_save_brightness(unsigned value) {
    if (value > 100) return false;
    bool changed = brightness != value;
    if (!save_brightness(value)) return false;
    if (changed) status();
    return true;
}

bool protocol_screen_rotated() { return screen_rotated; }

bool protocol_save_screen_rotation(bool value) {
    bool changed = screen_rotated != value;
    if (!save_screen_rotation(value)) return false;
    if (changed) status();
    return true;
}
