/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <functional>
#include <hal/hal.h>
#include <string>

/**
 * @brief
 *
 */
class ChatView {
public:
    ~ChatView();

    using SendMessageCallback = std::function<void(const std::string& message)>;
    SendMessageCallback onSendMsg;

    void init();
    virtual void update();
    void addMessage(const std::string& message, bool is_sent);

protected:
    static constexpr size_t INPUT_BUFFER_SIZE     = 256;
    static constexpr uint32_t CURSOR_BLINK_PERIOD = 500;
    static constexpr int INPUT_PANEL_HEIGHT       = 18;

    uint32_t _cursor_update_time = 0;
    bool _cursor_state           = false;
    std::string _input_buffer;
    int _key_event_slot_id = -1;

    virtual void render_chat_interface();
    virtual void render_input_panel();
    void handle_key_event(const Keyboard::KeyEvent_t& keyEvent);
    virtual void send_message();
    void update_cursor();
};
