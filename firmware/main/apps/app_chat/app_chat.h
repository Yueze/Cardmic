/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include "view/chat_view.h"
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>
#include <string>
#include <memory>

/**
 * @brief
 *
 */
class AppChat : public mooncake::AppAbility {
public:
    AppChat();
    ~AppChat();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    std::unique_ptr<ChatView> _chat_view;

    void send_message(const std::string& message);
    void check_received_messages();
};
