/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_chat.h"
#include "assets/chat_big.h"
#include "assets/chat_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>

using namespace mooncake;

AppChat::AppChat()
{
    setAppInfo().name     = "Chat";
    setAppInfo().userData = new AppIcon_t(image_data_chat_big, image_data_chat_small);
}

AppChat::~AppChat()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppChat::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Initialize ESP-NOW
    GetHAL().espNowInit();

    // Create chat view
    _chat_view            = std::make_unique<ChatView>();
    _chat_view->onSendMsg = [this](const std::string& message) { send_message(message); };
    _chat_view->init();
}

void AppChat::onRunning()
{
    if (_chat_view) {
        _chat_view->update();
    }

    check_received_messages();

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppChat::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");

    // Clean up chat view
    if (_chat_view) {
        _chat_view.reset();
    }
}

void AppChat::send_message(const std::string& message)
{
    // mclog::tagInfo(getAppInfo().name, "sending message: \"{}\"", message);

    // Send via ESP-NOW
    GetHAL().espNowSend(message);
}

void AppChat::check_received_messages()
{
    if (GetHAL().espNowAvailable()) {
        const std::string& received_data = GetHAL().espNowGetReceivedData();
        // mclog::tagInfo(getAppInfo().name, "received message: \"{}\"", received_data);

        if (_chat_view) {
            _chat_view->addMessage(received_data, false);
        }
        GetHAL().espNowClearReceivedData();
    }
}
