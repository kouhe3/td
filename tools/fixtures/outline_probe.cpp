// Oracle for td::get_outline_svg_path (td/telegram/Outline.cpp).
//
//   outline_probe <script.txt>
//
//   path <zoom> <svg>   encode <svg> into TDLib's compact path form (the inverse
//                       of the decoder in Outline.cpp), then run the real
//                       `get_outline_svg_path` and print the result
//
// The compact form is the one the JNI/Client layer produces: every character
// < 192 stands for a command byte, `>= 192` indexes the table
// `AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,`.
// The probe therefore only accepts characters from that table (spaces are
// turned into commas, which the SVG grammar treats as separators).
//
// Build (`Outline.cpp` lives in libtdcore.a; the full static set is needed
// because tdcore pulls in the managers):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I tddb -I C:/tmp/td-build/tdutils tools/fixtures/outline_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tdnet/libtdnet.a
//   C:/tmp/td-build/tdtl/libtdtl.a C:/tmp/td-build/tde2e/libtde2e.a
//   C:/tmp/td-build/tddb/libtddb.a C:/tmp/td-build/tdutils/libtdutils.a
//   C:/tmp/td-build/tdactor/libtdactor.a C:/tmp/td-build/sqlite/libtdsqlite.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lcrypt32 -lsecur32
//   -lmswsock -liphlpapi -ldnsapi -o C:/tmp/td-probe/outline_probe.exe

#include "td/telegram/Outline.h"

#include "td/utils/Slice.h"
#include "td/utils/misc.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <string>

namespace {

const char *kTable = "AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,";

std::string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

int table_index(char c) {
  for (int i = 0; i < 64; i++) {
    if (kTable[i] == c) {
      return i;
    }
  }
  return -1;
}

std::string encode_svg(const std::string &svg) {
  std::string encoded;
  for (char raw : svg) {
    char c = raw == ' ' ? ',' : raw;
    int index = table_index(c);
    if (index < 0) {
      std::fprintf(stderr, "character '%c' is not in the compact table\n", c);
      std::exit(2);
    }
    encoded.push_back(static_cast<char>(192 + index));
  }
  return encoded;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: outline_probe <script.txt>\n");
    return 2;
  }
  auto script = read_all(argv[1]);
  td::Slice rest(script);
  while (!rest.empty()) {
    auto pos = rest.find('\n');
    td::Slice line = pos == td::Slice::npos ? rest : rest.substr(0, pos);
    rest = pos == td::Slice::npos ? td::Slice() : rest.substr(pos + 1);
    if (!line.empty() && line.back() == '\r') {
      line.remove_suffix(1);
    }
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto parts = td::full_split(line, ' ', 3);
    if (parts.size() != 3 || parts[0] != "path") {
      std::fprintf(stderr, "bad line\n");
      return 2;
    }
    auto zoom = std::strtod(parts[1].str().c_str(), nullptr);
    auto svg = parts[2].str();
    auto encoded = encode_svg(svg);
    auto result = td::get_outline_svg_path(encoded, zoom, "outline_probe");
    std::printf("path zoom=%s out=%s\n", parts[1].str().c_str(), result.empty() ? "-" : result.c_str());
  }
  return 0;
}