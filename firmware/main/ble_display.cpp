#include "ble_display.h"
#include "frame.h"
#include "protocol.h"

#include "esp_heap_caps.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "esp_random.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/stream_buffer.h"
#include "host/ble_att.h"
#include "host/ble_gap.h"
#include "host/ble_gatt.h"
#include "host/ble_hs.h"
#include "host/ble_hs_mbuf.h"
#include "host/util/util.h"
#include "host/ble_sm.h"
#include "host/ble_store.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"

#include <algorithm>
#include <cstdio>
#include <deque>
#include <mutex>
#include <optional>
#include <string>
#include <utility>
#include <cstring>

extern "C" void ble_store_config_init(void);

namespace {
constexpr auto no_connection = BLE_HS_CONN_HANDLE_NONE;
constexpr auto tag = "mado_ble";
constexpr size_t rx_capacity = 8192;
constexpr int64_t pair_duration_us = 120'000'000;

// UUIDs 4e780001..4e780004-704f-4fd7-a0dc-2c0fad1e48ad
#define MADO_UUID(id) BLE_UUID128_INIT(0xad, 0x48, 0x1e, 0xad, 0x0f, 0x2c, 0xdc, 0xa0, \
                                       0xd7, 0x4f, 0x4f, 0x70, id, 0x00, 0x78, 0x4e)
const ble_uuid128_t service_id = MADO_UUID(1);
const ble_uuid128_t rx_id = MADO_UUID(2);
const ble_uuid128_t tx_id = MADO_UUID(3);
const ble_uuid128_t info_id = MADO_UUID(4);
#undef MADO_UUID

std::mutex mutex;
std::optional<LineFrame> frame;
StreamBufferHandle_t incoming;
StaticStreamBuffer_t incoming_control;
uint32_t generation;
uint16_t connection = no_connection, tx_handle;
bool authenticated;
bool subscribed;
bool ready;
bool forget_requested;
int passkey = -1;
int64_t pair_until, fragment_at;
uint8_t address_type;
char name[16];
ble_npl_event refresh_event, tx_event;

uint32_t active_generation;
std::deque<std::string> outgoing;
size_t sent_bytes;
bool tx_overflow;

bool pairing() { return passkey >= 0 && esp_timer_get_time() < pair_until; }

bool read_bond(const ble_addr_t &peer, ble_store_value_sec &value) {
    ble_store_key_sec key{};
    key.peer_addr = peer;
    return ble_store_read_peer_sec(&key, &value) == 0;
}

bool trusted_bond(const ble_store_value_sec &value) {
    return value.authenticated && value.sc && value.ltk_present && value.key_size == 16;
}

bool known_peer(uint16_t handle) {
    ble_gap_conn_desc desc{};
    ble_store_value_sec value{};
    return ble_gap_conn_find(handle, &desc) == 0 &&
           read_bond(desc.peer_id_addr, value) && trusted_bond(value);
}

bool confirm_pairing(uint16_t handle) {
    ble_gap_conn_desc desc{};
    ble_store_value_sec value{};
    if (ble_gap_conn_find(handle, &desc) != 0 || !read_bond(desc.peer_id_addr, value)) return false;
    if (!trusted_bond(value)) {
        ble_gap_unpair(&desc.peer_id_addr);
        return false;
    }
    return desc.sec_state.encrypted && desc.sec_state.authenticated && desc.sec_state.bonded;
}

int access(uint16_t handle, uint16_t, ble_gatt_access_ctxt *ctx, void *) {
    std::lock_guard lock(mutex);
    if (handle != connection || !authenticated) return BLE_ATT_ERR_INSUFFICIENT_AUTHEN;
    if (ctx->op == BLE_GATT_ACCESS_OP_READ_CHR) {
        auto mtu = ble_att_mtu(handle);
        uint8_t result[] = {static_cast<uint8_t>(mtu), static_cast<uint8_t>(mtu >> 8)};
        return os_mbuf_append(ctx->om, result, sizeof(result)) == 0 ? 0 : BLE_ATT_ERR_INSUFFICIENT_RES;
    }
    if (ctx->op != BLE_GATT_ACCESS_OP_WRITE_CHR || ble_uuid_cmp(ctx->chr->uuid, &rx_id.u))
        return BLE_ATT_ERR_UNLIKELY;
    uint8_t chunk[512];
    uint16_t size;
    if (ble_hs_mbuf_to_flat(ctx->om, chunk, sizeof(chunk), &size) != 0)
        return BLE_ATT_ERR_INVALID_ATTR_VALUE_LEN;
    if (size > xStreamBufferSpacesAvailable(incoming))
        return BLE_ATT_ERR_INSUFFICIENT_RES;
    xStreamBufferSend(incoming, chunk, size, 0);
    fragment_at = esp_timer_get_time();
    return 0;
}

int gap_event(ble_gap_event *event, void *);
void advertise() {
    bool open;
    {
        std::lock_guard lock(mutex);
        if (!ready || connection != no_connection) return;
        open = pairing();
    }
    int bonds = 0;
    if (ble_store_util_count(BLE_STORE_OBJ_TYPE_PEER_SEC, &bonds) != 0) return;
    if (!open && !bonds) {
        if (ble_gap_adv_active()) ble_gap_adv_stop();
        return;
    }
    if (ble_gap_adv_active()) return;

    ble_hs_adv_fields fields{};
    fields.flags = BLE_HS_ADV_F_DISC_GEN | BLE_HS_ADV_F_BREDR_UNSUP;
    fields.uuids128 = &service_id;
    fields.num_uuids128 = 1;
    fields.uuids128_is_complete = 1;
    if (ble_gap_adv_set_fields(&fields) != 0) return;
    ble_hs_adv_fields response{};
    response.name = reinterpret_cast<const uint8_t *>(name);
    response.name_len = strlen(name);
    response.name_is_complete = 1;
    if (ble_gap_adv_rsp_set_fields(&response) != 0) return;
    ble_gap_adv_params params{};
    params.conn_mode = BLE_GAP_CONN_MODE_UND;
    params.disc_mode = BLE_GAP_DISC_MODE_GEN;
    int rc = ble_gap_adv_start(address_type, nullptr, BLE_HS_FOREVER, &params, gap_event, nullptr);
    if (rc != 0) ESP_LOGW(tag, "advertise: %d", rc);
}

int gap_event(ble_gap_event *event, void *) {
    switch (event->type) {
    case BLE_GAP_EVENT_CONNECT: {
        if (event->connect.status != 0) { advertise(); break; }
        auto handle = event->connect.conn_handle;
        bool open;
        {
            std::lock_guard lock(mutex);
            connection = handle;
            ++generation;
            open = pairing();
        }
        if (!open && !known_peer(handle)) ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
        break;
    }
    case BLE_GAP_EVENT_DISCONNECT: {
        {
            std::lock_guard lock(mutex);
            connection = no_connection;
            ++generation;
            authenticated = subscribed = false;
            xStreamBufferReset(incoming);
            outgoing.clear();
            sent_bytes = 0;
            tx_overflow = false;
        }
        advertise();
        break;
    }
    case BLE_GAP_EVENT_ADV_COMPLETE:
        advertise();
        break;
    case BLE_GAP_EVENT_SUBSCRIBE:
        if (event->subscribe.attr_handle == tx_handle) {
            std::lock_guard lock(mutex);
            subscribed = event->subscribe.cur_notify;
        }
        break;
    case BLE_GAP_EVENT_ENC_CHANGE: {
        auto handle = event->enc_change.conn_handle;
        bool secure = event->enc_change.status == 0 && confirm_pairing(handle);
        {
            std::lock_guard lock(mutex);
            authenticated = secure;
            if (secure) passkey = -1;
        }
        if (!secure) ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
        break;
    }
    case BLE_GAP_EVENT_PASSKEY_ACTION: {
        int code;
        {
            std::lock_guard lock(mutex);
            code = pairing() ? passkey : -1;
        }
        auto handle = event->passkey.conn_handle;
        if (code < 0 || event->passkey.params.action != BLE_SM_IOACT_DISP) {
            ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
            break;
        }
        ble_sm_io io{};
        io.action = BLE_SM_IOACT_DISP;
        io.passkey = code;
        if (ble_sm_inject_io(handle, &io) != 0) ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
        break;
    }
    case BLE_GAP_EVENT_REPEAT_PAIRING: {
        {
            std::lock_guard lock(mutex);
            if (!pairing()) return BLE_GAP_REPEAT_PAIRING_IGNORE;
        }
        ble_gap_conn_desc desc{};
        if (ble_gap_conn_find(event->repeat_pairing.conn_handle, &desc) != 0 ||
            ble_store_util_delete_peer(&desc.peer_id_addr) != 0)
            return BLE_GAP_REPEAT_PAIRING_IGNORE;
        return BLE_GAP_REPEAT_PAIRING_RETRY;
    }
    }
    return 0;
}

void refresh(ble_npl_event *) {
    bool forget;
    uint16_t handle;
    {
        std::lock_guard lock(mutex);
        forget = std::exchange(forget_requested, false);
        handle = !pairing() && (!authenticated || forget) ? connection : no_connection;
    }
    if (handle != no_connection) ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
    if (forget) {
        if (ble_gap_adv_active()) ble_gap_adv_stop();
        ble_addr_t peers[CONFIG_BT_NIMBLE_MAX_BONDS];
        int count = 0;
        int rc = ble_store_util_bonded_peers(peers, &count, CONFIG_BT_NIMBLE_MAX_BONDS);
        if (rc == 0) {
            for (int i = 0; i < count; ++i) {
                rc = ble_gap_unpair(&peers[i]);
                if (rc != 0) ESP_LOGW(tag, "unpair: %d", rc);
            }
        } else {
            ESP_LOGW(tag, "list bonds: %d", rc);
        }
    }
    advertise();
}

void on_sync() {
    if (ble_hs_util_ensure_addr(0) != 0 || ble_hs_id_infer_auto(0, &address_type) != 0) return;
    {
        std::lock_guard lock(mutex);
        ready = true;
    }
    advertise();
}

void reset(int) {
    std::lock_guard lock(mutex);
    ready = false;
    connection = no_connection;
    ++generation;
    authenticated = subscribed = false;
    xStreamBufferReset(incoming);
    outgoing.clear();
    sent_bytes = 0;
    tx_overflow = false;
}

void host_task(void *) {
    nimble_port_run();
    nimble_port_freertos_deinit();
}

void schedule_refresh() {
    ble_npl_eventq_put(nimble_port_get_dflt_eventq(), &refresh_event);
}

void send(std::string_view wire) {
    std::lock_guard lock(mutex);
    if (active_generation != generation || connection == no_connection) return;
    if (outgoing.size() >= 8) tx_overflow = true;
    else outgoing.emplace_back(wire);
}

void transmit(ble_npl_event *) {
    uint16_t handle;
    bool overflow;
    {
        std::lock_guard lock(mutex);
        if (connection == no_connection || !authenticated || !subscribed || outgoing.empty()) return;
        handle = connection;
        overflow = tx_overflow;
    }
    if (overflow) {
        ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
        return;
    }
    const uint16_t mtu = ble_att_mtu(handle);
    if (mtu <= 3) return;
    for (int i = 0; i < 4; ++i) {
        os_mbuf *mbuf;
        size_t length;
        {
            std::lock_guard lock(mutex);
            if (outgoing.empty()) break;
            const auto &wire = outgoing.front();
            length = std::min<size_t>(mtu - 3, wire.size() - sent_bytes);
            mbuf = ble_hs_mbuf_from_flat(wire.data() + sent_bytes, length);
        }
        if (!mbuf) break;
        int rc = ble_gatts_notify_custom(handle, tx_handle, mbuf);
        if (rc != 0) {
            if (rc != BLE_HS_ENOMEM && rc != BLE_HS_EBUSY)
                ble_gap_terminate(handle, BLE_ERR_REM_USER_CONN_TERM);
            break;
        }
        std::lock_guard lock(mutex);
        sent_bytes += length;
        if (sent_bytes == outgoing.front().size()) {
            outgoing.pop_front();
            sent_bytes = 0;
        }
    }
}
}

const DisplayTransport ble_transport{"ble", send};

bool ble_display_init() {
    auto *buffer = static_cast<char *>(heap_caps_malloc(message_limit + 1, MALLOC_CAP_SPIRAM));
    auto *rx_bytes = static_cast<uint8_t *>(heap_caps_malloc(rx_capacity + 1, MALLOC_CAP_SPIRAM));
    if (!buffer || !rx_bytes) {
        heap_caps_free(buffer);
        heap_caps_free(rx_bytes);
        return false;
    }
    incoming = xStreamBufferCreateStatic(rx_capacity + 1, 1, rx_bytes, &incoming_control);
    frame.emplace(buffer, message_limit);
    if (nimble_port_init() != ESP_OK) {
        frame.reset();
        heap_caps_free(buffer);
        heap_caps_free(rx_bytes);
        incoming = nullptr;
        return false;
    }
    uint8_t mac[6]{};
    esp_read_mac(mac, ESP_MAC_BT);
    snprintf(name, sizeof(name), "Mado-%02X%02X", mac[4], mac[5]);

    static ble_gatt_chr_def chars[4]{};
    chars[0].uuid = &rx_id.u;
    chars[0].access_cb = access;
    chars[0].flags = BLE_GATT_CHR_F_WRITE | BLE_GATT_CHR_F_WRITE_ENC | BLE_GATT_CHR_F_WRITE_AUTHEN;
    chars[1].uuid = &tx_id.u;
    chars[1].access_cb = access;
    chars[1].flags = BLE_GATT_CHR_F_NOTIFY;
    chars[1].val_handle = &tx_handle;
    chars[2].uuid = &info_id.u;
    chars[2].access_cb = access;
    chars[2].flags = BLE_GATT_CHR_F_READ | BLE_GATT_CHR_F_READ_ENC | BLE_GATT_CHR_F_READ_AUTHEN;
    static ble_gatt_svc_def services[2]{};
    services[0].type = BLE_GATT_SVC_TYPE_PRIMARY;
    services[0].uuid = &service_id.u;
    services[0].characteristics = chars;
    if (ble_gatts_count_cfg(services) != 0 || ble_gatts_add_svcs(services) != 0) return false;
    ble_hs_cfg.sync_cb = on_sync;
    ble_hs_cfg.reset_cb = reset;
    ble_hs_cfg.sm_io_cap = BLE_SM_IO_CAP_DISP_ONLY;
    ble_hs_cfg.sm_bonding = ble_hs_cfg.sm_mitm = ble_hs_cfg.sm_sc = 1;
    ble_hs_cfg.sm_our_key_dist = BLE_SM_PAIR_KEY_DIST_ENC | BLE_SM_PAIR_KEY_DIST_ID;
    ble_hs_cfg.sm_their_key_dist = BLE_SM_PAIR_KEY_DIST_ENC | BLE_SM_PAIR_KEY_DIST_ID;
    ble_store_config_init();
    ble_npl_event_init(&refresh_event, refresh, nullptr);
    ble_npl_event_init(&tx_event, transmit, nullptr);
    nimble_port_freertos_init(host_task);
    return true;
}

void ble_display_poll() {
    uint32_t epoch;
    bool expired = false;
    bool can_send;
    bool recent_fragment;
    uint8_t bytes[1024];
    size_t size = 0;
    {
        std::lock_guard lock(mutex);
        if (passkey >= 0 && !pairing()) {
            passkey = -1;
            expired = true;
        }
        epoch = generation;
        can_send = authenticated && subscribed && !outgoing.empty();
        recent_fragment = authenticated && esp_timer_get_time() - fragment_at < 2'000'000;
        if (incoming) size = xStreamBufferReceive(incoming, bytes, sizeof(bytes), 0);
    }
    if (expired) schedule_refresh();
    if (active_generation != epoch) {
        protocol_disconnect(ble_transport);
        active_generation = epoch;
        frame->reset();
    }
    for (size_t i = 0; i < size; ++i) {
        if (const char *line = frame->push(bytes[i])) protocol_receive(ble_transport, line);
    }
    if (recent_fragment && frame->pending()) protocol_transport_progress(ble_transport);
    if (can_send || size) ble_npl_eventq_put(nimble_port_get_dflt_eventq(), &tx_event);
}

bool ble_pair_open() {
    {
        std::lock_guard lock(mutex);
        if (!ready || authenticated) return false;
        if (pairing()) return true;
        passkey = esp_random() % 1'000'000;
        pair_until = esp_timer_get_time() + pair_duration_us;
    }
    schedule_refresh();
    return true;
}

void ble_pair_close() {
    {
        std::lock_guard lock(mutex);
        if (passkey < 0) return;
        passkey = -1;
    }
    schedule_refresh();
}

void ble_forget_peers() {
    {
        std::lock_guard lock(mutex);
        if (!ready) return;
        passkey = -1;
        forget_requested = true;
    }
    schedule_refresh();
}

int ble_pair_code() {
    std::lock_guard lock(mutex);
    return pairing() ? passkey : -1;
}

bool ble_display_connected() {
    std::lock_guard lock(mutex);
    return authenticated;
}
