/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once
#include <smooth_ui_toolkit.h>
#include <apps/utils/common.h>
#include <functional>

class LauncherMenu : public smooth_ui_toolkit::SmoothSelectorMenu {
public:
    struct OptionInfo_t {
        int appId = -1;
        std::string name;
    };

    std::function<void(int index, int appId)> onAppOpen;
    bool pushCanvas = true;

    void init(int launcherAppId);
    void onReadInput() override;
    void onRender() override;
    void onClick() override;

private:
    std::vector<OptionInfo_t> _option_infos;
};
