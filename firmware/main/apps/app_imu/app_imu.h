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
class AppImu : public mooncake::AppAbility {
public:
    AppImu();
    ~AppImu();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    m5::imu_data_t _imu_data;
    std::string _str_buffer;
    float _panel_angle = 0.0f;

    void render_imu_data_label();
    void render_imu_panel();
    void update_panel_angle();
};
