/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>

/**
 * @brief
 *
 */
class AppGPS : public mooncake::AppAbility {
public:
    AppGPS();
    ~AppGPS();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    bool _is_cap_available    = false;
    uint32_t _time_count      = 0;
    uint32_t _sync_time_count = 0;

    void render_gps_data();
    void sync_gps_time_to_system(TinyGPSPlus* gps);
};
