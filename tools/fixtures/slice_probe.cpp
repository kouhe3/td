// Oracle for tdutils Slice's view operations: remove_prefix / remove_suffix /
// substr / truncate.
//
//   slice_probe <script.txt>
//   slice_probe --check <op> <len> <a> [b]
//
// Script lines:
//   new <name> <hex|->      Slice over a private copy of the bytes ("-" = empty)
//   prefix <name> <n>       remove_prefix(n)      (n <= size)
//   suffix <name> <n>       remove_suffix(n)      (n <= size)
//   trunc <name> <n>        truncate(n)           (never grows, no CHECK)
//   substr <name> <from>    substr(from)          (from <= size)
//   substr2 <name> <from> <n>  substr(from, n)    (length clamped to size - from)
//
// Each op prints `<op> <name> <args> size=… off=… hex=…`, where off is
// view.begin() - storage.data(): remove_prefix and substr move it, truncate and
// remove_suffix do not.
//
// Out-of-range remove_prefix / remove_suffix / substr CHECK-abort, so those
// cases are driven one per process through --check and only "did it return"
// is compared. `--check` prints one line when the op returns.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I <repo>/tdactor -I <repo>/tdnet -I <repo>/tddb \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/slice_probe.cpp -Wl,--start-group \
//       C:/tmp/td-build/tdutils/libtdutils.a -Wl,--end-group \
//       -lssl -lcrypto -lws2_32 -lpsapi -o slice_probe.exe

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

std::string hex_of(td::Slice data) {
  std::string out;
  out.reserve(data.size() * 2);
  for (auto c : data) {
    char buffer[3];
    std::snprintf(buffer, sizeof(buffer), "%02x", static_cast<unsigned char>(c));
    out += buffer;
  }
  return out;
}

struct Holder {
  std::string storage;
  td::Slice view;
};

void dump(const char *op, const std::string &name, const std::string &args, const Holder &holder) {
  std::printf("%s %s %s size=%llu off=%lld hex=%s\n", op, name.c_str(), args.c_str(),
              static_cast<unsigned long long>(holder.view.size()),
              static_cast<long long>(holder.view.begin() - holder.storage.data()), hex_of(holder.view).c_str());
}

int run_script(const char *path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) {
    std::fprintf(stderr, "cannot read %s\n", path);
    return 2;
  }
  std::string text((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
  std::istringstream lines(text);
  std::map<std::string, Holder> holders;
  std::string line;
  while (std::getline(lines, line)) {
    std::istringstream tokens(line);
    std::string op;
    if (!(tokens >> op) || op[0] == '#') {
      continue;
    }
    std::string name;
    tokens >> name;
    if (op == "new") {
      std::string hex;
      tokens >> hex;
      auto bytes = unhex(hex);
      auto &holder = holders[name];
      holder.storage.assign(bytes.begin(), bytes.end());
      holder.view = td::Slice(holder.storage.data(), holder.storage.size());
    } else if (op == "print") {
      dump(op.c_str(), name, "", holders.at(name));
    } else {
      unsigned long long a = 0;
      unsigned long long b = 0;
      tokens >> a;
      auto &holder = holders.at(name);
      if (op == "prefix") {
        holder.view.remove_prefix(a);
        dump(op.c_str(), name, "n=" + std::to_string(a), holder);
      } else if (op == "suffix") {
        holder.view.remove_suffix(a);
        dump(op.c_str(), name, "n=" + std::to_string(a), holder);
      } else if (op == "trunc") {
        holder.view.truncate(a);
        dump(op.c_str(), name, "n=" + std::to_string(a), holder);
      } else if (op == "substr") {
        holder.view = holder.view.substr(a);
        dump(op.c_str(), name, "from=" + std::to_string(a), holder);
      } else if (op == "substr2") {
        tokens >> b;
        holder.view = holder.view.substr(a, b);
        dump(op.c_str(), name, "from=" + std::to_string(a) + " n=" + std::to_string(b), holder);
      } else {
        std::printf("unknown %s\n", op.c_str());
      }
    }
  }
  return 0;
}

int run_check(int argc, char **argv) {
  // argv: --check <op> <len> <a> [b]
  if (argc < 5) {
    std::fprintf(stderr, "usage: slice_probe --check <op> <len> <a> [b]\n");
    return 2;
  }
  std::string op = argv[2];
  size_t len = std::stoull(argv[3]);
  size_t a = std::stoull(argv[4]);
  size_t b = argc > 5 ? std::stoull(argv[5]) : 0;
  std::string storage = "abcdefgh";
  if (len > storage.size()) {
    std::fprintf(stderr, "len must be <= 8\n");
    return 2;
  }
  td::Slice view(storage.data(), len);
  if (op == "prefix") {
    view.remove_prefix(a);
  } else if (op == "suffix") {
    view.remove_suffix(a);
  } else if (op == "trunc") {
    view.truncate(a);
  } else if (op == "substr") {
    view = view.substr(a);
  } else if (op == "substr2") {
    view = view.substr(a, b);
  } else {
    std::fprintf(stderr, "unknown op %s\n", op.c_str());
    return 2;
  }
  std::printf("check op=%s len=%llu a=%llu size=%llu off=%lld hex=%s\n", op.c_str(),
              static_cast<unsigned long long>(len), static_cast<unsigned long long>(a),
              static_cast<unsigned long long>(view.size()),
              static_cast<long long>(view.begin() - storage.data()), hex_of(view).c_str());
  return 0;
}

}  // namespace

int main(int argc, char **argv) {
  // Unbuffered, so an accidental CHECK abort still leaves the lines it wrote.
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc >= 2 && std::string(argv[1]) == "--check") {
    return run_check(argc, argv);
  }
  if (argc != 2) {
    std::fprintf(stderr, "usage: slice_probe <script.txt> | --check <op> <len> <a> [b]\n");
    return 2;
  }
  return run_script(argv[1]);
}