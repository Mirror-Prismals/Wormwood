cmake_minimum_required(VERSION 3.24)
project(WormwoodIOS LANGUAGES C OBJC)
get_filename_component(WORMWOOD_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
if(NOT WORMWOOD_ASSETS OR NOT WORMWOOD_RUST_LIBRARY)
 message(FATAL_ERROR "Supply WORMWOOD_TEAM, WORMWOOD_ASSETS and WORMWOOD_RUST_LIBRARY")
endif()
if(NOT WORMWOOD_BUNDLE_ID)
 set(WORMWOOD_BUNDLE_ID "org.salamander.mirror-prismals.ios")
endif()
if(NOT WORMWOOD_BUILD_NUMBER)
 set(WORMWOOD_BUILD_NUMBER "1")
endif()
if(NOT WORMWOOD_SIGN_IDENTITY)
 set(WORMWOOD_SIGN_IDENTITY "Apple Development")
endif()
add_executable(Wormwood MACOSX_BUNDLE "${WORMWOOD_ROOT}/Platform/iOS/App.m" "${WORMWOOD_ROOT}/Platform/iOS/WorkstationOverlay.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSGraphics.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSAudio.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSGraphicsSymbols.c")
set_target_properties(Wormwood PROPERTIES
 MACOSX_BUNDLE_INFO_PLIST "${CMAKE_CURRENT_LIST_DIR}/Info.plist"
 XCODE_ATTRIBUTE_PRODUCT_BUNDLE_IDENTIFIER "${WORMWOOD_BUNDLE_ID}"
 XCODE_ATTRIBUTE_DEVELOPMENT_TEAM "${WORMWOOD_TEAM}"
 XCODE_ATTRIBUTE_CODE_SIGN_STYLE Automatic
 XCODE_ATTRIBUTE_CODE_SIGN_IDENTITY "${WORMWOOD_SIGN_IDENTITY}"
 XCODE_ATTRIBUTE_MARKETING_VERSION "0.1.0"
 XCODE_ATTRIBUTE_CURRENT_PROJECT_VERSION "${WORMWOOD_BUILD_NUMBER}"
 XCODE_ATTRIBUTE_TARGETED_DEVICE_FAMILY "1,2"
 XCODE_ATTRIBUTE_CLANG_ENABLE_OBJC_ARC YES
 XCODE_ATTRIBUTE_ENABLE_BITCODE NO)
target_link_libraries(Wormwood PRIVATE "${WORMWOOD_RUST_LIBRARY}" "-framework UIKit" "-framework CoreText" "-framework OpenGLES" "-framework QuartzCore" "-framework AudioToolbox" "-framework AVFAudio" "-framework Foundation" "-framework Security" "-framework SystemConfiguration" "-framework CoreFoundation" "-liconv")
add_custom_command(TARGET Wormwood POST_BUILD
 COMMAND "${CMAKE_COMMAND}" -E copy_directory "${WORMWOOD_ASSETS}" "$<TARGET_BUNDLE_DIR:Wormwood>/EngineAssets")

if(NOT WORMWOOD_WGPU_LIBRARY OR NOT WORMWOOD_MOLTENVK_LIBRARY OR NOT WORMWOOD_WGPU_INCLUDE)
 message(FATAL_ERROR "Supply pinned iOS WebGPU/MoltenVK SDK paths")
endif()
target_include_directories(Wormwood PRIVATE "${WORMWOOD_WGPU_INCLUDE}" /opt/homebrew/include)
target_link_libraries(Wormwood PRIVATE "${WORMWOOD_WGPU_LIBRARY}" "${WORMWOOD_MOLTENVK_LIBRARY}" "-framework Metal" "-framework IOSurface" "-framework CoreGraphics" "-lc++")
# Rust resolves these statically linked APIs with dlsym. Retaining their code
# alone is insufficient: archive stripping must preserve the exported names too.
set(graphics_symbols_source "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSGraphicsSymbols.c")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${graphics_symbols_source}")
file(READ "${graphics_symbols_source}" graphics_symbols_source_text)
string(REGEX MATCHALL "&(vk|wgpu)[A-Za-z0-9_]+" graphics_symbol_references "${graphics_symbols_source_text}")
if(NOT graphics_symbol_references)
 message(FATAL_ERROR "The iOS graphics export list is empty")
endif()
set(graphics_exports "${CMAKE_CURRENT_BINARY_DIR}/graphics.exports")
set(graphics_export_lines "")
foreach(reference IN LISTS graphics_symbol_references)
 string(SUBSTRING "${reference}" 1 -1 symbol)
 string(APPEND graphics_export_lines "_${symbol}\n")
endforeach()
file(WRITE "${graphics_exports}" "${graphics_export_lines}")
target_link_options(Wormwood PRIVATE "-Wl,-u,_ww_graphics_symbols" "-Wl,-exported_symbols_list,${graphics_exports}")
set_property(TARGET Wormwood APPEND PROPERTY LINK_DEPENDS "${graphics_exports}")
# Xcode's default executable strip style removes unreferenced global exports.
set_target_properties(Wormwood PROPERTIES XCODE_ATTRIBUTE_STRIP_STYLE "non-global")

target_compile_definitions(Wormwood PRIVATE DAWII_GAME=0)

if(WORMWOOD_APP_ASSETS)
 set_source_files_properties("${WORMWOOD_APP_ASSETS}" PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
 target_sources(Wormwood PRIVATE "${WORMWOOD_APP_ASSETS}")
 set_target_properties(Wormwood PROPERTIES XCODE_ATTRIBUTE_ASSETCATALOG_COMPILER_APPICON_NAME AppIcon)
endif()
# Xcode archives must install the app under Products/Applications. An empty
# INSTALL_PATH produces a generic archive with no distributable application.
set_target_properties(Wormwood PROPERTIES
 XCODE_ATTRIBUTE_INSTALL_PATH "/Applications"
 XCODE_ATTRIBUTE_SKIP_INSTALL NO
 XCODE_ATTRIBUTE_ENABLE_USER_SCRIPT_SANDBOXING NO)

set_target_properties(Wormwood PROPERTIES XCODE_ATTRIBUTE_DEBUG_INFORMATION_FORMAT "dwarf-with-dsym" XCODE_ATTRIBUTE_GCC_GENERATE_DEBUGGING_SYMBOLS YES)

set_source_files_properties("${CMAKE_CURRENT_LIST_DIR}/PrivacyInfo.xcprivacy" PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
target_sources(Wormwood PRIVATE "${CMAKE_CURRENT_LIST_DIR}/PrivacyInfo.xcprivacy")
