/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <mooncake.h>
#include <cstdint>

/**
 * @brief
 *
 */
class AppSdcard : public mooncake::AppAbility {
public:
    AppSdcard();
    ~AppSdcard();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    uint32_t _time_count;

    void probe_sd_card();
};
