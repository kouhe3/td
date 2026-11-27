// Oracle for tdutils Slice / MutableSlice accessors and pointer consistency.
//
//   slice_ptr_probe <script.txt>
//   slice_ptr_probe --check <op> <len>
//
// Script lines:
//   new <name> <hex|->        Slice over a private copy ("-" = empty)
//   mnew <name> <hex|->       MutableSlice over a private copy
//   shift <name> <n>          Slice::remove_prefix(n)
//   mshift <name> <n>         MutableSlice::remove_prefix(n)
//   back <name>               -> `back <name> size=… value=…`    (CHECK on empty)
//   mback <name>              -> `mback <name> …`
//   index <name> <i>          -> `index <name> i=… value=…`     ([0, size), no CHECK)
//   mindex <name> <i>         -> `mindex <name> …`
//   ptr <name>                -> `ptr <name> size=… off=… endoff=… uboff=… ueoff=… same_db=… same_ub=…`
//   mptr <name>               -> `mptr <name> …`
//
// off/endoff/uboff/ueoff are byte offsets from the backing storage, so a view
// that moved keeps `endoff - off == size`. back() on an empty slice aborts, so
// those cases go through --check, one process each. operator[] out of range is
// UB in td, not a contract, and is never exercised.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/slice_ptr_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o slice_ptr_probe.exe

#include "td/utils/Slice.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <map>
#include <sstream>
#include <string>
#include <vector>

namespace {

std::vector<char> unhex(const std::string &text) {
  std::vector<char> bytes;
  if (text == "-") {
    return bytes;
  }
  bytes.reserve(text.size() / 2);
  for (size_t i = 0; i + 1 < text.size(); i += 2) {
    bytes.push_back(static_cast<char>(std::stoi(text.substr(i, 2), nullptr, 16)));
  }
  return bytes;
}

template <class T>
void print_ptr(const char *op, const std::string &name, T &slice, const std::string &storage) {
  const char *base = storage.data();
  std::printf("%s %s size=%llu off=%lld endoff=%lld uboff=%lld ueoff=%lld same_db=%d same_ub=%d\n", op,
              name.c_str(), static_cast<unsigned long long>(slice.size()),
              static_cast<long long>(slice.begin() - base), static_cast<long long>(slice.end() - base),
              static_cast<long long>(reinterpret_cast<const char *>(slice.ubegin()) - base),
              static_cast<long long>(reinterpret_cast<const char *>(slice.uend()) - base),
              static_cast<int>(slice.data() == slice.begin()),
              static_cast<int>(static_cast<const void *>(slice.ubegin()) == static_cast<const void *>(slice.data())));
}

int run_script(const char *path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", path);
    return 2;
  }
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::map<std::string, std::string> storages;
  std::map<std::string, td::Slice> slices;
  std::map<std::string, td::MutableSlice> mslices;
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    std::string name;
    tokens >> name;
    if (op == "new" || op == "mnew") {
      std::string hex;
      tokens >> hex;
      auto bytes = unhex(hex);
      auto &storage = storages[name];
      storage.assign(bytes.begin(), bytes.end());
      slices[name] = td::Slice(storage.data(), storage.size());
      mslices[name] = td::MutableSlice(storage.data(), storage.size());
    } else if (op == "shift" || op == "mshift") {
      size_t count = 0;
      tokens >> count;
      if (op == "shift") {
        slices.at(name).remove_prefix(count);
      } else {
        mslices.at(name).remove_prefix(count);
      }
    } else if (op == "back" || op == "mback") {
      if (op == "back") {
        auto &slice = slices.at(name);
        std::printf("back %s size=%llu value=%d\n", name.c_str(),
                    static_cast<unsigned long long>(slice.size()),
                    static_cast<int>(static_cast<unsigned char>(slice.back())));
      } else {
        auto &slice = mslices.at(name);
        std::printf("mback %s size=%llu value=%d\n", name.c_str(),
                    static_cast<unsigned long long>(slice.size()),
                    static_cast<int>(static_cast<unsigned char>(slice.back())));
      }
    } else if (op == "index" || op == "mindex") {
      size_t index = 0;
      tokens >> index;
      if (op == "index") {
        auto &view = slices.at(name);
        std::printf("index %s i=%llu value=%d\n", name.c_str(), static_cast<unsigned long long>(index),
                    static_cast<int>(static_cast<unsigned char>(view[index])));
      } else {
        auto &view = mslices.at(name);
        std::printf("mindex %s i=%llu value=%d\n", name.c_str(), static_cast<unsigned long long>(index),
                    static_cast<int>(static_cast<unsigned char>(view[index])));
      }
    } else if (op == "ptr" || op == "mptr") {
      if (op == "ptr") {
        print_ptr("ptr", name, slices.at(name), storages.at(name));
      } else {
        print_ptr("mptr", name, mslices.at(name), storages.at(name));
      }
    } else {
      std::printf("unknown %s\n", op.c_str());
    }
  }
  return 0;
}

int run_check(int argc, char **argv) {
  if (argc != 4) {
    std::fprintf(stderr, "usage: slice_ptr_probe --check <back|mback> <len>\n");
    return 2;
  }
  std::string op = argv[2];
  size_t len = std::stoull(argv[3]);
  std::string storage = "abcdefgh";
  if (len > storage.size()) {
    std::fprintf(stderr, "len must be <= 8\n");
    return 2;
  }
  td::Slice slice(storage.data(), len);
  td::MutableSlice mutable_slice(storage.data(), len);
  int value = 0;
  if (op == "back") {
    value = static_cast<unsigned char>(slice.back());
  } else if (op == "mback") {
    value = static_cast<unsigned char>(mutable_slice.back());
  } else {
    std::fprintf(stderr, "unknown op %s\n", op.c_str());
    return 2;
  }
  std::printf("check op=%s len=%llu value=%d\n", op.c_str(), static_cast<unsigned long long>(len), value);
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc >= 2 && std::string(argv[1]) == "--check") {
    return run_check(argc, argv);
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: slice_ptr_probe <script.txt> | --check <back|mback> <len>\n");
    return 2;
  }
  return run_script(argv[1]);
}