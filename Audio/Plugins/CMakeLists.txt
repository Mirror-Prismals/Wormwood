cmake_minimum_required(VERSION 3.20)
project(wormwood_plugins LANGUAGES CXX OBJCXX)
if(CMAKE_SYSTEM_NAME STREQUAL "iOS")
  message(FATAL_ERROR "Third-party plugin hosting is deliberately excluded from iOS")
endif()
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_OBJCXX_STANDARD 17)
set(SDK ${CMAKE_CURRENT_SOURCE_DIR}/vendor/vst3sdk)
set(SDK_SOURCES
 base/source/baseiids.cpp base/source/fbuffer.cpp base/source/fdebug.cpp
 base/source/fdynlib.cpp base/source/fobject.cpp base/source/fstreamer.cpp
 base/source/fstring.cpp base/source/timer.cpp base/source/updatehandler.cpp
 base/thread/source/fcondition.cpp base/thread/source/flock.cpp
 pluginterfaces/base/conststringtable.cpp pluginterfaces/base/coreiids.cpp
 pluginterfaces/base/funknown.cpp pluginterfaces/base/ustring.cpp
 public.sdk/source/common/commoniids.cpp public.sdk/source/common/commonstringconvert.cpp
 public.sdk/source/common/openurl.cpp public.sdk/source/common/readfile.cpp
 public.sdk/source/common/memorystream.cpp
 public.sdk/source/vst/vstpresetfile.cpp public.sdk/source/vst/utility/stringconvert.cpp
 public.sdk/source/vst/hosting/connectionproxy.cpp public.sdk/source/vst/hosting/eventlist.cpp
 public.sdk/source/vst/hosting/hostclasses.cpp public.sdk/source/vst/hosting/module.cpp
 public.sdk/source/vst/hosting/parameterchanges.cpp public.sdk/source/vst/hosting/pluginterfacesupport.cpp
 public.sdk/source/vst/hosting/plugprovider.cpp public.sdk/source/vst/hosting/processdata.cpp
 public.sdk/source/vst/vstinitiids.cpp
 public.sdk/source/common/threadchecker_mac.mm public.sdk/source/common/systemclipboard_mac.mm
 public.sdk/source/vst/hosting/module_mac.mm)
list(TRANSFORM SDK_SOURCES PREPEND "${SDK}/")
add_library(wormwood_plugins STATIC ${SDK_SOURCES}
 native/Vst3.cpp native/Clap.cpp native/AudioUnit.mm native/NativeUI.mm
 native/Vst2/HostedPlugin.cpp native/Vst2/AudioBridge.cpp native/Vst2/EditorBridge.cpp native/Vst2/Worker.cpp
 native/Bridge.mm native/StockEq.cpp)
target_include_directories(wormwood_plugins PRIVATE ${SDK} native native/Host vendor vendor/clap/include vendor/clap)
target_compile_definitions(wormwood_plugins PRIVATE RELEASE=1 NDEBUG=1)
target_compile_options(wormwood_plugins PRIVATE -O2 -fPIC "$<$<COMPILE_LANGUAGE:OBJCXX>:-fobjc-arc>")
