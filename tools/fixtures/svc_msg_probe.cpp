// Oracle for the maintenance/service messages TDLib's generated classes cover:
// bad_msg_notification, bad_server_salt, new_session_created, pong,
// msg_resend_req, msg_detailed_info, msg_new_detailed_info.
//
// svc_msg_probe bmn <bad_msg_id> <bad_msg_seqno> <error_code>
// svc_msg_probe bss <bad_msg_id> <bad_msg_seqno> <error_code> <new_server_salt>
// svc_msg_probe nsc <first_msg_id> <unique_id> <server_salt>
// svc_msg_probe pong <msg_id> <ping_id>
// svc_msg_probe resend <id64,id64,...>
// svc_msg_probe mdinfo <msg_id> <answer_msg_id> <bytes> <status>
// svc_msg_probe mdnew <answer_msg_id> <bytes> <status>
// svc_msg_probe parse_bmn <bytes_hex>
// svc_msg_probe parse_mdi <bytes_hex>
//
// Store modes print:  id=<8 hex> fields=<hex> full=<hex>
// Parse modes print:  type=<name> + every field, or status=err text=<parser error>
//
// `store` writes only the fields, exactly like the generated storers; `full`
// prepends the constructor id, so a `parse_*` of `full` exercises dispatch.
//
// Build (from a configured C:/tmp/td-build):
//   g++ -std=c++17 -O2 -fno-exceptions -fno-rtti -I <repo> -I <repo>/tdutils \
//       -I C:/tmp/td-build/tdutils -I <repo>/td/generate/auto \
//       tools/fixtures/svc_msg_probe.cpp -Wl,--start-group C:/tmp/td-build/libtdmtproto.a \
//       C:/tmp/td-build/libtdapi.a C:/tmp/td-build/tdutils/libtdutils.a \
//       C:/tmp/td-build/tdactor/libtdactor.a -Wl,--end-group -lssl -lcrypto -lz \
//       -lws2_32 -liphlpapi -lcrypt32 -lpsapi -lmswsock -o svc_msg_probe.exe

#include "td/mtproto/mtproto_api.h"

#include "td/utils/Slice.h"
#include "td/utils/common.h"
#include "td/utils/tl_parsers.h"
#include "td/utils/tl_storers.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace {

td::string unhex(const char *s) {
  td::string res;
  size_t len = std::strlen(s);
  for (size_t i = 0; i + 1 < len; i += 2) {
    unsigned int byte = 0;
    std::sscanf(s + i, "%2x", &byte);
    res.push_back(static_cast<char>(byte));
  }
  return res;
}

void print_hex(const char *name, td::Slice data) {
  std::printf("%s=", name);
  for (unsigned char c : data) {
    std::printf("%02x", c);
  }
}

template <class T>
void store_and_print(const T &object) {
  td::TlStorerCalcLength calc;
  object.store(calc);
  std::string fields(calc.get_length(), '\0');
  td::TlStorerUnsafe storer(reinterpret_cast<td::uint8 *>(fields.data()));
  object.store(storer);

  td::uint32 id = static_cast<td::uint32>(T::ID);
  std::string full;
  for (int i = 0; i < 4; i++) {
    full.push_back(static_cast<char>((id >> (8 * i)) & 0xff));
  }
  full += fields;

  std::printf("id=%08x ", id);
  print_hex("fields", td::Slice(fields));
  std::printf(" ");
  print_hex("full", td::Slice(full));
  std::printf("\n");
}

std::vector<td::int64> parse_ids(const char *text) {
  std::vector<td::int64> ids;
  const char *cursor = text;
  while (*cursor != '\0') {
    char *end = nullptr;
    long long value = std::strtoll(cursor, &end, 10);
    if (end == cursor) {
      break;
    }
    ids.push_back(static_cast<td::int64>(value));
    cursor = end;
    if (*cursor == ',') {
      cursor++;
    }
  }
  return ids;
}

void parse_bmn(const char *hex) {
  auto bytes = unhex(hex);
  td::TlParser parser{td::Slice(bytes)};
  auto object = td::mtproto_api::BadMsgNotification::fetch(parser);
  parser.fetch_end();
  if (parser.get_error() != nullptr) {
    std::printf("status=err text=%s\n", parser.get_error());
    return;
  }
  if (object->get_id() == td::mtproto_api::bad_server_salt::ID) {
    const auto &value = static_cast<const td::mtproto_api::bad_server_salt &>(*object);
    std::printf("type=bad_server_salt\n");
    std::printf("bad_msg_id=%lld\n", static_cast<long long>(value.bad_msg_id_));
    std::printf("bad_msg_seqno=%d\n", value.bad_msg_seqno_);
    std::printf("error_code=%d\n", value.error_code_);
    std::printf("new_server_salt=%lld\n", static_cast<long long>(value.new_server_salt_));
  } else if (object->get_id() == td::mtproto_api::bad_msg_notification::ID) {
    const auto &value = static_cast<const td::mtproto_api::bad_msg_notification &>(*object);
    std::printf("type=bad_msg_notification\n");
    std::printf("bad_msg_id=%lld\n", static_cast<long long>(value.bad_msg_id_));
    std::printf("bad_msg_seqno=%d\n", value.bad_msg_seqno_);
    std::printf("error_code=%d\n", value.error_code_);
  } else {
    std::printf("type=other id=%08x\n", static_cast<td::uint32>(object->get_id()));
  }
}

void parse_mdi(const char *hex) {
  auto bytes = unhex(hex);
  td::TlParser parser{td::Slice(bytes)};
  auto object = td::mtproto_api::MsgDetailedInfo::fetch(parser);
  parser.fetch_end();
  if (parser.get_error() != nullptr) {
    std::printf("status=err text=%s\n", parser.get_error());
    return;
  }
  if (object->get_id() == td::mtproto_api::msg_detailed_info::ID) {
    const auto &value = static_cast<const td::mtproto_api::msg_detailed_info &>(*object);
    std::printf("type=msg_detailed_info\n");
    std::printf("msg_id=%lld\n", static_cast<long long>(value.msg_id_));
    std::printf("answer_msg_id=%lld\n", static_cast<long long>(value.answer_msg_id_));
    std::printf("bytes=%d\n", value.bytes_);
    std::printf("status_code=%d\n", value.status_);
  } else if (object->get_id() == td::mtproto_api::msg_new_detailed_info::ID) {
    const auto &value = static_cast<const td::mtproto_api::msg_new_detailed_info &>(*object);
    std::printf("type=msg_new_detailed_info\n");
    std::printf("answer_msg_id=%lld\n", static_cast<long long>(value.answer_msg_id_));
    std::printf("bytes=%d\n", value.bytes_);
    std::printf("status_code=%d\n", value.status_);
  } else {
    std::printf("type=other id=%08x\n", static_cast<td::uint32>(object->get_id()));
  }
}

}  // namespace

int main(int argc, char **argv) {
  if (argc < 3) {
    std::fprintf(stderr, "usage: svc_msg_probe <mode> ...\n");
    return 2;
  }
  std::string mode = argv[1];
  if (mode == "bmn" && argc >= 5) {
    store_and_print(td::mtproto_api::bad_msg_notification(
        std::atoll(argv[2]), std::atoi(argv[3]), std::atoi(argv[4])));
    return 0;
  }
  if (mode == "bss" && argc >= 6) {
    store_and_print(td::mtproto_api::bad_server_salt(
        std::atoll(argv[2]), std::atoi(argv[3]), std::atoi(argv[4]), std::atoll(argv[5])));
    return 0;
  }
  if (mode == "nsc" && argc >= 5) {
    store_and_print(td::mtproto_api::new_session_created(
        std::atoll(argv[2]), std::atoll(argv[3]), std::atoll(argv[4])));
    return 0;
  }
  if (mode == "pong" && argc >= 4) {
    store_and_print(td::mtproto_api::pong(std::atoll(argv[2]), std::atoll(argv[3])));
    return 0;
  }
  if (mode == "resend" && argc >= 3) {
    td::mtproto_api::array<td::int64> ids;
    for (auto id : parse_ids(argv[2])) {
      ids.push_back(id);
    }
    store_and_print(td::mtproto_api::msg_resend_req(std::move(ids)));
    return 0;
  }
  if (mode == "mdinfo" && argc >= 6) {
    store_and_print(td::mtproto_api::msg_detailed_info(
        std::atoll(argv[2]), std::atoll(argv[3]), std::atoi(argv[4]), std::atoi(argv[5])));
    return 0;
  }
  if (mode == "mdnew" && argc >= 5) {
    store_and_print(td::mtproto_api::msg_new_detailed_info(
        std::atoll(argv[2]), std::atoi(argv[3]), std::atoi(argv[4])));
    return 0;
  }
  if (mode == "parse_bmn" && argc >= 3) {
    parse_bmn(argv[2]);
    return 0;
  }
  if (mode == "parse_mdi" && argc >= 3) {
    parse_mdi(argv[2]);
    return 0;
  }
  std::fprintf(stderr, "bad arguments for mode %s\n", mode.c_str());
  return 2;
}