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
class ReplView {
public:
    virtual ~ReplView();

    std::function<void()> onRenderTips;
    std::function<void(const std::string& command)> onCommand;
    bool autoClearPrompt = true;

    virtual void init();
    virtual void update();
    void clearScreen();
    void showMessage(const std::string& message, uint16_t color = TFT_WHITE);
    void showPrompt(const std::string& prompt_text = ">>> ");
    void setPromptText(const std::string& prompt);

    void setInputBuffer(const std::string& text);
    const std::string& getInputBuffer() const
    {
        return _input_buffer;
    }
    void clearInputBuffer();

protected:
    static constexpr size_t INPUT_BUFFER_SIZE     = 256;
    static constexpr uint32_t CURSOR_BLINK_PERIOD = 500;

    uint32_t _cursor_update_time = 0;
    bool _cursor_state           = false;
    std::string _input_buffer;
    std::string _prompt_text = ">>> ";
    int _key_event_slot_id   = -1;

    virtual void render_interface();
    virtual void render_prompt();
    void handle_key_event(const Keyboard::KeyEvent_t& keyEvent);
    virtual void handle_enter_key();
    virtual void handle_backspace();
    void update_cursor();
};
