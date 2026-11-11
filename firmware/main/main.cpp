#include "display.h"
#include "protocol.h"
#include "driver/gpio.h"
#include "driver/i2c_master.h"
#include "driver/usb_serial_jtag.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "nvs_flash.h"
#include <cstdlib>

namespace {
i2c_master_dev_handle_t bmu;
settings::Menu device_menu;
constexpr auto user_pin = GPIO_NUM_0, boot_pin = GPIO_NUM_28;

struct KeyEvent {
    settings::Press press;
    bool boot;
};

struct Keys {
    settings::Button user, boot;
    QueueHandle_t events;
};

// 独立采样，首次解码字形或接收 USB 大包时也能捕获完整的按压。
void sample_keys(void *context) {
    auto &keys = *static_cast<Keys *>(context);
    auto now = esp_timer_get_time() / 1000;
    KeyEvent user{keys.user.update(gpio_get_level(user_pin) == 0, now), false};
    KeyEvent boot{keys.boot.update(gpio_get_level(boot_pin) == 0, now, true), true};
    if (boot.press != settings::Press::none) xQueueSend(keys.events, &boot, 0);
    if (user.press != settings::Press::none) xQueueSend(keys.events, &user, 0);
}

bool read_power(uint8_t reg, uint8_t *value, size_t size) {
    return bmu && i2c_master_transmit_receive(bmu, &reg, 1, value, size, 10) == ESP_OK;
}
}

extern "C" void app_main() {
    ESP_ERROR_CHECK(nvs_flash_init());
    protocol_init();
    unsigned applied_brightness = protocol_brightness();
    display_init(applied_brightness, protocol_screen_rotated());
    display_startup(protocol_state());
    gpio_config_t key{};
    key.pin_bit_mask = (1ULL << user_pin) | (1ULL << boot_pin);
    key.mode = GPIO_MODE_INPUT;
    key.pull_up_en = GPIO_PULLUP_ENABLE;
    ESP_ERROR_CHECK(gpio_config(&key));
    Keys keys{settings::Button(gpio_get_level(user_pin) == 0),
              settings::Button(gpio_get_level(boot_pin) == 0),
              xQueueCreate(16, sizeof(KeyEvent))};
    if (!keys.events) abort();
    esp_timer_create_args_t key_timer_args{};
    key_timer_args.callback = sample_keys;
    key_timer_args.arg = &keys;
    key_timer_args.name = "device_keys";
    esp_timer_handle_t key_timer;
    ESP_ERROR_CHECK(esp_timer_create(&key_timer_args, &key_timer));
    ESP_ERROR_CHECK(esp_timer_start_periodic(key_timer, 10000));

    i2c_master_bus_handle_t bus{};
    i2c_master_bus_config_t config{};
    config.i2c_port = I2C_NUM_0;
    config.sda_io_num = GPIO_NUM_2;
    config.scl_io_num = GPIO_NUM_3;
    config.clk_source = I2C_CLK_SRC_DEFAULT;
    config.glitch_ignore_cnt = 7;
    config.flags.enable_internal_pullup = true;
    if (i2c_new_master_bus(&config, &bus) == ESP_OK &&
        i2c_master_probe(bus, 0x62, 10) == ESP_OK) {
        i2c_device_config_t device{};
        device.dev_addr_length = I2C_ADDR_BIT_LEN_7;
        device.device_address = 0x62;
        device.scl_speed_hz = 100000;
        if (i2c_master_bus_add_device(bus, &device, &bmu) == ESP_OK) {
            uint8_t id = 0, mode = 0;
            if (!read_power(0x00, &id, 1) || id != 0x1c) bmu = nullptr;
            else if (read_power(0x02, &mode, 1) && (mode & 1)) {
                uint8_t wake[] = {0x02, static_cast<uint8_t>(mode & ~1)};
                i2c_master_transmit(bmu, wake, sizeof(wake), 10);
            }
        }
    }

    // 驱动在环形缓冲满时丢弃 USB OUT 数据，需容纳完整的最大协议帧。
    usb_serial_jtag_driver_config_t usb{.tx_buffer_size = 4096, .rx_buffer_size = message_limit + 128};
    ESP_ERROR_CHECK(usb_serial_jtag_driver_install(&usb));
    auto *line = static_cast<char *>(heap_caps_malloc(message_limit + 1, MALLOC_CAP_SPIRAM));
    if (!line) abort();
    // 等待本地字形与首帧合成，USB 连接和实时读数不参与启动门槛。
    display_prepare(protocol_state());
    display_draw(protocol_state(), device_menu, protocol_brightness(), protocol_screen_rotated());
    size_t size = 0;
    bool overflow = false;
    uint64_t last_power = 0;
    constexpr uint64_t frame_us = 16667;
    uint64_t next_frame = esp_timer_get_time() + frame_us;
    for (;;) {
        uint8_t buffer[1024];
        int count = usb_serial_jtag_read_bytes(buffer, sizeof(buffer), pdMS_TO_TICKS(1));
        for (int i = 0; i < count; ++i) {
            if (buffer[i] == '\n') {
                if (!overflow && size > 0) {
                    line[size] = '\0';
                    protocol_receive(line);
                }
                size = 0;
                overflow = false;
            } else if (!overflow) {
                if (size == message_limit || buffer[i] == 0) overflow = true;
                else line[size++] = buffer[i];
            }
        }
        // USB 连续到达时先读完串口，避免冷字体解码阻塞接收。
        if (count > 0 && (size != 0 || overflow)) continue;
        uint64_t now = esp_timer_get_time() / 1000;
        KeyEvent event;
        while (xQueueReceive(keys.events, &event, 0) == pdTRUE) {
            auto user = event.boot ? settings::Press::none : event.press;
            auto boot = event.boot ? event.press : settings::Press::none;
            auto action = device_menu.update(user, boot, protocol_brightness());
            if (action == settings::Action::next_page) protocol_next_page();
            if (action == settings::Action::save)
                device_menu.saved(protocol_save_brightness(device_menu.brightness));
            if (action == settings::Action::toggle_rotation)
                device_menu.save_failed = !protocol_save_screen_rotation(!protocol_screen_rotated());
        }
        device_menu.update(settings::Press::none, settings::Press::none, protocol_brightness());
        auto brightness = device_menu.output_brightness(protocol_brightness());
        if (brightness != applied_brightness) {
            display_brightness(brightness);
            applied_brightness = brightness;
        }
        if (now - last_power >= 1000) {
            uint8_t soc = 0, voltage[2]{}, current[2]{};
            bool connected = usb_serial_jtag_is_connected();
            bool has_battery = read_power(0x04, voltage, 2) && (((voltage[0] & 0x3f) << 8) | voltage[1]) >= 2500;
            bool charging = has_battery && read_power(0x14, current, 2) &&
                            static_cast<int16_t>((current[0] << 8) | current[1]) > 0;
            int percent = has_battery && read_power(0x08, &soc, 1) && soc <= 100 ? soc : -1;
            protocol_power(percent, connected || has_battery, connected || charging, charging);
            last_power = now;
        }
        protocol_tick(device_menu.active());
        uint64_t frame_time = esp_timer_get_time();
        if (frame_time >= next_frame) {
            next_frame += ((frame_time - next_frame) / frame_us + 1) * frame_us;
            display_draw(protocol_state(), device_menu, protocol_brightness(), protocol_screen_rotated());
        }
        display_tick();
    }
}
