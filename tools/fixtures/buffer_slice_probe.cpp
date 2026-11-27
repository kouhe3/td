// Oracle for tdutils BufferSlice ownership semantics.
//
//   buffer_slice_probe <script.txt>
//
// Script lines (names are arbitrary identifiers):
//   new <name> <hex>             BufferSlice(Slice)
//   clone <name> <src>           clone(): shared buffer, its own window
//   copy <name> <src>            copy(): fresh buffer with a copy of the window
//   mutate <name> <idx> <byte>   as_mutable_slice()[idx] = byte
//   read <name> <n>              confirm_read(n) -> drained and new size
//   truncate <name> <limit>      truncate(limit)
//   after <name> <offset>        after(offset) bytes
//   print <name>                 size + hex of as_slice()
//
// BufferSlice::as_mutable_slice() does *not* copy on write: a clone aliases the
// same allocation, so mutating a clone is visible through the original.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/buffer_slice_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o buffer_slice_probe.exe

#include "td/utils/buffer.h"

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include <unordered_map>

namespace {

std::string unhex(const std::string &text) {
  std::string out;
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    unsigned int byte = 0;
    std::sscanf(text.c_str() + i, "%2x", &byte);
    out.push_back(static_cast<char>(byte));
  }
  return out;
}

std::string hex_of(td::Slice data) {
  std::string out;
  for (unsigned char c : data) {
    char buffer[3];
    std::snprintf(buffer, sizeof(buffer), "%02x", c);
    out += buffer;
  }
  return out;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: buffer_slice_probe <script.txt>\n");
    return 2;
  }
  std::ifstream input(argv[1], std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::unordered_map<std::string, td::BufferSlice> slices;
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    std::string name;
    if (op == "new") {
      std::string data;
      tokens >> name >> data;
      auto bytes = unhex(data);
      slices[name] = td::BufferSlice(td::Slice(bytes));
      std::printf("new %s size=%zu\n", name.c_str(), slices[name].size());
      continue;
    }
    if (op == "clone" || op == "copy") {
      std::string src;
      tokens >> name >> src;
      auto it = slices.find(src);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", src.c_str());
        return 2;
      }
      slices[name] = op == "clone" ? it->second.clone() : it->second.copy();
      std::printf("%s %s from %s size=%zu\n", op.c_str(), name.c_str(), src.c_str(), slices[name].size());
      continue;
    }
    if (op == "mutate") {
      size_t index = 0;
      unsigned int byte = 0;
      tokens >> name >> index >> byte;
      auto it = slices.find(name);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", name.c_str());
        return 2;
      }
      it->second.as_mutable_slice()[index] = static_cast<char>(byte);
      std::printf("mutate %s idx=%zu byte=%02x\n", name.c_str(), index, byte);
      continue;
    }
    if (op == "read") {
      size_t count = 0;
      tokens >> name >> count;
      auto it = slices.find(name);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", name.c_str());
        return 2;
      }
      bool drained = it->second.confirm_read(count);
      std::printf("read %s drained=%d size=%zu\n", name.c_str(), drained ? 1 : 0, it->second.size());
      continue;
    }
    if (op == "truncate") {
      size_t limit = 0;
      tokens >> name >> limit;
      auto it = slices.find(name);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", name.c_str());
        return 2;
      }
      it->second.truncate(limit);
      std::printf("truncate %s size=%zu\n", name.c_str(), it->second.size());
      continue;
    }
    if (op == "after") {
      size_t offset = 0;
      tokens >> name >> offset;
      auto it = slices.find(name);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", name.c_str());
        return 2;
      }
      std::printf("after %s hex=%s\n", name.c_str(), hex_of(it->second.after(offset)).c_str());
      continue;
    }
    if (op == "print") {
      tokens >> name;
      auto it = slices.find(name);
      if (it == slices.end()) {
        std::fprintf(stderr, "unknown slice %s\n", name.c_str());
        return 2;
      }
      std::printf("print %s size=%zu hex=%s\n", name.c_str(), it->second.size(), hex_of(it->second.as_slice()).c_str());
      continue;
    }
    std::fprintf(stderr, "unknown op %s\n", op.c_str());
    return 2;
  }
  return 0;
}