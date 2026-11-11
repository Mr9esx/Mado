#include "wifi_network.h"
#include "wifi_credentials.h"

#include "cJSON.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_timer.h"
#include "esp_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "lwip/sockets.h"
#include "mdns.h"
#include "nvs.h"

#include <cstring>
#include <mutex>
#include <utility>

namespace wifi {
namespace {
using ProfilePtr = std::shared_ptr<const Profile>;
struct Event {
    enum { associated, disconnected, got_address } type;
    uint32_t data;
};

std::mutex mutex;
QueueHandle_t events;
TaskHandle_t task;
nvs_handle_t storage;
esp_netif_t *station;
bool started;
bool mdns_ready;
bool advertised;
char identifier[13];
char host[24];
char instance[16];

Network online;
ProfilePtr candidate;
bool erase_requested;
bool has_saved;
std::string request_id;
std::string saved_id;
const char *phase = "idle";
const char *failure = nullptr;
bool check(const char *operation, esp_err_t result) {
    if (result != ESP_OK) ESP_LOGW("mado_wifi", "%s: %s", operation, esp_err_to_name(result));
    return result == ESP_OK;
}

const char *get_string(const cJSON *json, const char *key) {
    const char *value = cJSON_GetStringValue(cJSON_GetObjectItemCaseSensitive(json, key));
    return value ? value : "";
}

ProfilePtr parse_profile(const cJSON *payload) {
    for (const char *name : {"ssid", "password", "ca", "certificate", "key"}) {
        if (!cJSON_IsString(cJSON_GetObjectItemCaseSensitive(payload, name))) return {};
    }
    Profile profile{
        get_string(payload, "ssid"), get_string(payload, "password"), get_string(payload, "ca"),
        get_string(payload, "certificate"), get_string(payload, "key"), get_string(payload, "requestId"),
    };
    auto pem = [](const std::string &value, const char *prefix) {
        return value.size() <= 2048 && value.find(prefix) == 0;
    };
    if (!valid_credentials(profile.ssid, profile.password) ||
        !pem(profile.ca, "-----BEGIN CERTIFICATE-----") ||
        !pem(profile.certificate, "-----BEGIN CERTIFICATE-----") ||
        !pem(profile.key, "-----BEGIN PRIVATE KEY-----"))
        return {};
    return std::make_shared<Profile>(std::move(profile));
}

ProfilePtr load() {
    size_t length = 0;
    if (nvs_get_blob(storage, "profile", nullptr, &length) != ESP_OK || length < 2 || length > 8192) return {};
    std::string data(length, '\0');
    if (nvs_get_blob(storage, "profile", data.data(), &length) != ESP_OK) return {};
    auto *json = cJSON_Parse(data.c_str());
    auto profile = parse_profile(json);
    cJSON_Delete(json);
    return profile;
}

bool save(const Profile &profile) {
    auto *json = cJSON_CreateObject();
    cJSON_AddStringToObject(json, "ssid", profile.ssid.c_str());
    cJSON_AddStringToObject(json, "password", profile.password.c_str());
    cJSON_AddStringToObject(json, "ca", profile.ca.c_str());
    cJSON_AddStringToObject(json, "certificate", profile.certificate.c_str());
    cJSON_AddStringToObject(json, "key", profile.key.c_str());
    cJSON_AddStringToObject(json, "requestId", profile.id.c_str());
    char *data = cJSON_PrintUnformatted(json);
    cJSON_Delete(json);
    if (!data) return false;
    auto result = nvs_set_blob(storage, "profile", data, strlen(data) + 1);
    cJSON_free(data);
    return result == ESP_OK && nvs_commit(storage) == ESP_OK;
}

void event_handler(void *, esp_event_base_t base, int32_t id, void *data) {
    Event event{};
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_CONNECTED) {
        event.type = Event::associated;
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        event = {Event::disconnected, static_cast<wifi_event_sta_disconnected_t *>(data)->reason};
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        event = {Event::got_address, static_cast<ip_event_got_ip_t *>(data)->ip_info.ip.addr};
    } else {
        return;
    }
    xQueueSend(events, &event, 0);
}

bool initialize_radio() {
    if (!check("netif", esp_netif_init())) return false;
    const auto loop = esp_event_loop_create_default();
    if (loop != ESP_ERR_INVALID_STATE && !check("event-loop", loop)) return false;
    station = esp_netif_create_default_wifi_sta();
    if (!station) return false;
    wifi_init_config_t config = WIFI_INIT_CONFIG_DEFAULT();
    if (!check("radio-init", esp_wifi_init(&config)) ||
        !check("storage", esp_wifi_set_storage(WIFI_STORAGE_RAM)) ||
        !check("station-mode", esp_wifi_set_mode(WIFI_MODE_STA)) ||
        !check("wifi-handler", esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, event_handler, nullptr)) ||
        !check("ip-handler", esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, event_handler, nullptr)))
        return false;
    mdns_ready = mdns_init() == ESP_OK && mdns_hostname_set(host) == ESP_OK;
    return true;
}

void set_online(ProfilePtr profile, uint32_t ip) {
    {
        std::lock_guard lock(mutex);
        online = {std::move(profile), ip};
    }
    if (advertised) {
        mdns_service_remove("_mado", "_tcp");
        advertised = false;
    }
    if (ip && mdns_ready) {
        mdns_txt_item_t txt[] = {{"id", identifier}, {"version", "1"}};
        advertised = mdns_service_add(instance, "_mado", "_tcp", port, txt, 2) == ESP_OK;
    }
}

void stop_radio() {
    set_online({}, 0);
    if (started) esp_wifi_stop();
    started = false;
}

bool command_waiting() {
    std::lock_guard lock(mutex);
    return candidate || erase_requested;
}

uint32_t join(const Profile &profile, const char *&error, bool interruptible) {
    stop_radio();
    xQueueReset(events);
    wifi_config_t config{};
    memcpy(config.sta.ssid, profile.ssid.data(), profile.ssid.size());
    memcpy(config.sta.password, profile.password.data(), profile.password.size());
    config.sta.threshold.authmode = profile.password.empty() ? WIFI_AUTH_OPEN : WIFI_AUTH_WPA2_PSK;
    config.sta.pmf_cfg.capable = true;
    config.sta.sae_pwe_h2e = WPA3_SAE_PWE_BOTH;
    if (!check("radio-config", esp_wifi_set_config(WIFI_IF_STA, &config)) ||
        !check("radio-start", esp_wifi_start())) {
        error = "RADIO_FAILED";
        return 0;
    }
    started = true;
    if (!check("radio-connect", esp_wifi_connect())) {
        error = "RADIO_FAILED";
        return 0;
    }
    const auto deadline = esp_timer_get_time() + 19'000'000;
    bool associated = false;
    int retries = 0;
    while (esp_timer_get_time() < deadline && (!interruptible || !command_waiting())) {
        Event event;
        if (xQueueReceive(events, &event, pdMS_TO_TICKS(100)) != pdTRUE) continue;
        if (event.type == Event::associated) associated = true;
        if (event.type == Event::got_address && associated && event.data) {
            error = nullptr;
            return event.data;
        }
        if (event.type != Event::disconnected) continue;
        associated = false;
        switch (event.data) {
        case WIFI_REASON_AUTH_FAIL:
        case WIFI_REASON_4WAY_HANDSHAKE_TIMEOUT:
        case WIFI_REASON_HANDSHAKE_TIMEOUT:
            error = "AUTH_FAILED";
            break;
        case WIFI_REASON_NO_AP_FOUND:
        case WIFI_REASON_NO_AP_FOUND_W_COMPATIBLE_SECURITY:
            error = "NETWORK_NOT_FOUND";
            break;
        default:
            error = "JOIN_TIMEOUT";
        }
        if (retries++ >= 2) break;
        vTaskDelay(pdMS_TO_TICKS(350));
        esp_wifi_connect();
    }
    if (!error) error = associated ? "JOIN_TIMEOUT" : "NETWORK_NOT_FOUND";
    return 0;
}

void network_task(void *) {
    auto saved = load();
    {
        std::lock_guard lock(mutex);
        has_saved = static_cast<bool>(saved);
        if (saved) saved_id = saved->id;
    }
    bool init_attempted = false, ready = false;
    uint32_t ip = 0;
    int64_t retry_at = 0;
    for (;;) {
        ProfilePtr next;
        bool erase;
        {
            std::lock_guard lock(mutex);
            next = std::move(candidate);
            erase = std::exchange(erase_requested, false);
        }
        if (!init_attempted && (saved || next)) {
            init_attempted = true;
            ready = initialize_radio();
        }
        if (erase) {
            const auto result = nvs_erase_key(storage, "profile");
            if ((result == ESP_OK || result == ESP_ERR_NVS_NOT_FOUND) && nvs_commit(storage) == ESP_OK) {
                stop_radio();
                saved.reset();
                ip = 0;
            }
            std::lock_guard lock(mutex);
            has_saved = static_cast<bool>(saved);
            if (!saved) saved_id.clear();
            phase = saved ? "failed" : "idle";
            failure = saved ? "STORAGE_FAILED" : nullptr;
        } else if (next) {
            const char *error = "RADIO_FAILED";
            ip = 0;
            if (ready) {
                error = nullptr;
                ip = join(*next, error, false);
            }
            if (ip && save(*next)) {
                saved = std::move(next);
                set_online(saved, ip);
            } else {
                if (ip) error = "STORAGE_FAILED";
                stop_radio();
                ip = 0;
                retry_at = 0;
            }
            std::lock_guard lock(mutex);
            has_saved = static_cast<bool>(saved);
            if (saved) saved_id = saved->id;
            phase = ip ? "ready" : "failed";
            failure = error;
        } else if (ready && saved && !ip && esp_timer_get_time() >= retry_at) {
            const char *error = nullptr;
            ip = join(*saved, error, true);
            if (ip) set_online(saved, ip);
            else retry_at = esp_timer_get_time() + 3'000'000;
        }
        Event event;
        if (xQueueReceive(events, &event, pdMS_TO_TICKS(100)) != pdTRUE) continue;
        if (ip && event.type == Event::disconnected) {
            ip = 0;
            retry_at = esp_timer_get_time() + 1'000'000;
            set_online({}, 0);
        } else if (ip && event.type == Event::got_address && event.data && event.data != ip) {
            ip = event.data;
            set_online(saved, ip);
        }
    }
}

std::string ip_string(uint32_t ip) {
    if (!ip) return {};
    in_addr address{ip};
    char buffer[INET_ADDRSTRLEN]{};
    inet_ntop(AF_INET, &address, buffer, sizeof(buffer));
    return buffer;
}
}

void init(const char *device_id) {
    if (nvs_open_from_partition("displaycfg", "wifi", NVS_READWRITE, &storage) != ESP_OK) return;
    snprintf(identifier, sizeof(identifier), "%.12s", device_id);
    snprintf(host, sizeof(host), "mado-%s", identifier);
    snprintf(instance, sizeof(instance), "Mado-%s", identifier + 8);
    events = xQueueCreate(16, sizeof(Event));
    if (events && xTaskCreate(network_task, "mado_wifi", 8192, nullptr, 4, &task) != pdPASS) {
        vQueueDelete(events);
        events = nullptr;
    }
}

const char *provision(const cJSON *payload) {
    if (!task) return "RADIO_FAILED";
    auto profile = parse_profile(payload);
    if (!profile || profile->id.size() != 36) return "INVALID_PAYLOAD";
    std::lock_guard lock(mutex);
    if (phase == std::string_view("joining") || phase == std::string_view("forgetting")) return "WIFI_BUSY";
    request_id = profile->id;
    phase = "joining";
    failure = nullptr;
    candidate = std::move(profile);
    return nullptr;
}

cJSON *provision_status() {
    std::lock_guard lock(mutex);
    auto *json = cJSON_CreateObject();
    cJSON_AddStringToObject(json, "requestId", request_id.c_str());
    cJSON_AddStringToObject(json, "phase", phase);
    if (failure) cJSON_AddStringToObject(json, "error", failure);
    if (online.address) cJSON_AddStringToObject(json, "address", ip_string(online.address).c_str());
    cJSON_AddBoolToObject(json, "saved", has_saved);
    if (!saved_id.empty()) cJSON_AddStringToObject(json, "savedProfileId", saved_id.c_str());
    return json;
}

bool forget() {
    std::lock_guard lock(mutex);
    if (!task || phase == std::string_view("joining") || phase == std::string_view("forgetting")) return false;
    phase = "forgetting";
    erase_requested = true;
    return true;
}

Network active_network() {
    std::lock_guard lock(mutex);
    return online;
}

std::string address() {
    return ip_string(active_network().address);
}

bool has_saved_network() {
    std::lock_guard lock(mutex);
    return has_saved;
}
}
