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
class AppWifiScan : public mooncake::AppAbility {
public:
    AppWifiScan();
    ~AppWifiScan();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    std::vector<Hal::ScanResult_t> _scan_result;
    uint32_t _time_count = 0;

    void render_page_scanning();
    void render_page_result();
};
