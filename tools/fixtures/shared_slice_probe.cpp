// Oracle for tdutils SharedSlice / UniqueSharedSlice / UniqueSlice semantics.
//
//   shared_slice_probe <script.txt>
//
// Script lines (names are arbitrary identifiers):
//   new <name> <hex>             SharedSlice(Slice): create(size) + copy_from
//   default <name>               SharedSlice(): null, no allocation at all
//   clone <name> <src>           src.clone(): shares the allocation, bumps refcnt
//   unique <name> <src>          UniqueSharedSlice(std::move(src)): steals a unique
//                                buffer, deep-copies a shared one; src becomes null
//   to_shared <name> <src>       SharedSlice(std::move(src)): steals the unique impl
//   mutate <name> <idx> <byte>   UniqueSharedSlice::as_mutable_slice()[idx] = byte
//   buffer <name> <src>          src.clone_as_buffer_slice(): BufferSlice copy
//   clear <name>                 clear() for any kind
//   unew <name> <size> <byte>    UniqueSlice(size, c), filled with the byte
//   ucopy <name> <src>           UniqueSlice::copy()
//   umutate <name> <idx> <byte>  UniqueSlice::as_mutable_slice()[idx] = byte
//   uclear <name>                UniqueSlice::clear()
//   snew/scopy/smutate/sclear    the same four on SecureString (zero_on_destruct)
//   print <name>                 kind, size, empty, hex
//
// Two distinctions the script pins down:
//   - a default-constructed SharedSlice and SharedSlice(Slice()) are
//     indistinguishable through the public API: Slice() points its data at "",
//     not nullptr, and is_unique() lives in detail:: only, so both print size=0
//     and both clone/convert to an empty slice;
//   - UniqueSharedSlice does not copy on write: it steals a unique buffer and
//     deep-copies a shared one, and the source is left null either way.
//
// Build (from a configured C:/tmp/td-build; the link line is the one in
// buffer_slice_probe.cpp):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/shared_slice_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a \
//       C:/tmp/td-build/libtdmtproto.a C:/tmp/td-build/tddb/libtddb.a \
//       C:/tmp/td-build/tdnet/libtdnet.a C:/tmp/td-build/tde2e/libtde2e.a \
//       C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a \
//       C:/tmp/td-build/sqlite/libtdsqlite.a -Wl,--end-group \
//       -lssl -lcrypto -lz -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock \
//       -lshell32 -o shared_slice_probe.exe

#include "td/utils/SharedSlice.h"

#include "td/utils/Slice.h"
#include "td/utils/buffer.h"

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

int unknown(const std::string &name) {
  std::fprintf(stderr, "unknown slice %s\n", name.c_str());
  return 2;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fprintf(stderr, "usage: shared_slice_probe <script.txt>\n");
    return 2;
  }
  std::ifstream input(argv[1], std::ios::binary);
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::unordered_map<std::string, td::SharedSlice> shared;
  std::unordered_map<std::string, td::UniqueSharedSlice> unique_shared;
  std::unordered_map<std::string, td::BufferSlice> buffers;
  std::unordered_map<std::string, td::UniqueSlice> uniques;
  std::unordered_map<std::string, td::SecureString> secures;
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
      shared[name] = td::SharedSlice(td::Slice(bytes));
      std::printf("new %s size=%zu\n", name.c_str(), shared[name].size());
      continue;
    }
    if (op == "default") {
      tokens >> name;
      shared[name] = td::SharedSlice();
      std::printf("default %s size=0\n", name.c_str());
      continue;
    }
    if (op == "clone") {
      std::string src;
      tokens >> name >> src;
      auto it = shared.find(src);
      if (it == shared.end()) {
        return unknown(src);
      }
      auto made = it->second.clone();
      shared[name] = std::move(made);
      std::printf("clone %s from %s size=%zu\n", name.c_str(), src.c_str(), shared[name].size());
      continue;
    }
    if (op == "unique") {
      std::string src;
      tokens >> name >> src;
      auto it = shared.find(src);
      if (it == shared.end()) {
        return unknown(src);
      }
      td::UniqueSharedSlice made(std::move(it->second));
      unique_shared[name] = std::move(made);
      std::printf("unique %s from %s size=%zu\n", name.c_str(), src.c_str(), unique_shared[name].size());
      continue;
    }
    if (op == "to_shared") {
      std::string src;
      tokens >> name >> src;
      auto it = unique_shared.find(src);
      if (it == unique_shared.end()) {
        return unknown(src);
      }
      td::SharedSlice made(std::move(it->second));
      shared[name] = std::move(made);
      std::printf("to_shared %s from %s size=%zu\n", name.c_str(), src.c_str(), shared[name].size());
      continue;
    }
    if (op == "mutate") {
      size_t index = 0;
      unsigned int byte = 0;
      tokens >> name >> index >> byte;
      auto it = unique_shared.find(name);
      if (it == unique_shared.end()) {
        return unknown(name);
      }
      it->second.as_mutable_slice()[index] = static_cast<char>(byte);
      std::printf("mutate %s idx=%zu byte=%02x\n", name.c_str(), index, byte);
      continue;
    }
    if (op == "buffer") {
      std::string src;
      tokens >> name >> src;
      auto it = shared.find(src);
      if (it == shared.end()) {
        return unknown(src);
      }
      buffers[name] = it->second.clone_as_buffer_slice();
      std::printf("buffer %s from %s size=%zu\n", name.c_str(), src.c_str(), buffers[name].size());
      continue;
    }
    if (op == "clear") {
      tokens >> name;
      if (auto it = shared.find(name); it != shared.end()) {
        it->second.clear();
      } else if (auto it = unique_shared.find(name); it != unique_shared.end()) {
        it->second.clear();
      } else if (auto it = buffers.find(name); it != buffers.end()) {
        it->second.clear();
      } else if (auto it = uniques.find(name); it != uniques.end()) {
        it->second.clear();
      } else if (auto it = secures.find(name); it != secures.end()) {
        it->second.clear();
      } else {
        return unknown(name);
      }
      std::printf("clear %s\n", name.c_str());
      continue;
    }
    if (op == "unew" || op == "snew") {
      size_t size = 0;
      unsigned int byte = 0;
      tokens >> name >> size >> byte;
      if (op == "unew") {
        uniques[name] = td::UniqueSlice(size, static_cast<char>(byte));
        std::printf("unew %s size=%zu\n", name.c_str(), uniques[name].size());
      } else {
        secures[name] = td::SecureString(size, static_cast<char>(byte));
        std::printf("snew %s size=%zu\n", name.c_str(), secures[name].size());
      }
      continue;
    }
    if (op == "ucopy" || op == "scopy") {
      std::string src;
      tokens >> name >> src;
      if (op == "ucopy") {
        auto it = uniques.find(src);
        if (it == uniques.end()) {
          return unknown(src);
        }
        auto made = it->second.copy();
        uniques[name] = std::move(made);
        std::printf("ucopy %s from %s size=%zu\n", name.c_str(), src.c_str(), uniques[name].size());
      } else {
        auto it = secures.find(src);
        if (it == secures.end()) {
          return unknown(src);
        }
        auto made = it->second.copy();
        secures[name] = std::move(made);
        std::printf("scopy %s from %s size=%zu\n", name.c_str(), src.c_str(), secures[name].size());
      }
      continue;
    }
    if (op == "umutate" || op == "smutate") {
      size_t index = 0;
      unsigned int byte = 0;
      tokens >> name >> index >> byte;
      if (op == "umutate") {
        auto it = uniques.find(name);
        if (it == uniques.end()) {
          return unknown(name);
        }
        it->second.as_mutable_slice()[index] = static_cast<char>(byte);
      } else {
        auto it = secures.find(name);
        if (it == secures.end()) {
          return unknown(name);
        }
        it->second.as_mutable_slice()[index] = static_cast<char>(byte);
      }
      std::printf("%s %s idx=%zu byte=%02x\n", op.c_str(), name.c_str(), index, byte);
      continue;
    }
    if (op == "uclear" || op == "sclear") {
      tokens >> name;
      if (op == "uclear") {
        auto it = uniques.find(name);
        if (it == uniques.end()) {
          return unknown(name);
        }
        it->second.clear();
      } else {
        auto it = secures.find(name);
        if (it == secures.end()) {
          return unknown(name);
        }
        it->second.clear();
      }
      std::printf("%s %s\n", op.c_str(), name.c_str());
      continue;
    }
    if (op == "print") {
      tokens >> name;
      if (auto it = shared.find(name); it != shared.end()) {
        std::printf("print %s kind=shared size=%zu empty=%d hex=%s\n", name.c_str(), it->second.size(),
                    it->second.empty() ? 1 : 0, hex_of(it->second.as_slice()).c_str());
        continue;
      }
      if (auto it = unique_shared.find(name); it != unique_shared.end()) {
        std::printf("print %s kind=unique_shared size=%zu empty=%d hex=%s\n", name.c_str(), it->second.size(),
                    it->second.empty() ? 1 : 0, hex_of(it->second.as_slice()).c_str());
        continue;
      }
      if (auto it = buffers.find(name); it != buffers.end()) {
        std::printf("print %s kind=buffer size=%zu hex=%s\n", name.c_str(), it->second.size(),
                    hex_of(it->second.as_slice()).c_str());
        continue;
      }
      if (auto it = uniques.find(name); it != uniques.end()) {
        std::printf("print %s kind=unique size=%zu empty=%d hex=%s\n", name.c_str(), it->second.size(),
                    it->second.empty() ? 1 : 0, hex_of(it->second.as_slice()).c_str());
        continue;
      }
      if (auto it = secures.find(name); it != secures.end()) {
        std::printf("print %s kind=secure size=%zu empty=%d hex=%s\n", name.c_str(), it->second.size(),
                    it->second.empty() ? 1 : 0, hex_of(it->second.as_slice()).c_str());
        continue;
      }
      return unknown(name);
    }
    std::fprintf(stderr, "unknown op %s\n", op.c_str());
    return 2;
  }
  return 0;
}
