/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include "app_chat/view/chat_view.h"

class LoraChatView : public ChatView {
public:
    void update() override;
    inline bool isGpsMode() const
    {
        return _is_gps_mode;
    }
    inline const std::string& getDeviceId() const
    {
        return _device_id;
    }

private:
    static constexpr int LORA_CONFIG_PANEL_HEIGHT = 14;

    bool _is_gps_mode = false;
    std::string _device_id;

    void render_chat_interface() override;
    void render_input_panel() override;
    void render_lora_config_panel();
    void render_gps_broadcast_panel();
    void send_message() override;
};
