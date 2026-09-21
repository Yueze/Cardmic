/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_gps.h"
#include "assets/gps_big.h"
#include "assets/gps_small.h"
#include "assets/icon_satellite.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>
#include <sys/time.h>
#include <time.h>
#include <hal.h>

using namespace mooncake;

AppGPS::AppGPS()
{
    setAppInfo().name     = "GPS";
    setAppInfo().userData = new AppIcon_t(image_data_gps_big, image_data_gps_small);
}

AppGPS::~AppGPS()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppGPS::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Clear screen
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setTextScroll(true);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setCursor(0, 0);

    GetHAL().canvas.setTextColor(TFT_ORANGE);
    GetHAL().canvas.printf("Init Cap LoRa868...\n");
    GetHAL().pushCanvas();
    _is_cap_available = GetHAL().capLora868.init();
    if (!_is_cap_available) {
        mclog::tagError(getAppInfo().name, "cap not available");

        GetHAL().canvas.setTextColor(TFT_ORANGE);
        GetHAL().canvas.printf("Cap init failed.\n");
        GetHAL().canvas.setTextColor(TFT_CYAN);
        GetHAL().canvas.printf("Cap LoRa868 ");
        GetHAL().canvas.setTextColor(TFT_WHITE);
        GetHAL().canvas.printf("is required.\nPlease connect it\nto continue.");

        GetHAL().pushCanvas();
    }

    _time_count      = 0;
    _sync_time_count = 0;
}

void AppGPS::onRunning()
{
    if (_is_cap_available) {
        render_gps_data();
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppGPS::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");
}

void AppGPS::render_gps_data()
{
    if (GetHAL().millis() - _time_count > 1000) {
        GetHAL().canvas.fillScreen(THEME_COLOR_BG);
        GetHAL().canvas.setFont(FONT_REPL);
        GetHAL().canvas.setTextSize(1);

        auto gps = GetHAL().capLora868.borrowGPS();
        if (gps) {
            if (gps->date.isValid()) {
                mclog::debug("gps date: {}/{}/{}", gps->date.year(), gps->date.month(), gps->date.day());
            }
            if (gps->time.isValid()) {
                mclog::debug("gps time: {}:{}:{}", gps->time.hour(), gps->time.minute(), gps->time.second());
            }
            if (gps->location.isValid()) {
                mclog::debug("gps location: {:.6f},{:.6f}", gps->location.lat(), gps->location.lng());
            }

            std::string buffer;

            // Speed
            buffer = "..";
            if (gps->speed.isValid()) {
                buffer = fmt::format("{:.1f} km/h", gps->speed.kmph());
            }
            GetHAL().canvas.fillRect(2, 7, 3, 5, (uint32_t)0xB3B6DD);
            GetHAL().canvas.setTextColor((uint32_t)0xB3B6DD);
            GetHAL().canvas.setTextDatum(textdatum_t::middle_left);
            GetHAL().canvas.drawString(buffer.c_str(), 12, 9);

            // Satellite
            buffer = "..";
            if (gps->satellites.isValid()) {
                buffer = fmt::format("{}", gps->satellites.value());
            }
            GetHAL().canvas.setTextColor((uint32_t)0x7EF8B0);
            GetHAL().canvas.setTextDatum(textdatum_t::middle_right);
            GetHAL().canvas.drawString(buffer.c_str(), 175, 9);
            GetHAL().canvas.pushImage(178, -1, 20, 20, image_data_icon_satellite);

            // Divider
            GetHAL().canvas.fillRect(0, 20, 198, 1, (uint32_t)0xA2A1A1);

            // Location
            GetHAL().canvas.setTextSize(1.5);
            if (gps->location.isValid()) {
                GetHAL().canvas.fillRect(2, 35, 3, 5, (uint32_t)0x60FFF5);
                GetHAL().canvas.fillRect(2, 64, 3, 5, (uint32_t)0x60FFF5);

                GetHAL().canvas.setTextColor((uint32_t)0x60FFF5);
                GetHAL().canvas.setTextDatum(textdatum_t::middle_left);

                buffer = fmt::format("{:.6f} {}", gps->location.lat(), gps->location.lat() > 0 ? "N" : "S");
                GetHAL().canvas.drawString(buffer.c_str(), 12, 37);

                buffer = fmt::format("{:.6f} {}", gps->location.lng(), gps->location.lng() > 0 ? "E" : "W");
                GetHAL().canvas.drawString(buffer.c_str(), 12, 66);
            } else {
                GetHAL().canvas.setTextColor((uint32_t)0xFFFEB4);
                GetHAL().canvas.setTextDatum(textdatum_t::middle_center);
                GetHAL().canvas.drawString("No GPS Fix", 98, 52);
            }

            // Divider
            GetHAL().canvas.fillRect(0, 83, 198, 1, (uint32_t)0xA2A1A1);

            // Date time
            GetHAL().canvas.setTextSize(1);
            buffer = "..";
            if (gps->date.isValid() && gps->time.isValid()) {
                buffer = fmt::format("{}.{}.{} / {:02d}:{:02d}:{:02d}", gps->date.year(), gps->date.month(),
                                     gps->date.day(), gps->time.hour(), gps->time.minute(), gps->time.second());

                sync_gps_time_to_system(gps);
            }
            GetHAL().canvas.fillRect(2, 93, 3, 5, (uint32_t)0x95AF8F);
            GetHAL().canvas.setTextColor((uint32_t)0x95AF8F);
            GetHAL().canvas.setTextDatum(textdatum_t::middle_left);
            GetHAL().canvas.drawString(buffer.c_str(), 12, 95);

            // Reset
            GetHAL().canvas.setTextDatum(textdatum_t::top_left);

            GetHAL().capLora868.returnGPS();

        } else {
            GetHAL().canvas.setCursor(0, 0);
            GetHAL().canvas.setTextColor(TFT_RED);
            GetHAL().canvas.println("gps not available");
            mclog::tagError(getAppInfo().name, "gps not available");
        }

        GetHAL().pushCanvas();

        _time_count = GetHAL().millis();
    }
}

void AppGPS::sync_gps_time_to_system(TinyGPSPlus* gps)
{
    if (GetHAL().isWifiConnected()) {
        return;
    }

    if (GetHAL().millis() - _sync_time_count < 5000) {
        return;
    }

    mclog::tagInfo(getAppInfo().name, "sync gps time to system");

    struct tm tm_time;
    memset(&tm_time, 0, sizeof(tm_time));

    tm_time.tm_year = gps->date.year() - 1900;
    tm_time.tm_mon  = gps->date.month() - 1;
    tm_time.tm_mday = gps->date.day();
    tm_time.tm_hour = gps->time.hour();
    tm_time.tm_min  = gps->time.minute();
    tm_time.tm_sec  = gps->time.second();

    time_t t = mktime(&tm_time);

    struct timeval now;
    now.tv_sec  = t;
    now.tv_usec = 0;

    if (settimeofday(&now, nullptr) != 0) {
        mclog::tagError(getAppInfo().name, "settimeofday failed");
    }

    _sync_time_count = GetHAL().millis();
}
