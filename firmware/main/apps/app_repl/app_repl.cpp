/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_repl.h"
#include "assets/repl_big.h"
#include "assets/repl_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>

using namespace mooncake;

/// -----------------------------
/// pikaScript output redirect
static AppREPL* _current_repl_instance = nullptr;

int pika_platform_putchar(char ch)
{
    if (_current_repl_instance) {
        GetHAL().canvas.setTextColor(TFT_CYAN);
        GetHAL().canvas.print(ch);
        GetHAL().canvas.setTextColor(TFT_WHITE);
    }
    return putchar(ch);
}

void pika_platform_printf(char* fmt, ...)
{
    if (_current_repl_instance) {
        GetHAL().canvas.setTextColor(TFT_CYAN);
        va_list args;
        va_start(args, fmt);
        GetHAL().canvas.vprintf(fmt, args);
        va_end(args);
        GetHAL().canvas.setTextColor(TFT_WHITE);
    }
}

void pika_platform_clear(void)
{
    if (_current_repl_instance) {
        GetHAL().canvas.fillScreen(THEME_COLOR_BG);
        GetHAL().canvas.setCursor(0, 0);
        GetHAL().pushCanvas();
    }
}
/// -----------------------------

AppREPL::AppREPL()
{
    setAppInfo().name     = "REPL";
    setAppInfo().userData = new AppIcon_t(image_data_repl_big, image_data_repl_small);
}

AppREPL::~AppREPL()
{
    delete static_cast<AppIcon_t*>(getAppInfo().userData);

    // Clear the global instance pointer
    if (_current_repl_instance == this) {
        _current_repl_instance = nullptr;
    }
}

void AppREPL::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Set up pikaScript output redirect
    setup_pikapython_output_redirect();

    // Create repl view
    _repl_view               = std::make_unique<ReplView>();
    _repl_view->onRenderTips = []() {
        auto tips = fmt::format("PikaScript v{}.{}.{}", PIKA_VERSION_MAJOR, PIKA_VERSION_MINOR, PIKA_VERSION_MICRO);
        GetHAL().canvas.setTextColor(TFT_CYAN);
        GetHAL().canvas.println(tips.c_str());
    };
    _repl_view->onCommand = [this](const std::string& command) { execute_python_command(command); };
    _repl_view->init();
}

void AppREPL::onRunning()
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

void AppREPL::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");

    // Clean up repl view
    if (_repl_view) {
        _repl_view.reset();
    }

    // Clear the global instance pointer
    if (_current_repl_instance == this) {
        _current_repl_instance = nullptr;
    }

    if (_pika_main) {
        obj_deinit(_pika_main);
        _pika_main = nullptr;
    }
}

void AppREPL::setup_pikapython_output_redirect()
{
    // Set global instance pointer for output redirect
    _current_repl_instance = this;

    // Create pikaScript instance if not already created
    if (_pika_main == nullptr) {
        mclog::tagInfo(getAppInfo().name, "creating pikaScript instance");
        _pika_main = pikaScriptInit();
    } else {
        mclog::tagInfo(getAppInfo().name, "pikaScript instance already exists");
    }
}

void AppREPL::execute_python_command(const std::string& command)
{
    if (command.empty()) {
        return;
    }

    mclog::tagInfo(getAppInfo().name, "execute command: \"{}\"", command);

    if (_pika_main) {
        // Execute the command - pikaScript will output directly to canvas via our redirect functions
        obj_run(_pika_main, const_cast<char*>(command.c_str()));
        GetHAL().pushCanvas();
    } else {
        if (_repl_view) {
            _repl_view->showMessage("Error: Python interpreter not initialized", TFT_RED);
        }
    }
}
