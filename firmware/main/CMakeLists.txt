idf_component_register(SRCS "main.cpp" "protocol.cpp" "display.cpp" "font.cpp" "ble_display.cpp" "wifi_display.cpp" "wifi_network.cpp"
    INCLUDE_DIRS "."
    REQUIRES bt esp_wifi esp_event esp_netif esp-tls lwip esp_partition json nvs_flash esp_timer esp_lcd esp_driver_gpio esp_driver_i2c
             esp_driver_spi esp_driver_ledc esp_driver_usb_serial_jtag)
target_compile_features(${COMPONENT_LIB} PRIVATE cxx_std_17)

set(FONT_VENDOR "${CMAKE_CURRENT_LIST_DIR}/../vendor")
if(NOT EXISTS "${FONT_VENDOR}/freetype/CMakeLists.txt" OR
   NOT EXISTS "${FONT_VENDOR}/brotli/c/include/brotli/decode.h")
    message(FATAL_ERROR "运行 firmware/setup-resources.sh 获取锁定的 FreeType 与 Brotli 源码")
endif()
file(GLOB BROTLI_SOURCES CONFIGURE_DEPENDS "${FONT_VENDOR}/brotli/c/common/*.c" "${FONT_VENDOR}/brotli/c/dec/*.c")
add_library(tdisplay_brotli STATIC ${BROTLI_SOURCES})
target_include_directories(tdisplay_brotli PUBLIC "${FONT_VENDOR}/brotli/c/include")
target_compile_options(tdisplay_brotli PRIVATE -Os -ffunction-sections -fdata-sections)
foreach(dep ZLIB BZIP2 PNG HARFBUZZ BROTLI)
    set(FT_DISABLE_${dep} ON CACHE BOOL "" FORCE)
endforeach()
set(SKIP_INSTALL_ALL ON)
add_subdirectory("${FONT_VENDOR}/freetype" "${CMAKE_CURRENT_BINARY_DIR}/freetype" EXCLUDE_FROM_ALL)
target_compile_definitions(freetype PRIVATE FT_CONFIG_OPTION_USE_BROTLI)
target_compile_options(freetype PRIVATE -Os -ffunction-sections -fdata-sections)
target_link_libraries(freetype PRIVATE tdisplay_brotli)
target_link_libraries(${COMPONENT_LIB} PRIVATE freetype)
