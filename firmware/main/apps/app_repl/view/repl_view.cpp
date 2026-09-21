/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "repl_view.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>

ReplView::~ReplView()
{
    if (_key_event_slot_id >= 0) {
        GetHAL().keyboard.onKeyEvent.disconnect(_key_event_slot_id);
        _key_event_slot_id = -1;
    }
}

void ReplView::init()
{
    _key_event_slot_id = GetHAL().keyboard.onKeyEvent.connect(
        [this](const Keyboard::KeyEvent_t& keyEvent) { handle_key_event(keyEvent); });

    render_interface();
}

void ReplView::update()
{
    // 更新光标闪烁
    update_cursor();
}

void ReplView::clearScreen()
{
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setCursor(0, 0);
    GetHAL().pushCanvas();
}

void ReplView::showMessage(const std::string& message, uint16_t color)
{
    GetHAL().canvas.setTextColor(color, THEME_COLOR_BG);
    GetHAL().canvas.println(message.c_str());
    GetHAL().canvas.setTextColor(TFT_WHITE, THEME_COLOR_BG);
    GetHAL().pushCanvas();
}

void ReplView::showPrompt(const std::string& prompt_text)
{
    _prompt_text = prompt_text;
    render_prompt();
}

void ReplView::setPromptText(const std::string& prompt)
{
    _prompt_text = prompt;
}

void ReplView::setInputBuffer(const std::string& text)
{
    _input_buffer = text;
    render_prompt();
}

void ReplView::clearInputBuffer()
{
    _input_buffer.clear();
    render_prompt();
}

void ReplView::render_interface()
{
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setFont(FONT_REPL);
    GetHAL().canvas.setTextScroll(true);
    GetHAL().canvas.setCursor(0, 0);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setTextColor(TFT_WHITE, THEME_COLOR_BG);

    if (onRenderTips) {
        onRenderTips();
    }

    render_prompt();
}

void ReplView::render_prompt()
{
    // 显示输入和光标
    std::string display_text = _prompt_text + _input_buffer;
    if (_cursor_state) {
        display_text += "_";
    } else {
        display_text += " ";
    }

    // 获取当前光标位置
    int cursor_y = GetHAL().canvas.getCursorY();

    // 清除当前行，确保光标被完全移除
    GetHAL().canvas.fillRect(0, cursor_y, GetHAL().canvas.width(), 15, THEME_COLOR_BG);

    // 打印提示符和输入
    GetHAL().canvas.setCursor(0, cursor_y);
    GetHAL().canvas.setTextColor(TFT_WHITE, THEME_COLOR_BG);
    GetHAL().canvas.print(display_text.c_str());

    GetHAL().pushCanvas();
}

void ReplView::handle_key_event(const Keyboard::KeyEvent_t& keyEvent)
{
    // 只处理按键按下事件，跳过修饰键
    if (!keyEvent.state || keyEvent.isModifier) {
        return;
    }

    switch (keyEvent.keyCode) {
        case KEY_ENTER:
            handle_enter_key();
            break;

        case KEY_BACKSPACE:
            handle_backspace();
            break;

        case KEY_SPACE:
            if (_input_buffer.length() < INPUT_BUFFER_SIZE - 1) {
                _input_buffer += ' ';
                render_prompt();
            }
            break;

        default:
            // 添加常规字符
            if (keyEvent.keyName && strlen(keyEvent.keyName) == 1 && _input_buffer.length() < INPUT_BUFFER_SIZE - 1) {
                _input_buffer += keyEvent.keyName;
                render_prompt();
            }
            break;
    }
}

void ReplView::handle_enter_key()
{
    // 清除当前输入行并重新绘制不带光标的版本
    int cursor_y = GetHAL().canvas.getCursorY();
    GetHAL().canvas.fillRect(0, cursor_y, GetHAL().canvas.width(), 15, THEME_COLOR_BG);
    GetHAL().canvas.setCursor(0, cursor_y);
    GetHAL().canvas.setTextColor(TFT_WHITE, THEME_COLOR_BG);
    GetHAL().canvas.print(_prompt_text.c_str());
    GetHAL().canvas.print(_input_buffer.c_str());
    GetHAL().canvas.println();  // 移动到下一行

    // 通过回调执行命令
    if (onCommand && !_input_buffer.empty()) {
        onCommand(_input_buffer);
    }

    // 重置输入缓冲区并显示新提示符
    if (autoClearPrompt) {
        _input_buffer.clear();
    }
    render_prompt();
}

void ReplView::handle_backspace()
{
    if (!_input_buffer.empty()) {
        _input_buffer.pop_back();

        // 清除更大区域确保旧光标被完全移除
        int cursor_y = GetHAL().canvas.getCursorY();
        GetHAL().canvas.fillRect(0, cursor_y, GetHAL().canvas.width(), 15, THEME_COLOR_BG);

        // 重新绘制提示行
        render_prompt();
    }
}

void ReplView::update_cursor()
{
    if (GetHAL().millis() - _cursor_update_time > CURSOR_BLINK_PERIOD) {
        _cursor_state       = !_cursor_state;
        _cursor_update_time = GetHAL().millis();
        render_prompt();
    }
}
