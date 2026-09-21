/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_sdcard.h"
#include "assets/tf_big.h"
#include "assets/tf_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>
#include <hal.h>

using namespace mooncake;

AppSdcard::AppSdcard()
{
    setAppInfo().name     = "SDCard";
    setAppInfo().userData = new AppIcon_t(image_data_tf_big, image_data_tf_small);
}

AppSdcard::~AppSdcard()
{
    // delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppSdcard::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Clear screen
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setFont(FONT_REPL);
    GetHAL().canvas.setTextScroll(true);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setCursor(0, 0);

    probe_sd_card();
    _time_count = GetHAL().millis();
}

void AppSdcard::onRunning()
{
    if (GetHAL().millis() - _time_count > 2000) {
        probe_sd_card();
        _time_count = GetHAL().millis();
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppSdcard::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");
}

void AppSdcard::probe_sd_card()
{
    mclog::tagInfo(getAppInfo().name, "probe sd card");

    auto result = GetHAL().sdCardProbe();
    // mclog::tagDebug(getAppInfo().name, "sd card probe result:\n  is_mounted: {}\n  name: {}\n  size: {}\n  type: {}",
    //                 result.is_mounted, result.name, result.size, result.type);

    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setCursor(0, 0);
    GetHAL().canvas.setTextColor(TFT_ORANGE);
    GetHAL().canvas.println("SD Card Info:");

    GetHAL().canvas.setCursor(0, 24);
    if (result.is_mounted) {
        GetHAL().canvas.setTextColor(TFT_CYAN);
        GetHAL().canvas.println(result.name.c_str());
        GetHAL().canvas.println(result.size.c_str());
        GetHAL().canvas.println(result.type.c_str());
    } else {
        GetHAL().canvas.setTextColor(TFT_RED);
        GetHAL().canvas.println("SD Card Not Found");
    }
    GetHAL().pushCanvas();
}
