/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>
#include <string>
#include <vector>
#include <smooth_ui_toolkit.h>

/**
 * @brief
 *
 */
class KeyboardSelectorMenu : public smooth_ui_toolkit::SmoothSelectorMenu {
public:
    enum KeyboardType_t {
        KEYBOARD_TYPE_NONE = 0,
        KEYBOARD_TYPE_BLE,
        KEYBOARD_TYPE_USB,
    };

    void init();
    void onReadInput() override;
    void onRender() override;
    void onClick() override;

    bool isSelected() const
    {
        return _is_selected;
    }

    KeyboardType_t getSelectedType() const
    {
        return _keyboard_type;
    }

private:
    bool _is_selected             = false;
    KeyboardType_t _keyboard_type = KEYBOARD_TYPE_NONE;
};

/**
 * @brief
 *
 */
class AppKeyboard : public mooncake::AppAbility {
public:
    AppKeyboard();
    ~AppKeyboard();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    uint32_t _info_update_time           = 0;
    bool _is_selecting                   = true;
    bool _is_keyboard_active             = false;
    KeyboardSelectorMenu* _selector_menu = nullptr;

    void select_keyboard_type();
    void init_ble_keyboard();
    void init_usb_keyboard();
    void update_connection_info();
    void render_keyboard_interface();
    void render_connection_status();
};
