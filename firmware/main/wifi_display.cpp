#include "wifi_display.h"
#include "wifi_network.h"
#include "frame.h"
#include "protocol.h"

#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "esp_tls.h"
#include "freertos/FreeRTOS.h"
#include "freertos/stream_buffer.h"
#include "freertos/idf_additions.h"
#include "freertos/task.h"
#include "lwip/sockets.h"

#include <algorithm>
#include <deque>
#include <mutex>
#include <optional>
#include <string>
#include <utility>

namespace {
constexpr size_t capacity = 8192;
std::mutex mutex;
StreamBufferHandle_t incoming;
StaticStreamBuffer_t incoming_control;
std::optional<LineFrame> frame;
std::deque<std::string> outgoing;
bool connected;
bool overflow;
uint32_t generation, active_generation;

void send(std::string_view bytes) {
    std::lock_guard lock(mutex);
    if (!connected || active_generation != generation) return;
    if (outgoing.size() >= 16) overflow = true;
    else outgoing.emplace_back(bytes);
}

const DisplayTransport transport{"wifi", send};

void connection_changed(bool online) {
    std::lock_guard lock(mutex);
    connected = online;
    ++generation;
    xStreamBufferReset(incoming);
    outgoing.clear();
    overflow = false;
}

bool same_network(const wifi::Network &network) {
    auto current = wifi::active_network();
    return current.address == network.address && current.profile == network.profile;
}

bool wants_io(int result) {
    return result == ESP_TLS_ERR_SSL_WANT_READ || result == ESP_TLS_ERR_SSL_WANT_WRITE;
}

void serve(esp_tls_t *tls, const wifi::Network &network) {
    connection_changed(true);
    auto last_input = esp_timer_get_time();
    std::string pending;
    size_t sent = 0;
    for (;;) {
        if (!same_network(network) || esp_timer_get_time() - last_input >= 20'000'000) break;
        uint8_t chunk[1024];
        size_t available;
        {
            std::lock_guard lock(mutex);
            if (overflow) break;
            if (pending.empty() && !outgoing.empty()) {
                pending = std::move(outgoing.front());
                outgoing.pop_front();
                sent = 0;
            }
            available = xStreamBufferSpacesAvailable(incoming);
        }

        bool progress = false;
        if (!pending.empty()) {
            int count = esp_tls_conn_write(tls, pending.data() + sent, pending.size() - sent);
            if (count > 0) {
                sent += count;
                if (sent == pending.size()) pending.clear();
                progress = true;
            } else if (!wants_io(count)) {
                break;
            }
        }
        if (available) {
            int count = esp_tls_conn_read(tls, chunk, std::min(available, sizeof(chunk)));
            if (count > 0) {
                std::lock_guard lock(mutex);
                xStreamBufferSend(incoming, chunk, count, 0);
                last_input = esp_timer_get_time();
                progress = true;
            } else if (!wants_io(count)) {
                break;
            }
        }
        if (!progress) vTaskDelay(pdMS_TO_TICKS(10));
    }
    connection_changed(false);
}

void handle_client(int socket, const wifi::Network &network) {
    if (fcntl(socket, F_SETFL, O_NONBLOCK) < 0) return;
    const int one = 1;
    setsockopt(socket, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
    auto *tls = esp_tls_init();
    if (!tls) return;
    const auto &profile = *network.profile;
    esp_tls_cfg_server_t config{};
    config.cacert_buf = reinterpret_cast<const uint8_t *>(profile.ca.c_str());
    config.cacert_bytes = profile.ca.size() + 1;
    config.servercert_buf = reinterpret_cast<const uint8_t *>(profile.certificate.c_str());
    config.servercert_bytes = profile.certificate.size() + 1;
    config.serverkey_buf = reinterpret_cast<const uint8_t *>(profile.key.c_str());
    config.serverkey_bytes = profile.key.size() + 1;
    if (esp_tls_server_session_init(&config, socket, tls) == ESP_OK) {
        const auto deadline = esp_timer_get_time() + 5'000'000;
        while (same_network(network) && esp_timer_get_time() < deadline) {
            const auto result = esp_tls_server_session_continue_async(tls);
            if (result == 0) {
                serve(tls, network);
                break;
            }
            if (!wants_io(result)) break;
            vTaskDelay(pdMS_TO_TICKS(10));
        }
    }
    esp_tls_server_session_delete(tls);
}

void server_task(void *) {
    for (;;) {
        auto network = wifi::active_network();
        if (!network.address) {
            vTaskDelay(pdMS_TO_TICKS(200));
            continue;
        }
        const int listener = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        if (listener >= 0) {
            sockaddr_in address{};
            address.sin_family = AF_INET;
            address.sin_addr.s_addr = network.address;
            address.sin_port = htons(wifi::port);
            const int one = 1;
            setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
            if (bind(listener, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0 &&
                listen(listener, 2) == 0 && fcntl(listener, F_SETFL, O_NONBLOCK) == 0) {
                while (same_network(network)) {
                    const int client = accept(listener, nullptr, nullptr);
                    if (client >= 0) {
                        handle_client(client, network);
                        close(client);
                    } else if (errno == EWOULDBLOCK || errno == EAGAIN) {
                        vTaskDelay(pdMS_TO_TICKS(50));
                    } else {
                        break;
                    }
                }
            }
            close(listener);
        }
        vTaskDelay(pdMS_TO_TICKS(200));
    }
}
}

bool wifi_display_init() {
    auto *line = static_cast<char *>(heap_caps_malloc(message_limit + 1, MALLOC_CAP_SPIRAM));
    auto *buffer = static_cast<uint8_t *>(heap_caps_malloc(capacity + 1, MALLOC_CAP_SPIRAM));
    if (!line || !buffer) {
        heap_caps_free(line);
        heap_caps_free(buffer);
        return false;
    }
    frame.emplace(line, message_limit);
    incoming = xStreamBufferCreateStatic(capacity + 1, 1, buffer, &incoming_control);
    // TLS handles sockets only; the network task keeps an internal stack for NVS writes.
    if (xTaskCreateWithCaps(server_task, "mado_tls", 8192, nullptr, 4, nullptr,
                           MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT) == pdPASS) return true;
    incoming = nullptr;
    frame.reset();
    heap_caps_free(line);
    heap_caps_free(buffer);
    return false;
}

void wifi_display_poll() {
    if (!incoming) return;
    uint8_t chunk[2048];
    uint32_t epoch;
    size_t size;
    {
        std::lock_guard lock(mutex);
        epoch = generation;
        size = xStreamBufferReceive(incoming, chunk, sizeof(chunk), 0);
    }
    if (active_generation != epoch) {
        active_generation = epoch;
        frame->reset();
        protocol_disconnect(transport);
    }
    for (size_t i = 0; i < size; ++i) {
        if (const char *line = frame->push(chunk[i])) protocol_receive(transport, line);
    }
    if (size && frame->pending()) protocol_transport_progress(transport);
}
