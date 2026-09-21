/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_wifi_scan.h"
#include "assets/scan_big.h"
#include "assets/scan_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>

using namespace mooncake;

AppWifiScan::AppWifiScan()
{
    setAppInfo().name     = "Scan";
    setAppInfo().userData = new AppIcon_t(image_data_scan_big, image_data_scan_small);
    _time_count           = 0;
}

AppWifiScan::~AppWifiScan()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppWifiScan::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    GetHAL().wifiInit();
    render_page_scanning();
}

void AppWifiScan::onRunning()
{
    // Scan in every 6 seconds
    if (GetHAL().millis() - _time_count > 5000) {
        GetHAL().wifiScan(_scan_result);
        render_page_result();
        _time_count = GetHAL().millis();
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppWifiScan::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");
}

void AppWifiScan::render_page_scanning()
{
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setTextScroll(false);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setCursor(10, 5);
    GetHAL().canvas.setTextColor(TFT_ORANGE, THEME_COLOR_BG);
    GetHAL().canvas.println("Scanning WiFi...");
    GetHAL().pushCanvas();
}

void AppWifiScan::render_page_result()
{
    // Clear screen and display results
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setCursor(0, 0);
    GetHAL().canvas.setTextSize(1);

    if (_scan_result.empty()) {
        GetHAL().canvas.setTextColor(TFT_RED, THEME_COLOR_BG);
        GetHAL().canvas.println("No WiFi networks found");
    } else {
        int max_count = 0;
        for (const auto& result : _scan_result) {
            max_count++;
            if (max_count > 6) {
                break;
            }

            mclog::tagInfo(getAppInfo().name, "found: {} {}", result.second, result.first);

            // Set color based on signal strength
            if (result.first > -60) {
                GetHAL().canvas.setTextColor(TFT_GREEN, THEME_COLOR_BG);
            } else if (result.first > -70) {
                GetHAL().canvas.setTextColor(TFT_YELLOW, THEME_COLOR_BG);
            } else {
                GetHAL().canvas.setTextColor(TFT_RED, THEME_COLOR_BG);
            }

            GetHAL().canvas.printf("%d ", result.first);
            GetHAL().canvas.println(result.second.c_str());
        }
    }

    GetHAL().pushCanvas();
}
