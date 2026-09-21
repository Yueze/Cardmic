/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "chat_view.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>

ChatView::~ChatView()
{
    if (_key_event_slot_id >= 0) {
        GetHAL().keyboard.onKeyEvent.disconnect(_key_event_slot_id);
        _key_event_slot_id = -1;
    }
}

void ChatView::init()
{
    _key_event_slot_id = GetHAL().keyboard.onKeyEvent.connect(
        [this](const Keyboard::KeyEvent_t& keyEvent) { handle_key_event(keyEvent); });

    render_chat_interface();
}

void ChatView::update()
{
    // 更新光标闪烁
    update_cursor();
}

void ChatView::addMessage(const std::string& message, bool is_sent)
{
    // 确保我们在聊天区域写入（输入面板下方）
    int current_y = GetHAL().canvas.getCursorY();
    if (current_y < INPUT_PANEL_HEIGHT + 2) {
        GetHAL().canvas.setCursor(0, INPUT_PANEL_HEIGHT + 2);
    }

    // 根据消息类型设置颜色和前缀
    if (is_sent) {
        GetHAL().canvas.setTextColor(TFT_GREEN, THEME_COLOR_BG);
        GetHAL().canvas.print("<< ");
    } else {
        GetHAL().canvas.setTextColor(TFT_CYAN, THEME_COLOR_BG);
        GetHAL().canvas.print(">> ");
    }

    GetHAL().canvas.println(message.c_str());

    // 重新绘制输入面板以保持其可见性（光标位置会被保留）
    render_input_panel();
}

void ChatView::render_chat_interface()
{
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setFont(FONT_REPL);

    // 为聊天消息启用文本滚动，但设置滚动区域在输入面板下方
    GetHAL().canvas.setTextScroll(true);

    // 设置初始光标位置在输入面板下方
    GetHAL().canvas.setCursor(0, INPUT_PANEL_HEIGHT + 2);
    GetHAL().canvas.setTextColor(TFT_DARKGRAY);
    GetHAL().canvas.println("Chat ready...");

    render_input_panel();
}

void ChatView::render_input_panel()
{
    // 保存当前光标位置
    int saved_x = GetHAL().canvas.getCursorX();
    int saved_y = GetHAL().canvas.getCursorY();

    const uint32_t input_panel_color = (uint32_t)(0x606060);

    // 清除顶部输入面板区域
    GetHAL().canvas.fillRect(0, 0, GetHAL().canvas.width(), INPUT_PANEL_HEIGHT, input_panel_color);

    // 绘制输入提示符和缓冲区
    GetHAL().canvas.setTextColor(TFT_WHITE, input_panel_color);
    GetHAL().canvas.setCursor(5, 0);
    GetHAL().canvas.setTextSize(1);

    std::string display_text = ">>> " + _input_buffer;
    if (_cursor_state) {
        display_text += "_";
    } else {
        display_text += " ";
    }

    GetHAL().canvas.print(display_text.c_str());

    // 恢复聊天消息的光标位置
    GetHAL().canvas.setCursor(saved_x, saved_y);

    GetHAL().pushCanvas();
}

void ChatView::handle_key_event(const Keyboard::KeyEvent_t& keyEvent)
{
    // 只处理按键按下事件，跳过修饰键
    if (!keyEvent.state || keyEvent.isModifier) {
        return;
    }

    switch (keyEvent.keyCode) {
        case KEY_ENTER:
            send_message();
            break;

        case KEY_BACKSPACE:
            if (!_input_buffer.empty()) {
                _input_buffer.pop_back();
                render_input_panel();
            }
            break;

        case KEY_SPACE:
            if (_input_buffer.length() < INPUT_BUFFER_SIZE - 1) {
                _input_buffer += ' ';
                render_input_panel();
            }
            break;

        default:
            // 添加常规字符
            if (keyEvent.keyName && strlen(keyEvent.keyName) == 1 && _input_buffer.length() < INPUT_BUFFER_SIZE - 1) {
                _input_buffer += keyEvent.keyName;
                render_input_panel();
            }
            break;
    }
}

void ChatView::send_message()
{
    if (_input_buffer.empty()) {
        return;
    }

    // 通过回调函数发送消息
    if (onSendMsg) {
        onSendMsg(_input_buffer);
    }

    // 添加到聊天显示
    addMessage(_input_buffer, true);

    // 清空输入缓冲区
    _input_buffer.clear();
    render_input_panel();
}

void ChatView::update_cursor()
{
    if (GetHAL().millis() - _cursor_update_time > CURSOR_BLINK_PERIOD) {
        _cursor_state       = !_cursor_state;
        _cursor_update_time = GetHAL().millis();
        render_input_panel();
    }
}
