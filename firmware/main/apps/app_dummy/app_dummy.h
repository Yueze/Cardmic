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
class AppDummy : public mooncake::AppAbility {
public:
    AppDummy();
    ~AppDummy();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    uint32_t _time_count;
    int _handle_key_event_slot_id = -1;
};
