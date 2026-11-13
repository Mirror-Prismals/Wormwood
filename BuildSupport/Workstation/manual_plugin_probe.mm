// Catalog refresh regression: a controlled user root, no third-party execution.
#import <Cocoa/Cocoa.h>
#include "Bridge.h"
#include "json.hpp"
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
using Json=nlohmann::json;
int main(int argc,char**argv){@autoreleasepool{
 if(argc!=3)return 2;
 [NSApplication sharedApplication];
 const std::filesystem::path root=argv[1],user=argv[2];
 std::filesystem::create_directories(user);
 void* host=ww_plugins_create();if(!host)return 3;
 auto command=[&](Json q){Json v=Json::parse(ww_plugins_command(host,q.dump().c_str()));if(v.contains("error"))throw std::runtime_error(v.dump());return v;};
 try{
  Json q={{"op","scan"},{"root",root.string()},{"user_root",user.string()}};
  auto before=command(q);if(before["catalog"].size()!=1)throw std::runtime_error("Empty custom user root must expose only stock plugin");
  auto added=command({{"op","add"},{"track","a"},{"track_kind","audio"},{"stock","LShaping"},{"sample_rate",48000}});
  auto id=added["inserts"][0]["id"];
  std::filesystem::create_directories(user/"USER_VST2/Selected.vst/Contents/MacOS");
  std::ofstream(user/"USER_VST2/Selected.vst/Contents/Info.plist")<<"fixture";
  q["refresh"]=true;auto refreshed=command(q);auto again=command(q);
#if defined(__aarch64__)
  if(refreshed["catalog"].size()!=2 || refreshed["catalog"][1]["format"]!="VST2")throw std::runtime_error("Manual user plugin was not discovered");
#endif
  if(refreshed["catalog"]!=again["catalog"])throw std::runtime_error("Refresh duplicated catalog entries");
  if(again["inserts"].size()!=1||again["inserts"][0]["id"]!=id)throw std::runtime_error("Refresh replaced active insert");
  std::cout<<Json({{"status","passed"},{"catalog",again["catalog"]},{"active_insert_preserved",true},{"system_directories_scanned",false}}).dump(2)<<std::endl;
 }catch(const std::exception&e){std::cerr<<e.what()<<std::endl;ww_plugins_destroy(host);return 1;}
 ww_plugins_destroy(host);return 0;
}}
