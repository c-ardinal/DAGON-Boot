cmake_minimum_required(VERSION 3.22)
# The vendored 2nd stage's options are the single source (see its PROVENANCE.md).
set(SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_LIST_DIR}/../mcu/esp32s3/vendor/sdkconfig.defaults")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(dagon_2ndstage)
