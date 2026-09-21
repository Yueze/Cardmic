/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "lora_chat_view.h"
#include <apps/utils/theme.h>
#include <mooncake_log.h>

static const std::string _tag = "LoraChatView";

void LoraChatView::update()
{
    if (_is_gps_mode) {
        return;
    }

    ChatView::update();
}

void LoraChatView::render_lora_config_panel()
{
    GetHAL().canvas.fillRect(0, 0, GetHAL().canvas.width(), LORA_CONFIG_PANEL_HEIGHT, THEME_COLOR_BG);
    GetHAL().canvas.setFont(&fonts::Font0);
    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setTextColor((uint32_t)(0xFFFFFF));
    GetHAL().canvas.setTextDatum(textdatum_t::middle_left);

    const uint32_t brick_color = 0x60FFF5;
    std::string buffer;

    GetHAL().canvas.fillRect(2, 5, 3, 4, brick_color);
    buffer = fmt::format("Freq:{:.0f}MHz", CapLoRa868::lora_config::ferq);
    GetHAL().canvas.drawString(buffer.c_str(), 9, 7);

    GetHAL().canvas.fillRect(81, 5, 3, 4, brick_color);
    buffer = fmt::format("BW:{:.0f}", CapLoRa868::lora_config::bw);
    GetHAL().canvas.drawString(buffer.c_str(), 88, 7);

    GetHAL().canvas.fillRect(132, 5, 3, 4, brick_color);
    buffer = fmt::format("SF:{}", CapLoRa868::lora_config::sf);
    GetHAL().canvas.drawString(buffer.c_str(), 139, 7);

    GetHAL().canvas.fillRect(170, 5, 3, 4, brick_color);
    buffer = fmt::format("CR:{}", CapLoRa868::lora_config::cr);
    GetHAL().canvas.drawString(buffer.c_str(), 177, 7);

    GetHAL().canvas.setTextDatum(textdatum_t::top_left);
    GetHAL().canvas.setFont(FONT_REPL);
}

void LoraChatView::render_input_panel()
{
    render_lora_config_panel();

    if (_is_gps_mode) {
        render_gps_broadcast_panel();
        return;
    }

    // 保存当前光标位置
    int saved_x = GetHAL().canvas.getCursorX();
    int saved_y = GetHAL().canvas.getCursorY();

    const uint32_t input_panel_color = (uint32_t)(0x606060);

    // 清除顶部输入面板区域
    GetHAL().canvas.fillRect(0, LORA_CONFIG_PANEL_HEIGHT, GetHAL().canvas.width(), INPUT_PANEL_HEIGHT,
                             input_panel_color);

    // 绘制输入提示符和缓冲区
    GetHAL().canvas.setTextColor(TFT_WHITE, input_panel_color);
    GetHAL().canvas.setCursor(5, LORA_CONFIG_PANEL_HEIGHT);
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

void LoraChatView::render_gps_broadcast_panel()
{
    // 保存当前光标位置
    int saved_x = GetHAL().canvas.getCursorX();
    int saved_y = GetHAL().canvas.getCursorY();

    const uint32_t input_panel_color = (uint32_t)(0x606060);

    GetHAL().canvas.fillRect(0, LORA_CONFIG_PANEL_HEIGHT, GetHAL().canvas.width(), INPUT_PANEL_HEIGHT,
                             input_panel_color);

    GetHAL().canvas.setTextSize(1);
    GetHAL().canvas.setCursor(0, LORA_CONFIG_PANEL_HEIGHT);

    GetHAL().canvas.setTextColor(TFT_WHITE);
    GetHAL().canvas.print("Device ID: ");

    GetHAL().canvas.setTextColor(TFT_GREENYELLOW);
    GetHAL().canvas.print(_device_id.c_str());

    GetHAL().pushCanvas();

    // 恢复聊天消息的光标位置
    GetHAL().canvas.setCursor(saved_x, saved_y);
}

void LoraChatView::render_chat_interface()
{
    GetHAL().canvas.fillScreen(THEME_COLOR_BG);
    GetHAL().canvas.setBaseColor(THEME_COLOR_BG);
    GetHAL().canvas.setFont(FONT_REPL);
    GetHAL().canvas.setTextSize(1);

    // 为聊天消息启用文本滚动，但设置滚动区域在输入面板下方
    GetHAL().canvas.setTextScroll(true);

    // 设置初始光标位置在输入面板下方
    GetHAL().canvas.setCursor(0, LORA_CONFIG_PANEL_HEIGHT + INPUT_PANEL_HEIGHT + 2);

    GetHAL().canvas.setTextColor(TFT_DARKGRAY);
    if (_is_gps_mode) {
        GetHAL().canvas.println("Start broadcasting GPS\nlocation...");
    } else {
        GetHAL().canvas.print("Chat ready...\nSend \"");

        GetHAL().canvas.setTextColor((uint32_t)(0x7FD5CF));
        GetHAL().canvas.print("gps");

        GetHAL().canvas.setTextColor(TFT_DARKGRAY);
        GetHAL().canvas.println("\" to broadcast\nlocation");
    }

    render_input_panel();
}

void LoraChatView::send_message()
{
    if (_input_buffer.empty()) {
        return;
    }

    if (_is_gps_mode) {
        return;
    }

    // If send "gps", set gps mode
    if (_input_buffer == "gps") {
        mclog::tagDebug(_tag, "start gps mode");

        _is_gps_mode = true;

        // Get device id
        auto mac   = GetHAL().getDeviceMac();
        _device_id = fmt::format("{:02X}{:02X}{:02X}", mac[3], mac[4], mac[5]);
        mclog::tagDebug(_tag, "get device id: {}", _device_id);

        // Re render everything
        render_chat_interface();
        return;
    }

    ChatView::send_message();
}
