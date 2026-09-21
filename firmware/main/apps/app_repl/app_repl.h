/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include "view/repl_view.h"
#include <mooncake.h>
#include <cstdint>
#include <hal/hal.h>
#include <string>
#include <memory>

extern "C" {
#include <pikaScript.h>
}

/**
 * @brief
 *
 */
class AppREPL : public mooncake::AppAbility {
public:
    AppREPL();
    ~AppREPL();

    void onOpen() override;
    void onRunning() override;
    void onClose() override;

private:
    std::unique_ptr<ReplView> _repl_view;
    PikaObj* _pika_main = nullptr;

    void execute_python_command(const std::string& command);
    void setup_pikapython_output_redirect();
};
