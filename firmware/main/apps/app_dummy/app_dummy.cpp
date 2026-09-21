/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_dummy.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>
#include <hal.h>

using namespace mooncake;

static int _dummy_id = 0;

AppDummy::AppDummy()
{
    _dummy_id++;
    setAppInfo().name = fmt::format("Dummy{}", _dummy_id);
    // setAppInfo().userData = new AppIcon_t(icon_big..., icon_small...);
}

AppDummy::~AppDummy()
{
    // delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppDummy::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Clear screen
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setTextScroll(true);
    GetHAL().canvas.setFont(FONT_REPL);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setCursor(0, 0);

    // Handle key event, print key name when key pressed
    _handle_key_event_slot_id = GetHAL().keyboard.onKeyEvent.connect([this](const Keyboard::KeyEvent_t& keyEvent) {
        // Skip modifier key and key released
        if (keyEvent.isModifier || keyEvent.state == false) {
            return;
        }
        if (keyEvent.keyCode == KEY_ENTER) {
            GetHAL().canvas.println();
        } else {
            GetHAL().canvas.print(keyEvent.keyName);
        }
        GetHAL().pushCanvas();
    });
}

void AppDummy::onRunning()
{
    // Log hi every 2 seconds
    if (GetHAL().millis() - _time_count > 2000) {
        mclog::tagInfo(getAppInfo().name, "hi");
        _time_count = GetHAL().millis();
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppDummy::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");

    // Disconnect key event handler
    if (_handle_key_event_slot_id >= 0) {
        GetHAL().keyboard.onKeyEvent.disconnect(_handle_key_event_slot_id);
    }
}
