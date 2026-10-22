// SPDX-License-Identifier: MIT
#pragma once
#include "bdf_font.hpp"
#include <nlohmann/json.hpp>
extern "C" const char *dome_bdf_font_resolve(const char *id, int pixels, int scale) {
  static std::string result;
  try {
    std::string name(id); auto slash = name.find(" / ");
    auto selected = bdf_fonts::resolve(name.substr(0, slash), slash == std::string::npos ? "" : name.substr(slash + 3), pixels, scale);
    auto media=bdf_fonts::qt_alias(selected,"media");
    if(getenv("DOME_BDF_QT_ALIAS"))selected=bdf_fonts::qt_alias(selected);
    result = nlohmann::json{{"file",selected.file},{"family",selected.family},{"style",selected.style},{"pixels",selected.pixels},{"media_family",media.family},{"media_file",media.file},{"fontconfig",bdf_fonts::export_config()},{"fontconfig_base",getenv("DE_XR_FONTCONFIG_BASE") ? getenv("DE_XR_FONTCONFIG_BASE") : "/etc/fonts/fonts.conf"}}.dump();
  } catch (const std::exception &e) { result = nlohmann::json{{"error",e.what()}}.dump(); }
  return result.c_str();
}
