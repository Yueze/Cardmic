/*
 * Cardmic: use the Cardputer ADV as a microphone -- driverless over USB, and
 * over WiFi to the Cardmic desktop client, using the network saved in the
 * stock "Set WiFi" app.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once
#include <mooncake.h>
#include <cstdint>

class AppCardmic : public mooncake::AppAbility {
public:
    AppCardmic();
    ~AppCardmic();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    void draw();

    int _key_slot_id       = -1;
    uint32_t _last_draw_ms = 0;
};
