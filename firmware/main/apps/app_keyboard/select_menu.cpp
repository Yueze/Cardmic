/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_keyboard.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>

using namespace mooncake;
using namespace smooth_ui_toolkit;

// Selector Menu Implementation
void KeyboardSelectorMenu::init()
{
    // Add options directly
    addOption({{10, 30, 100, 20}, nullptr});  // BLE Keyboard
    addOption({{10, 60, 100, 20}, nullptr});  // USB Keyboard

    // Configure selector animation
    getSelectorPostion().x.springOptions().visualDuration = 0.3;
    getSelectorPostion().y.springOptions().visualDuration = 0.3;
    getSelectorShape().x.springOptions().visualDuration   = 0.3;
    getSelectorShape().y.springOptions().visualDuration   = 0.3;

    // Handle in every keyboard update
    setConfig().readInputInterval = 0;
}

void KeyboardSelectorMenu::onClick()
{
    mclog::tagInfo("KeyboardSelector", "selection made: {}", getSelectedOptionIndex());

    // Set the selected keyboard type based on the option index
    if (getSelectedOptionIndex() == 0) {
        _keyboard_type = KEYBOARD_TYPE_BLE;
    } else if (getSelectedOptionIndex() == 1) {
        _keyboard_type = KEYBOARD_TYPE_USB;
    }

    _is_selected = true;
}

void KeyboardSelectorMenu::onReadInput()
{
    auto event = GetHAL().keyboard.getLatestKeyEventRaw();
    // mclog::info("({},{}) {}", event.row, event.col, event.state);

    // Key pressed
    if (event.state == true) {
        // Left
        if (event.row == 3 && event.col == 10) {
            goLast();
        }
        // Right
        else if (event.row == 3 && event.col == 12) {
            goNext();
        }
        // Up
        else if (event.row == 2 && event.col == 11) {
            goLast();
        }
        // Down
        else if (event.row == 3 && event.col == 11) {
            goNext();
        }
        // Enter
        else if (event.row == 2 && event.col == 13) {
            auto pressed_keyframe = shape::scale<float>(getSelectorCurrentFrame(), anchor_center, {1.3, 0.6});
            press(pressed_keyframe);
        }
    }
    // Key released
    else {
        // Enter
        if (event.row == 2 && event.col == 13) {
            release();
        }
    }
}

void KeyboardSelectorMenu::onRender()
{
    // Clear background
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);

    // Render title
    GetHAL().canvas.setTextColor(TFT_ORANGE, THEME_COLOR_BG);
    GetHAL().canvas.setCursor(0, 0);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.println("[Select Keyboard Type]");

    // Render selector
    auto selector_frame = getSelectorCurrentFrame();
    GetHAL().canvas.fillSmoothRoundRect(selector_frame.x, selector_frame.y, selector_frame.width, selector_frame.height,
                                        4, (uint32_t)0x69B38C);

    // Render options directly
    const char* option_names[] = {"BLE Keyboard", "USB Keyboard"};

    int i = 0;
    for (auto& option : getOptionList()) {
        GetHAL().canvas.setTextColor(TFT_WHITE);
        GetHAL().canvas.setCursor(option.keyframe.x, option.keyframe.y);
        GetHAL().canvas.print(option_names[i]);

        i++;
    }

    GetHAL().pushCanvas();
}
