# The following five lines of boilerplate have to be in your project's
# CMakeLists in this exact order for cmake to work correctly
cmake_minimum_required(VERSION 3.16)

include($ENV{IDF_PATH}/tools/cmake/project.cmake)

# Cardmic USB Audio Class options. Must be GLOBAL compile options: the TinyUSB
# audio class driver is in the espressif/tinyusb component and is compiled out
# unless its own sources see CFG_TUD_AUDIO=1. HID (stock USB keyboard) is
# configured separately via Kconfig and is unaffected.
foreach(opt
    CFG_TUD_AUDIO=1
    CFG_TUD_AUDIO_FUNC_1_N_AS_INT=1               # tinyusb 0.18 needs this explicitly
    CFG_TUD_AUDIO_FUNC_1_SAMPLE_RATE=16000
    CFG_TUD_AUDIO_FUNC_1_DESC_LEN=132
    CFG_TUD_AUDIO_FUNC_1_CTRL_BUF_SZ=64
    CFG_TUD_AUDIO_ENABLE_EP_IN=1
    CFG_TUD_AUDIO_ENABLE_EP_OUT=0
    CFG_TUD_AUDIO_FUNC_1_N_BYTES_PER_SAMPLE_TX=2
    CFG_TUD_AUDIO_FUNC_1_N_CHANNELS_TX=1
    CFG_TUD_AUDIO_EP_SZ_IN=34
    CFG_TUD_AUDIO_FUNC_1_EP_IN_SZ_MAX=34
    CFG_TUD_AUDIO_FUNC_1_EP_IN_SW_BUF_SZ=512)
    idf_build_set_property(COMPILE_OPTIONS "-D${opt}" APPEND)
endforeach()

# Development builds: idf.py -B build-dev -DCARDMIC_DEV=1 build
# (remote reflash and key injection over Wi-Fi; never in releases).
if(CARDMIC_DEV)
    idf_build_set_property(COMPILE_OPTIONS "-DCARDMIC_DEV_TOOLS=1" APPEND)
endif()

# Single source of the firmware version (shown in About, compared by OTA).
set(PROJECT_VER "0.6.2")
project(cardputer-adv)
