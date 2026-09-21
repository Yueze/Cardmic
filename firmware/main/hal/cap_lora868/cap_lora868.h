/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include "TinyGPSPlus/TinyGPS++.h"
#include <mooncake_log_signal.h>
#include <M5Unified.h>
#include <string>

class CapLoRa868 {
public:
    bool init();
    void update();

    /* ---------------------------------- LoRa ---------------------------------- */
    struct lora_config {
        static constexpr float ferq              = 868.0f;
        static constexpr float bw                = 500.0f;
        static constexpr uint8_t sf              = 7;
        static constexpr uint8_t cr              = 5;
        static constexpr uint8_t syncWord        = 0x34;
        static constexpr int8_t power            = 10;
        static constexpr uint16_t preambleLength = 10;
    };

    bool loraSendMsg(const std::string& msg);
    mclog::Signal<const std::string&> onLoraMsg;

    /* ----------------------------------- GPS ---------------------------------- */
    TinyGPSPlus* borrowGPS();
    void returnGPS();

private:
    bool _is_inited = false;

    bool lora_init();
    void lora_update();
    bool gps_init();
};
