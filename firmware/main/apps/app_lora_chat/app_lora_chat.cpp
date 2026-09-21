/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_lora_chat.h"
#include "assets/chat_lora_big.h"
#include "assets/chat_lora_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>
#include <hal.h>

using namespace mooncake;

AppLoraChat::AppLoraChat()
{
    setAppInfo().name     = "LoRaChat";
    setAppInfo().userData = new AppIcon_t(image_data_chat_lora_big, image_data_chat_lora_small);
}

AppLoraChat::~AppLoraChat()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppLoraChat::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Clear screen
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setFont(FONT_REPL);
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
        return;
    }

    // Create chat view
    _chat_view            = std::make_unique<LoraChatView>();
    _chat_view->onSendMsg = [this](const std::string& message) { send_message(message); };
    _chat_view->init();

    _lora_receive_slot_id = GetHAL().capLora868.onLoraMsg.connect([this](const std::string& message) {
        mclog::tagDebug(getAppInfo().name, "received message: \"{}\"", message);
        if (_chat_view) {
            _chat_view->addMessage(message, false);
        }
    });

    _time_count = 0;
}

void AppLoraChat::onRunning()
{
    if (_is_cap_available) {
        if (_chat_view) {
            _chat_view->update();
        }

        if (_chat_view->isGpsMode()) {
            update_gps_broadcast();
        }
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppLoraChat::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");

    // Clean up chat view
    if (_chat_view) {
        _chat_view.reset();
    }

    if (_lora_receive_slot_id >= 0) {
        GetHAL().capLora868.onLoraMsg.disconnect(_lora_receive_slot_id);
        _lora_receive_slot_id = -1;
    }
}

void AppLoraChat::send_message(const std::string& message)
{
    // mclog::tagInfo(getAppInfo().name, "sending message: \"{}\"", message);
    GetHAL().capLora868.loraSendMsg(message);
}

void AppLoraChat::update_gps_broadcast()
{
    if (GetHAL().millis() - _time_count > 1000) {
        auto gps = GetHAL().capLora868.borrowGPS();
        if (gps) {
            std::string msg;

            if (gps->location.isValid()) {
                msg = fmt::format("[{}]\n", _chat_view->getDeviceId());
                msg += fmt::format("Lat: {:>10.6f} {}\n", gps->location.lat(), gps->location.lat() > 0 ? "N" : "S");
                msg += fmt::format("Lng: {:>10.6f} {}", gps->location.lng(), gps->location.lng() > 0 ? "E" : "W");
            } else {
                msg = fmt::format("[{}] No GPS Fix", _chat_view->getDeviceId());
            }

            GetHAL().capLora868.loraSendMsg(msg);
            GetHAL().capLora868.returnGPS();
        }

        _time_count = GetHAL().millis();
    }
}
