/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_remote.h"
#include "assets/ir_big.h"
#include "assets/ir_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>

using namespace mooncake;

AppRemote::AppRemote()
{
    setAppInfo().name     = "Remote";
    setAppInfo().userData = new AppIcon_t(image_data_ir_big, image_data_ir_small);
}

AppRemote::~AppRemote()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);
}

void AppRemote::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Initialize IR
    GetHAL().irInit();

    // Create repl view
    _repl_view               = std::make_unique<ReplView>();
    _repl_view->onRenderTips = []() {
        GetHAL().canvas.setTextColor(TFT_ORANGE);
        GetHAL().canvas.println("Input NEC message in:");
        GetHAL().canvas.println("addr(dec),data(dec)");
        GetHAL().canvas.println("e.g. 16,1");
        GetHAL().canvas.println("Hit Enter to send");
    };
    _repl_view->onCommand       = [this](const std::string& command) { parse_and_send_ir(command); };
    _repl_view->autoClearPrompt = false;
    _repl_view->init();
}

void AppRemote::onRunning()
{
    if (_repl_view) {
        _repl_view->update();
    }

    // Close app when home button clicked
    if (GetHAL().homeButton.wasClicked()) {
        audio::play_random_tone();
        close();
    }
}

void AppRemote::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");

    // Clean up repl view
    if (_repl_view) {
        _repl_view.reset();
    }
}

// 辅助函数：尝试把字符串解析成 uint8_t
auto parse_uint8 = [](const std::string& s, uint8_t& out) -> bool {
    int value   = 0;
    auto result = std::from_chars(s.data(), s.data() + s.size(), value);
    if (result.ec != std::errc() || value < 0 || value > 255) {
        return false;
    }
    out = static_cast<uint8_t>(value);
    return true;
};

void AppRemote::parse_and_send_ir(const std::string& command)
{
    mclog::tagDebug(getAppInfo().name, "parsing input: \"{}\"", command);

    bool parse_success = false;
    uint8_t ir_addr    = 0;
    uint8_t ir_cmd     = 0;

    // Split input string
    split_string(command, ',', _parse_result);

    mclog::tagDebug(getAppInfo().name, "parsed {} parts", _parse_result.size());
    for (const auto& part : _parse_result) {
        mclog::tagDebug(getAppInfo().name, "part: \"{}\"", part);
    }

    if (_parse_result.size() >= 2) {
        if (parse_uint8(_parse_result[0], ir_addr) && parse_uint8(_parse_result[1], ir_cmd)) {
            parse_success = true;
            mclog::tagInfo(getAppInfo().name, "parsed addr: {} cmd: {}", ir_addr, ir_cmd);
        } else {
            parse_success = false;
            mclog::tagError(getAppInfo().name, "parse failed");
        }
    }

    // Send IR signal if parsing was successful
    if (parse_success) {
        GetHAL().irSend(ir_addr, ir_cmd);

        auto msg = fmt::format("Message sent:\naddr: 0x{:02X} cmd: 0x{:02X}", ir_addr, ir_cmd);
        _repl_view->showMessage(msg, TFT_GREEN);
    } else {
        auto msg = fmt::format("Parse failed: \"{}\"", command);
        _repl_view->showMessage(msg, TFT_RED);
    }
}

void AppRemote::split_string(const std::string& str, char separator, std::vector<std::string>& result)
{
    result.clear();

    size_t start = 0;
    size_t end   = 0;

    while (end != std::string::npos) {
        end              = str.find(separator, start);
        std::string part = str.substr(start, end - start);

        // Trim whitespace
        size_t first = part.find_first_not_of(' ');
        if (first != std::string::npos) {
            size_t last = part.find_last_not_of(' ');
            part        = part.substr(first, (last - first + 1));
        }

        if (!part.empty()) {
            result.push_back(part);
        }

        start = end + 1;
    }
}
