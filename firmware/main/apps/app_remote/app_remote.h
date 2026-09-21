/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <apps/app_repl/view/repl_view.h>
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>
#include <string>
#include <vector>
#include <memory>

/**
 * @brief
 *
 */
class AppRemote : public mooncake::AppAbility {
public:
    AppRemote();
    ~AppRemote();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    std::unique_ptr<ReplView> _repl_view;
    std::vector<std::string> _parse_result;

    void parse_and_send_ir(const std::string& command);
    void split_string(const std::string& str, char separator, std::vector<std::string>& result);
};
