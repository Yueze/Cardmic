/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>
#include <ctime>

/**
 * @brief
 *
 */
class AppClock : public mooncake::AppAbility {
public:
    AppClock();
    ~AppClock();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    static constexpr uint32_t UPDATE_INTERVAL = 1000;  // Update every 1 second
    uint32_t _time_count                      = 0;

    void render_interface();
    void update_time_display();
    void show_network_time();
    void show_system_time();
};
