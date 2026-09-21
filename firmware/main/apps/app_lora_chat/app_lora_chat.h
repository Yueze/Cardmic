/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include "view/lora_chat_view.h"
#include <mooncake.h>
#include <cstdint>
#include <memory>

/**
 * @brief
 *
 */
class AppLoraChat : public mooncake::AppAbility {
public:
    AppLoraChat();
    ~AppLoraChat();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    std::unique_ptr<LoraChatView> _chat_view;
    bool _is_cap_available    = false;
    int _lora_receive_slot_id = -1;
    uint32_t _time_count      = 0;

    void send_message(const std::string& message);
    void update_gps_broadcast();
};
