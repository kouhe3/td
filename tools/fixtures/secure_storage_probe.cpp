// Oracle for `td/telegram/SecureStorage.{h,cpp}`, driven by a script:
//
//   secure_storage_probe <script.txt>
//
// Commands (`-` is the empty byte string):
//   vhash <hex>                       calc_value_hash(Slice)
//   dhash <hex>                       calc_value_hash(BufferSliceDataView) + chunk log
//   vcreate <hex>                     ValueHash::create
//   aes <hex seed>                    calc_aes_cbc_state_sha512 -> encrypt(32 zero bytes)
//   pbkdf2 <hex secret> <hex salt>    calc_aes_cbc_state_pbkdf2 -> same
//   skey <hex32>                      Secret::create (+ get_hash / as_slice)
//   enc <hex key> <hex salt> <0|1>    Secret::encrypt(Sha512|Pbkdf2)
//   dec <hex enc> <hex key> <hex salt> <0|1>   EncryptedSecret::decrypt
//   prefix <n>                        gen_random_prefix
//   encval <hex data>                 encrypt_value with a fixed secret
//   decval <hex hash> <hex data>      decrypt_value with a fixed secret
//   dstate <hex seed>                 new Decryptor
//   dappend <hex>                     Decryptor::append
//   dfinish                           Decryptor::finish
//   estate <hex seed> <hex data>      new Encryptor over a BufferSliceDataView
//   epread <offset> <size>            Encryptor::pread
//
// Build (`SecureStorage.cpp` lives in libtdcore.a; only libtdutils is needed on
// top of it):
//
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I . -I td/generate/auto -I tdutils
//   -I tdactor -I C:/tmp/td-build/tdutils tools/fixtures/secure_storage_probe.cpp
//   -Wl,--start-group C:/tmp/td-build/libtdcore.a C:/tmp/td-build/libtdapi.a
//   C:/tmp/td-build/tdutils/libtdutils.a C:/tmp/td-build/tdactor/libtdactor.a
//   -Wl,--end-group -lssl -lcrypto -lz -lws2_32 -lpsapi -lshell32
//   -o C:/tmp/td-probe/secure_storage_probe.exe
//
// Measured behaviour this probe encodes (docs/facts.md F244): the hashes and the
// two AesCbcState derivations are deterministic, `Secret::create` needs a
// checksum of 239 mod 255, the Decryptor zeroes `to_skip_` when the prefix byte
// exceeds the first part, `finish` hashes the whole plaintext (prefix included),
// and a one-bit flip in the *last* ciphertext block reaches the
// `Hash mismatch` message without breaking the prefix check.

#include "td/telegram/SecureStorage.h"

#include "td/utils/buffer.h"
#include "td/utils/common.h"
#include "td/utils/crypto.h"
#include "td/utils/misc.h"
#include "td/utils/Slice.h"
#include "td/utils/Status.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

namespace {

using td::Slice;
using td::string;
namespace ss = td::secure_storage;

// A valid secret: 00..00 EF has sum 239 and 239 % 255 == 239.
const char *kSecretHex = "00000000000000000000000000000000000000000000000000000000000000ef";

string read_all(const char *path) {
  std::ifstream in(path, std::ios::binary);
  if (!in) {
    std::fprintf(stderr, "cannot open %s\n", path);
    std::exit(2);
  }
  return string(std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>()));
}

std::vector<Slice> split_tokens(Slice line) {
  std::vector<Slice> tokens;
  while (!line.empty()) {
    while (!line.empty() && line[0] == ' ') {
      line.remove_prefix(1);
    }
    if (line.empty()) {
      break;
    }
    size_t i = 0;
    while (i < line.size() && line[i] != ' ') {
      i++;
    }
    tokens.push_back(line.substr(0, i));
    line.remove_prefix(i);
  }
  return tokens;
}

Slice trim_line(Slice line) {
  while (!line.empty() && (line.back() == '\r' || line.back() == ' ' || line.back() == '\t')) {
    line.remove_suffix(1);
  }
  while (!line.empty() && (line[0] == ' ' || line[0] == '\t')) {
    line.remove_prefix(1);
  }
  return line;
}

int hex_digit(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  return -1;
}

string unhex(Slice hex) {
  if (hex == "-") {
    return string();
  }
  string out;
  if (hex.size() % 2 != 0) {
    std::fprintf(stderr, "bad hex\n");
    std::exit(2);
  }
  for (size_t i = 0; i < hex.size(); i += 2) {
    out.push_back(static_cast<char>((hex_digit(hex[i]) << 4) | hex_digit(hex[i + 1])));
  }
  return out;
}

string hex(Slice data) {
  return td::hex_encode(data);
}

class LoggingDataView final : public ss::DataView {
 public:
  explicit LoggingDataView(td::BufferSlice buffer) : buffer_(std::move(buffer)) {
  }
  td::int64 size() const final {
    return static_cast<td::int64>(buffer_.size());
  }
  td::Result<td::BufferSlice> pread(td::int64 offset, td::int64 size) const final {
    log_ += (log_.empty() ? "" : ",") + std::to_string(offset) + ":" + std::to_string(size);
    return td::BufferSlice(buffer_.as_slice().substr(static_cast<size_t>(offset), static_cast<size_t>(size)));
  }
  const string &log() const {
    return log_;
  }

 private:
  td::BufferSlice buffer_;
  mutable string log_;
};

void print_aes(td::AesCbcState state, const char *tag) {
  string block(32, '\0');
  string out(32, '\0');
  state.encrypt(block, out);
  std::printf("%s=%s\n", tag, hex(out).c_str());
}

}  // namespace

int main(int argc, char **argv) {
  std::setvbuf(stdout, nullptr, _IONBF, 0);
  if (argc != 2) {
    std::fprintf(stderr, "usage: probe <script.txt>\n");
    return 2;
  }

  auto script = read_all(argv[1]);
  ss::Secret secret = ss::Secret::create(unhex(Slice(kSecretHex))).move_as_ok();
  std::unique_ptr<ss::Decryptor> decryptor;
  std::unique_ptr<ss::Encryptor> encryptor;
  std::unique_ptr<LoggingDataView> encryptor_view;

  for (auto raw_line : td::full_split(Slice(script), '\n')) {
    auto line = trim_line(raw_line);
    if (line.empty() || line[0] == '#') {
      continue;
    }
    auto tokens = split_tokens(line);
    auto command = tokens[0];
    if (command == "vhash") {
      auto data = unhex(tokens[1]);
      auto hash = ss::calc_value_hash(data);
      std::printf("vhash=%s\n", hex(hash.as_slice()).c_str());
    } else if (command == "dhash") {
      LoggingDataView view(td::BufferSlice(unhex(tokens[1])));
      auto result = ss::calc_value_hash(view);
      if (result.is_error()) {
        std::printf("dhash err=%s\n", result.error().message().c_str());
      } else {
        std::printf("dhash=%s chunks=%s\n", hex(result.ok().as_slice()).c_str(), view.log().c_str());
      }
    } else if (command == "vcreate") {
      auto result = ss::ValueHash::create(unhex(tokens[1]));
      if (result.is_error()) {
        std::printf("vcreate err=%s\n", result.error().message().c_str());
      } else {
        std::printf("vcreate ok=%s\n", hex(result.ok().as_slice()).c_str());
      }
    } else if (command == "aes") {
      auto state = ss::calc_aes_cbc_state_sha512(unhex(tokens[1]));
      print_aes(std::move(state), "aes");
    } else if (command == "pbkdf2") {
      auto state = ss::calc_aes_cbc_state_pbkdf2(unhex(tokens[1]), unhex(tokens[2]));
      print_aes(std::move(state), "pbkdf2");
    } else if (command == "skey") {
      auto result = ss::Secret::create(unhex(tokens[1]));
      if (result.is_error()) {
        std::printf("skey err=%s\n", result.error().message().c_str());
      } else {
        auto &created = result.ok();
        std::printf("skey hash=%lld hex=%s\n", static_cast<long long>(created.get_hash()),
                    hex(created.as_slice()).c_str());
      }
    } else if (command == "enc") {
      auto algorithm = tokens[3] == "1" ? ss::EnryptionAlgorithm::Pbkdf2 : ss::EnryptionAlgorithm::Sha512;
      auto encrypted = secret.encrypt(unhex(tokens[1]), unhex(tokens[2]), algorithm);
      std::printf("enc=%s\n", hex(encrypted.as_slice()).c_str());
    } else if (command == "dec") {
      auto algorithm = tokens[4] == "1" ? ss::EnryptionAlgorithm::Pbkdf2 : ss::EnryptionAlgorithm::Sha512;
      auto created = ss::EncryptedSecret::create(unhex(tokens[1]));
      if (created.is_error()) {
        std::printf("dec err=%s\n", created.error().message().c_str());
        continue;
      }
      auto encrypted = created.move_as_ok();
      auto result = encrypted.decrypt(unhex(tokens[2]), unhex(tokens[3]), algorithm);
      if (result.is_error()) {
        std::printf("dec err=%s\n", result.error().message().c_str());
      } else {
        std::printf("dec ok hash=%lld hex=%s\n", static_cast<long long>(result.ok().get_hash()),
                    hex(result.ok().as_slice()).c_str());
      }
    } else if (command == "prefix") {
      auto prefix = ss::gen_random_prefix(td::to_integer<td::int64>(tokens[1]));
      std::printf("prefix len=%zu first=%d\n", prefix.size(),
                  prefix.size() == 0 ? -1 : static_cast<int>(static_cast<td::uint8>(prefix.as_slice()[0])));
    } else if (command == "encval") {
      auto result = ss::encrypt_value(secret, unhex(tokens[1]));
      if (result.is_error()) {
        std::printf("encval err=%s\n", result.error().message().c_str());
      } else {
        std::printf("encval hash=%s data=%s\n", hex(result.ok().hash.as_slice()).c_str(),
                    hex(result.ok().data.as_slice()).c_str());
      }
    } else if (command == "decval") {
      auto hash = ss::ValueHash::create(unhex(tokens[1])).move_as_ok();
      auto result = ss::decrypt_value(secret, hash, unhex(tokens[2]));
      if (result.is_error()) {
        std::printf("decval err=%s\n", result.error().message().c_str());
      } else {
        auto &value = result.ok();
        std::printf("decval ok len=%zu hex=%s\n", value.size(),
                    value.empty() ? "-" : hex(value.as_slice()).c_str());
      }
    } else if (command == "dstate") {
      decryptor = std::make_unique<ss::Decryptor>(ss::calc_aes_cbc_state_sha512(unhex(tokens[1])));
    } else if (command == "dappend") {
      auto result = decryptor->append(td::BufferSlice(unhex(tokens[1])));
      if (result.is_error()) {
        std::printf("dappend err=%s\n", result.error().message().c_str());
      } else {
        auto &value = result.ok();
        std::printf("dappend ok len=%zu hex=%s\n", value.size(),
                    value.empty() ? "-" : hex(value.as_slice()).c_str());
      }
    } else if (command == "dfinish") {
      auto result = decryptor->finish();
      if (result.is_error()) {
        std::printf("dfinish err=%s\n", result.error().message().c_str());
      } else {
        std::printf("dfinish ok=%s\n", hex(result.ok().as_slice()).c_str());
      }
    } else if (command == "estate") {
      auto state = ss::calc_aes_cbc_state_sha512(unhex(tokens[1]));
      encryptor_view = std::make_unique<LoggingDataView>(td::BufferSlice(unhex(tokens[2])));
      encryptor = std::make_unique<ss::Encryptor>(std::move(state), *encryptor_view);
    } else if (command == "epread") {
      auto result = encryptor->pread(td::to_integer<td::int64>(tokens[1]), td::to_integer<td::int64>(tokens[2]));
      if (result.is_error()) {
        std::printf("epread err=%s\n", result.error().message().c_str());
      } else {
        auto &value = result.ok();
        std::printf("epread ok len=%zu hex=%s chunks=%s\n", value.size(),
                    value.empty() ? "-" : hex(value.as_slice()).c_str(), encryptor_view->log().c_str());
      }
    } else {
      std::fprintf(stderr, "unknown command %s\n", command.str().c_str());
      return 2;
    }
  }
  return 0;
}