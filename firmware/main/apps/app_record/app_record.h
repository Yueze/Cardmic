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
class AppRecord : public mooncake::AppAbility {
public:
    AppRecord();
    ~AppRecord();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    // 16000Hz采样率，1秒 = 16000个采样点，按200像素宽度分成80个缓冲区
    static constexpr size_t RECORD_NUMBER       = 80;   // 16000/200 = 80个缓冲区，正好1秒
    static constexpr size_t RECORD_LENGTH       = 200;  // 屏幕宽度
    static constexpr size_t RECORD_SIZE         = RECORD_NUMBER * RECORD_LENGTH;
    static constexpr size_t RECORD_SAMPLERATE   = 16000;
    static constexpr size_t PLAYBACK_SAMPLERATE = 16000;

    uint32_t _time_count    = 0;
    int16_t* _rec_data      = nullptr;
    size_t _rec_record_idx  = 2;
    size_t _draw_record_idx = 0;
    bool _is_recording      = true;

    void render_page_recording();
    void render_page_playing();
    void render_waveform();
    void handle_enter_key();
    void start_recording();
    void start_playback();
};
