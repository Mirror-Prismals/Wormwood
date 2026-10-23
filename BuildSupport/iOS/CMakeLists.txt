cmake_minimum_required(VERSION 3.24)
project(WormwoodIOS LANGUAGES C OBJC)
get_filename_component(WORMWOOD_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
if(NOT WORMWOOD_TEAM OR NOT WORMWOOD_ASSETS OR NOT WORMWOOD_RUST_LIBRARY)
 message(FATAL_ERROR "Supply WORMWOOD_TEAM, WORMWOOD_ASSETS and WORMWOOD_RUST_LIBRARY")
endif()
add_executable(Wormwood MACOSX_BUNDLE "${WORMWOOD_ROOT}/Platform/iOS/App.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSGraphics.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSAudio.m" "${WORMWOOD_ROOT}/BaseSystem/LayerOne/IOSGraphicsSymbols.c")
set_target_properties(Wormwood PROPERTIES
 MACOSX_BUNDLE_INFO_PLIST "${CMAKE_CURRENT_LIST_DIR}/Info.plist"
 XCODE_ATTRIBUTE_PRODUCT_BUNDLE_IDENTIFIER "org.salamander.wormwood"
 XCODE_ATTRIBUTE_DEVELOPMENT_TEAM "${WORMWOOD_TEAM}"
 XCODE_ATTRIBUTE_CODE_SIGN_STYLE Automatic
 XCODE_ATTRIBUTE_CODE_SIGN_IDENTITY "Apple Development"
 XCODE_ATTRIBUTE_TARGETED_DEVICE_FAMILY "1,2"
 XCODE_ATTRIBUTE_CLANG_ENABLE_OBJC_ARC YES
 XCODE_ATTRIBUTE_ENABLE_BITCODE NO)
target_link_libraries(Wormwood PRIVATE "${WORMWOOD_RUST_LIBRARY}" "-framework UIKit" "-framework OpenGLES" "-framework QuartzCore" "-framework AudioToolbox" "-framework AVFAudio" "-framework Foundation" "-framework Security" "-framework SystemConfiguration" "-framework CoreFoundation" "-liconv")
add_custom_command(TARGET Wormwood POST_BUILD
 COMMAND "${CMAKE_COMMAND}" -E copy_directory "${WORMWOOD_ASSETS}" "$<TARGET_BUNDLE_DIR:Wormwood>/EngineAssets")

if(NOT WORMWOOD_WGPU_LIBRARY OR NOT WORMWOOD_MOLTENVK_LIBRARY OR NOT WORMWOOD_WGPU_INCLUDE)
 message(FATAL_ERROR "Supply pinned iOS WebGPU/MoltenVK SDK paths")
endif()
target_include_directories(Wormwood PRIVATE "${WORMWOOD_WGPU_INCLUDE}" /opt/homebrew/include)
target_link_libraries(Wormwood PRIVATE "${WORMWOOD_WGPU_LIBRARY}" "${WORMWOOD_MOLTENVK_LIBRARY}" "-framework Metal" "-framework IOSurface" "-framework CoreGraphics" "-lc++")
target_link_options(Wormwood PRIVATE "-Wl,-u,_ww_graphics_symbols" "-Wl,-export_dynamic")
