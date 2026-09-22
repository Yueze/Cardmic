/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include <smooth_ui_toolkit.h>
#include <M5Unified.hpp>
#include <mooncake_log.h>
#include <mooncake.h>
#include <apps.h>
#include <hal.h>
#include <esp_ota_ops.h>

using namespace mooncake;
using namespace smooth_ui_toolkit;

// Cardmic: this firmware is for the Cardputer ADV, whose keyboard is a
// TCA8418 controller. On an original Cardputer the keyboard never answers and
// the launcher looks frozen, so say what is wrong instead. Retries every
// second (in case of a transient I2C hiccup); G0 continues anyway.
static void keyboard_missing_notice()
{
    auto& d = M5.Display;
    d.setBrightness(128);
    d.fillScreen(TFT_BLACK);
    d.setTextDatum(textdatum_t::top_left);
    d.setFont(&fonts::FreeSansBold9pt7b);
    d.setTextColor(0xFFB020);
    d.drawString("Keyboard not found", 8, 8);
    d.setFont(&fonts::DejaVu12);
    d.setTextColor(TFT_WHITE);
    const char* lines[] = {
        "This firmware is for the",
        "M5Stack Cardputer ADV only.",
        "Original Cardputer? Use",
        "M5Burner to restore it.",
    };
    for (int i = 0; i < 4; ++i) d.drawString(lines[i], 8, 34 + i * 16);
    d.setTextColor(0x99FF00);
    d.drawString("G0: continue anyway", 8, 110);

    uint32_t last_try = GetHAL().millis();
    while (true) {
        M5.update();
        if (M5.BtnA.wasClicked()) break;
        if (GetHAL().millis() - last_try > 1000) {
            last_try = GetHAL().millis();
            if (GetHAL().retryKeyboardInit()) break;
        }
        vTaskDelay(pdMS_TO_TICKS(20));
    }
    d.fillScreen(TFT_BLACK);
}

extern "C" void app_main(void)
{
    // Setup logger
    mclog::set_level(mclog::level_debug);
    mclog::set_time_format(mclog::time_format_unix_milliseconds);

    // HAL init
    GetHAL().init();

    // Reaching this point means an OTA image boots; keep it (else the
    // bootloader rolls back to the previous slot on the next reset).
    esp_ota_mark_app_valid_cancel_rollback();

    if (!GetHAL().isKeyboardReady()) {
        keyboard_missing_notice();
    }

    // Setup ui hal
    ui_hal::on_delay([](uint32_t ms) { GetHAL().delay(ms); });
    ui_hal::on_get_tick([]() { return GetHAL().millis(); });

    // Install apps
    GetMooncake().installApp(std::make_unique<Launcher>());
    GetMooncake().installApp(std::make_unique<AppCardmic>());
    GetMooncake().installApp(std::make_unique<AppWifiScan>());
    GetMooncake().installApp(std::make_unique<AppRecord>());
    GetMooncake().installApp(std::make_unique<AppChat>());
    GetMooncake().installApp(std::make_unique<AppRemote>());
    GetMooncake().installApp(std::make_unique<AppREPL>());
    GetMooncake().installApp(std::make_unique<AppSetWiFi>());
    GetMooncake().installApp(std::make_unique<AppClock>());
    GetMooncake().installApp(std::make_unique<AppKeyboard>());
    GetMooncake().installApp(std::make_unique<AppImu>());
    GetMooncake().installApp(std::make_unique<AppSdcard>());
    GetMooncake().installApp(std::make_unique<AppStringIRToolKit>());
    GetMooncake().installApp(std::make_unique<AppLoraChat>());
    GetMooncake().installApp(std::make_unique<AppGPS>());
    // GetMooncake().installApp(std::make_unique<AppDummy>());

    // Main loop
    audio::set_keyboard_sfx_enable(true);
    while (1) {
        GetHAL().feedTheDog();
        GetHAL().update();
        GetMooncake().update();
    }
}
