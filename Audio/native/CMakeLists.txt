# Included by Rewrite's native build; no automatic download or installation.
cmake_minimum_required(VERSION 3.20)
project(MirrorChuck LANGUAGES C CXX)
file(GLOB MIRROR_CHUCK_CPP CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/chuck/*.cpp")
list(REMOVE_ITEM MIRROR_CHUCK_CPP "${CMAKE_CURRENT_SOURCE_DIR}/chuck/chuck_otf.cpp")
add_library(mirror_chuck STATIC ${MIRROR_CHUCK_CPP}
    ChuckBridge.cpp chuck/chuck_yacc.c chuck/util_network.c
    chuck/util_raw.c chuck/util_xforms.c chuck/util_sndfile.c)
target_include_directories(mirror_chuck PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/chuck")
target_compile_features(mirror_chuck PRIVATE cxx_std_17)
target_compile_definitions(mirror_chuck PRIVATE
    __DISABLE_HID__ __DISABLE_MIDI__ __DISABLE_OPSC__
    __DISABLE_OTF_SERVER__ __DISABLE_SERIAL__ __DISABLE_SHELL__)
if(CMAKE_SYSTEM_NAME STREQUAL "iOS")
    target_compile_definitions(mirror_chuck PRIVATE __CHIP_MODE__)
endif()
find_package(Threads REQUIRED)
target_link_libraries(mirror_chuck PUBLIC Threads::Threads ${CMAKE_DL_LIBS})
set_target_properties(mirror_chuck PROPERTIES POSITION_INDEPENDENT_CODE ON)
